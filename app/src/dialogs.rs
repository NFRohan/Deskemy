// File and folder dialogs (rfd). rfd returns `None` both when the user
// cancels and when the dialog never opened at all (#10: on Linux, no session
// bus for the desktop portal and no zenity to fall back on), so a broken
// dialog looked like a button that did nothing. `pick` checks first whether a
// dialog can open here, and logs every empty return.

/// What came back from a dialog.
pub enum Picked<T> {
    Chosen(T),
    /// Closed without a choice: nothing to say to the user.
    Cancelled,
    /// The dialog can't open here; the message says why, for the user.
    Unavailable(String),
}

/// Open a dialog (`show` runs rfd) unless it can't open here. `what` names it
/// in the log.
pub fn pick<T>(what: &str, show: impl FnOnce() -> Option<T>) -> Picked<T> {
    if let Some(why) = unavailable() {
        tracing::warn!(dialog = what, why, "file dialog can't open");
        return Picked::Unavailable(why);
    }
    match show() {
        Some(chosen) => Picked::Chosen(chosen),
        None => {
            // rfd logs its own failures (portal, zenity) just before this.
            tracing::debug!(dialog = what, "file dialog closed without a choice");
            Picked::Cancelled
        }
    }
}

/// Why no file dialog can open here, if we can tell. Linux: rfd asks the
/// desktop portal over the session bus, else runs zenity; with neither, its
/// dialogs return at once.
#[cfg(target_os = "linux")]
fn unavailable() -> Option<String> {
    let bus = session_bus_path(
        std::env::var("DBUS_SESSION_BUS_ADDRESS").ok().as_deref(),
        std::env::var("XDG_RUNTIME_DIR").ok().as_deref(),
    );
    let bus_reachable = match &bus {
        Some(BusPath::Socket(path)) => std::os::unix::net::UnixStream::connect(path).is_ok(),
        // Abstract sockets, TCP, …: assume reachable rather than nag.
        Some(BusPath::Other) => true,
        None => false,
    };
    if bus_reachable || on_path("zenity") {
        return None;
    }
    tracing::warn!(
        ?bus,
        "no session bus for the file chooser portal (DBUS_SESSION_BUS_ADDRESS / XDG_RUNTIME_DIR) and no zenity"
    );
    // Short: the sidebar shows two lines of it.
    Some("No file dialog here: it needs a desktop session (D-Bus) or zenity.".into())
}

#[cfg(not(target_os = "linux"))]
fn unavailable() -> Option<String> {
    None
}

/// Where the session bus listens, from DBUS_SESSION_BUS_ADDRESS
/// (`unix:path=/run/user/1000/bus[,guid=…][;other…]`), else the default
/// `$XDG_RUNTIME_DIR/bus`.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
#[derive(Debug, PartialEq)]
enum BusPath {
    Socket(std::path::PathBuf),
    Other,
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn session_bus_path(address: Option<&str>, runtime_dir: Option<&str>) -> Option<BusPath> {
    match address.map(str::trim).filter(|a| !a.is_empty()) {
        Some(address) => {
            // The first of ';'-separated addresses is tried first.
            let first = address.split(';').next().unwrap_or_default();
            let (transport, params) = first.split_once(':')?;
            if transport != "unix" {
                return Some(BusPath::Other);
            }
            let path = params.split(',').find_map(|kv| kv.strip_prefix("path="));
            Some(path.map_or(BusPath::Other, |p| BusPath::Socket(p.into())))
        }
        None => runtime_dir
            .filter(|d| !d.is_empty())
            .map(|d| BusPath::Socket(std::path::Path::new(d).join("bus"))),
    }
}

#[cfg(target_os = "linux")]
fn on_path(program: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(program).is_file()))
}

#[cfg(test)]
mod tests {
    use super::{session_bus_path, BusPath};
    #[cfg(not(target_os = "linux"))]
    use super::{pick, Picked};

    #[test]
    fn the_bus_address_names_the_socket() {
        assert_eq!(
            session_bus_path(Some("unix:path=/run/user/1000/bus"), None),
            Some(BusPath::Socket("/run/user/1000/bus".into()))
        );
        assert_eq!(
            session_bus_path(Some("unix:path=/run/user/0/bus,guid=abc;tcp:host=x"), Some("/ignored")),
            Some(BusPath::Socket("/run/user/0/bus".into()))
        );
        assert_eq!(session_bus_path(Some("unix:abstract=/tmp/dbus-x"), None), Some(BusPath::Other));
        assert_eq!(session_bus_path(Some("tcp:host=localhost,port=1"), None), Some(BusPath::Other));
        assert_eq!(session_bus_path(Some("garbage"), None), None);
    }

    #[test]
    fn without_an_address_the_runtime_dir_has_it() {
        assert_eq!(session_bus_path(None, Some("/run/user/1000")), Some(BusPath::Socket("/run/user/1000/bus".into())));
        assert_eq!(session_bus_path(Some(""), Some("/run/user/1000")), Some(BusPath::Socket("/run/user/1000/bus".into())));
        assert_eq!(session_bus_path(None, None), None);
    }

    // Elsewhere a dialog always counts as available (on Linux it depends on
    // the machine running the tests).
    #[cfg(not(target_os = "linux"))]
    #[test]
    fn a_choice_or_a_cancel_passes_through() {
        assert!(matches!(pick("test", || Some(7)), Picked::Chosen(7)));
        assert!(matches!(pick::<i32>("test", || None), Picked::Cancelled));
    }
}
