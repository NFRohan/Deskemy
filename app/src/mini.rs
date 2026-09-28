//! The follow-along mini player: the main window shrinks to a small,
//! always-on-top video beside an editor or terminal, and returns to where it
//! was. A mode of the one window rather than a second one: mpv renders into
//! this window's OpenGL context.

use crate::settings::{self, Config};
use crate::{AppWindow, Playback};
use deskemy_core::paths;
use slint::{ComponentHandle, PhysicalPosition, PhysicalSize};
use std::cell::Cell;
use std::rc::Rc;
use std::time::Duration;

/// A screen rectangle in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    fn from_array([x, y, w, h]: [i32; 4]) -> Self {
        Rect { x, y, w, h }
    }
}

/// Logical size of a first-time mini player (16:9), and its gap from the
/// screen's edges.
const MINI: (f32, f32) = (480.0, 270.0);
/// Its smallest (as app.slint's `min-width` / `min-height` in mini mode).
const MIN: (f32, f32) = (240.0, 135.0);
const MARGIN: f32 = 24.0;
/// The window's size to come back to when it was maximized or fullscreen
/// before (so un-maximizing later doesn't land on the mini size).
const NORMAL: (f32, f32) = (1280.0, 800.0);

/// First-time placement: bottom-right of the work area (above the taskbar).
pub fn default_bounds(work: Rect, scale: f32) -> Rect {
    let (w, h) = ((MINI.0 * scale).round() as i32, (MINI.1 * scale).round() as i32);
    let margin = (MARGIN * scale).round() as i32;
    Rect { x: work.x + work.w - w - margin, y: work.y + work.h - h - margin, w, h }
}

/// A normal-sized window centred in the work area (never larger than it).
pub fn centred(work: Rect, scale: f32) -> Rect {
    let w = ((NORMAL.0 * scale).round() as i32).min(work.w);
    let h = ((NORMAL.1 * scale).round() as i32).min(work.h);
    Rect { x: work.x + (work.w - w) / 2, y: work.y + (work.h - h) / 2, w, h }
}

/// What to go back to on leaving the mini player.
#[derive(Clone, Copy, Debug)]
struct Saved {
    normal: Rect,
    maximized: bool,
    fullscreen: bool,
}

pub struct MiniPlayer {
    config: Config,
    saved: Cell<Option<Saved>>,
    /// Logs the window's real rectangle for a moment after a switch (see
    /// `watch`), to see what the transition actually does.
    watch: Rc<slint::Timer>,
}

impl MiniPlayer {
    pub fn new(ui: &AppWindow, config: Config) -> Rc<Self> {
        let mini = Rc::new(MiniPlayer { config, saved: Cell::new(None), watch: Rc::new(slint::Timer::default()) });
        let (m, weak) = (mini.clone(), ui.as_weak());
        ui.global::<Playback>().on_toggle_mini(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.global::<Playback>().get_mini() {
                m.exit(&ui);
            } else {
                m.enter(&ui);
            }
        });
        mini
    }

    fn enter(&self, ui: &AppWindow) {
        let window = ui.window();
        let playback = ui.global::<Playback>();
        let scale = window.scale_factor();
        let work = work_area(ui);
        let (maximized, fullscreen) = (window.is_maximized(), window.is_fullscreen());
        // Maximized or fullscreen, the window's own rect is the screen's: come
        // back to a normal size instead of that.
        let normal = if maximized || fullscreen {
            work.map(|w| centred(w, scale)).unwrap_or(current(ui))
        } else {
            current(ui)
        };
        self.saved.set(Some(Saved { normal, maximized, fullscreen }));
        trace(ui, "enter: before");

        if fullscreen {
            window.set_fullscreen(false);
            playback.set_fullscreen(false);
            trace(ui, "enter: left fullscreen");
        }
        if maximized {
            window.set_maximized(false);
            trace(ui, "enter: un-maximized");
        }
        playback.set_open_menu("".into());
        // Lifts the minimum size and puts the window on top.
        playback.set_mini(true);
        // Slint applies that minimum on its next update; the window has to be
        // allowed to shrink now, so it moves straight to its spot. (Placing it
        // later showed the restored pre-fullscreen window in between.)
        set_min_size(ui, MIN);

        let remembered = settings::lock(&self.config).mini_player.map(Rect::from_array).filter(|r| on_screen(*r));
        let target = remembered.or_else(|| work.map(|w| default_bounds(w, scale)));
        tracing::debug!(?target, ?work, remembered = remembered.is_some(), "mini: enter target");
        if let Some(r) = target {
            place(ui, r);
        }
        trace(ui, "enter: placed");
        self.watch(ui, "enter");
    }

    fn exit(&self, ui: &AppWindow) {
        self.remember(ui);
        ui.global::<Playback>().set_mini(false);
        let Some(saved) = self.saved.take() else { return };
        trace(ui, "exit: before");
        tracing::debug!(?saved, "mini: exit target");
        // The normal rect first, even on the way to maximized / fullscreen:
        // it's what the window returns to when those end.
        place(ui, saved.normal);
        trace(ui, "exit: placed");
        if saved.fullscreen {
            ui.window().set_fullscreen(true);
            ui.global::<Playback>().set_fullscreen(true);
            trace(ui, "exit: fullscreen");
        } else if saved.maximized {
            ui.window().set_maximized(true);
            trace(ui, "exit: maximized");
        }
        self.watch(ui, "exit");
    }

    /// For 600ms after a switch, log every change in the window's real
    /// rectangle with the time since the switch: where it went, and for how
    /// long, frame by frame.
    fn watch(&self, ui: &AppWindow, what: &'static str) {
        let (weak, start) = (ui.as_weak(), std::time::Instant::now());
        let timer = Rc::downgrade(&self.watch);
        let last = Cell::new(None);
        self.watch.start(slint::TimerMode::Repeated, Duration::from_millis(1), move || {
            if start.elapsed() > Duration::from_millis(600) {
                if let Some(timer) = timer.upgrade() {
                    timer.stop();
                }
                return;
            }
            let Some(ui) = weak.upgrade() else { return };
            let now = (os_rect(&ui), current(&ui));
            if last.get() != Some(now) {
                last.set(Some(now));
                let ms = start.elapsed().as_secs_f64() * 1000.0;
                tracing::debug!(what, ms = %format!("{ms:.1}"), os = ?now.0, slint = ?now.1, "mini: window moved");
            }
        });
    }

    /// Keep where the mini player is now for next time (on leaving it, and on
    /// quitting while in it).
    pub fn remember(&self, ui: &AppWindow) {
        if !ui.global::<Playback>().get_mini() {
            return;
        }
        let r = current(ui);
        let mut config = settings::lock(&self.config);
        config.mini_player = Some([r.x, r.y, r.w, r.h]);
        if let Some(dir) = paths::data_dir() {
            if let Err(e) = config.save(&dir.join(paths::CONFIG_FILE)) {
                tracing::warn!(error = %e, "save mini player position");
            }
        }
    }
}

/// One line on the window's state: the rectangle Windows reports (and, when
/// maximized, the one it restores to), and what Slint thinks.
fn trace(ui: &AppWindow, step: &str) {
    let window = ui.window();
    tracing::debug!(
        step,
        os = ?os_rect(ui),
        restore = ?restore_rect(ui),
        slint = ?current(ui),
        maximized = window.is_maximized(),
        fullscreen = window.is_fullscreen(),
        "mini: window"
    );
}

/// The window's rectangle as Windows has it right now.
#[cfg(windows)]
fn os_rect(ui: &AppWindow) -> Option<Rect> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::GetWindowRect;
    let hwnd = hwnd(ui)?;
    let mut r = RECT { left: 0, top: 0, right: 0, bottom: 0 };
    // SAFETY: a live window handle and a RECT to fill.
    (unsafe { GetWindowRect(hwnd, &mut r) } != 0)
        .then(|| Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
}

/// Where Windows returns the window when it stops being maximized.
#[cfg(windows)]
fn restore_rect(ui: &AppWindow) -> Option<Rect> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
    let hwnd = hwnd(ui)?;
    // SAFETY: a live window handle; WINDOWPLACEMENT is sized for the call.
    unsafe {
        let mut p: WINDOWPLACEMENT = std::mem::zeroed();
        p.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        (GetWindowPlacement(hwnd, &mut p) != 0).then(|| {
            let r = p.rcNormalPosition;
            Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
        })
    }
}

#[cfg(not(windows))]
fn os_rect(_: &AppWindow) -> Option<Rect> {
    None
}

#[cfg(not(windows))]
fn restore_rect(_: &AppWindow) -> Option<Rect> {
    None
}

fn current(ui: &AppWindow) -> Rect {
    let window = ui.window();
    let (p, s) = (window.position(), window.size());
    Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
}

/// Set the window's minimum size right away (Slint's own follows).
fn set_min_size(ui: &AppWindow, (w, h): (f32, f32)) {
    use slint::winit_030::{winit::dpi::LogicalSize, WinitWindowAccessor};
    ui.window().with_winit_window(|window| window.set_min_inner_size(Some(LogicalSize::new(w, h))));
}

fn place(ui: &AppWindow, r: Rect) {
    let window = ui.window();
    window.set_size(PhysicalSize::new(r.w.max(1) as u32, r.h.max(1) as u32));
    window.set_position(PhysicalPosition::new(r.x, r.y));
}

#[cfg(windows)]
fn hwnd(ui: &AppWindow) -> Option<windows_sys::Win32::Foundation::HWND> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match ui.window().window_handle().window_handle().map(|h| h.as_raw()) {
        Ok(RawWindowHandle::Win32(h)) => Some(h.hwnd.get() as _),
        _ => None,
    }
}

/// The usable part of the window's monitor (without the taskbar).
#[cfg(windows)]
fn work_area(ui: &AppWindow) -> Option<Rect> {
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let hwnd = hwnd(ui)?;
    // SAFETY: a live window handle; MONITORINFO is sized for the call.
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(monitor, &mut info) != 0).then(|| {
            let r = info.rcWork;
            Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
        })
    }
}

/// Elsewhere: the monitor's full area (winit knows no work area).
#[cfg(not(windows))]
fn work_area(ui: &AppWindow) -> Option<Rect> {
    use slint::winit_030::WinitWindowAccessor;
    ui.window()
        .with_winit_window(|w| {
            w.current_monitor().map(|m| {
                let (p, s) = (m.position(), m.size());
                Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
            })
        })
        .flatten()
}

/// Whether a remembered spot is still on a screen (a monitor may be gone).
#[cfg(windows)]
fn on_screen(r: Rect) -> bool {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONULL};
    let rect = RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h };
    // SAFETY: plain value in, a handle (or null) out.
    !unsafe { MonitorFromRect(&rect, MONITOR_DEFAULTTONULL) }.is_null()
}

#[cfg(not(windows))]
fn on_screen(_: Rect) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect { x: 0, y: 0, w: 1920, h: 1032 };

    #[test]
    fn opens_bottom_right_above_the_taskbar() {
        assert_eq!(default_bounds(WORK, 1.0), Rect { x: 1416, y: 738, w: 480, h: 270 });
        // At 150 %: physical pixels, same logical gap.
        assert_eq!(default_bounds(WORK, 1.5), Rect { x: 1920 - 720 - 36, y: 1032 - 405 - 36, w: 720, h: 405 });
        // A second monitor to the left.
        let left = Rect { x: -1280, y: 0, w: 1280, h: 984 };
        assert_eq!(default_bounds(left, 1.0).x, -1280 + 1280 - 480 - 24);
    }

    #[test]
    fn comes_back_centred_and_never_larger_than_the_screen() {
        assert_eq!(centred(WORK, 1.0), Rect { x: 320, y: 116, w: 1280, h: 800 });
        let small = Rect { x: 0, y: 0, w: 1366, h: 728 };
        assert_eq!(centred(small, 1.25), Rect { x: 0, y: 0, w: 1366, h: 728 });
    }
}
