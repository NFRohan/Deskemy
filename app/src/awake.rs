//! Keeping the machine and display awake while a video is actually playing.
//! mpv can't do this itself here: with vo=libmpv it has no window for its
//! stop-screensaver to act on. Called only from the player's long-lived
//! event thread (Windows scopes the request to the calling thread).

#[cfg(windows)]
pub fn keep_awake(on: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };
    let flags = if on {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe { SetThreadExecutionState(flags) };
    tracing::debug!(on, "sleep inhibit");
}

#[cfg(target_os = "linux")]
pub use linux::keep_awake;

#[cfg(not(any(windows, target_os = "linux")))]
pub fn keep_awake(_on: bool) {}

/// The desktop's idle inhibitor over D-Bus (org.freedesktop.ScreenSaver:
/// KDE, GNOME through its screensaver proxy, Xfce, Cinnamon). The
/// inhibition lasts until it's lifted or our bus connection closes, so a
/// crash can't leave the screen awake.
#[cfg(target_os = "linux")]
mod linux {
    use std::cell::RefCell;
    use zbus::blocking::Connection;

    const SERVICE: &str = "org.freedesktop.ScreenSaver";
    const PATH: &str = "/org/freedesktop/ScreenSaver";

    #[derive(Default)]
    struct Inhibitor {
        bus: Option<Connection>,
        /// The held inhibition's cookie.
        cookie: Option<u32>,
        /// Inhibiting failed once (logged; the next tries stay quiet).
        failed: bool,
    }

    thread_local! {
        static INHIBITOR: RefCell<Inhibitor> = RefCell::default();
    }

    pub fn keep_awake(on: bool) {
        INHIBITOR.with_borrow_mut(|i| {
            let result = if on { i.inhibit() } else { i.release() };
            match result {
                Ok(()) => tracing::debug!(on, "sleep inhibit"),
                Err(e) if !i.failed => {
                    i.failed = true;
                    tracing::warn!(error = %e, on, "sleep inhibit (the screen may dim during lectures)");
                }
                Err(_) => {}
            }
        });
    }

    impl Inhibitor {
        fn inhibit(&mut self) -> zbus::Result<()> {
            if self.cookie.is_some() {
                return Ok(());
            }
            let bus = match self.bus.take() {
                Some(bus) => bus,
                None => Connection::session()?,
            };
            let reply = bus.call_method(Some(SERVICE), PATH, Some(SERVICE), "Inhibit", &("Deskemy", "Playing a lecture"))?;
            self.cookie = Some(reply.body().deserialize()?);
            self.bus = Some(bus);
            Ok(())
        }

        fn release(&mut self) -> zbus::Result<()> {
            match (&self.bus, self.cookie.take()) {
                (Some(bus), Some(cookie)) => bus.call_method(Some(SERVICE), PATH, Some(SERVICE), "UnInhibit", &(cookie,)).map(drop),
                _ => Ok(()),
            }
        }
    }
}
