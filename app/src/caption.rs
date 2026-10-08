// The custom title bar's maximize button, as Windows sees it: Windows 11
// shows Snap Layouts when the pointer rests on a window's maximize button,
// which it only knows of from the hit test (HTMAXBUTTON). Over that spot
// the button's mouse input comes to us as non-client messages, so its hover,
// press and click are handled here and passed to the Slint button.
// Windows only; elsewhere the Slint button works as it is.
#![cfg_attr(not(windows), allow(dead_code))]

use crate::{AppWindow, WindowChrome};
use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};

/// Caption button size (logical px), as `CaptionButton` in shell.slint.
const BUTTON: (f64, f64) = (46.0, 32.0);
/// The window's resize border (logical px), as `resize-border-width` in
/// app.slint: the top edge above the button stays a resize handle.
const RESIZE_BORDER: f64 = 6.0;

/// The caption buttons are in sight and not under a dialog.
static SHOWN: AtomicBool = AtomicBool::new(true);
/// The pointer is over the maximize button / pressed on it.
static HOVER: AtomicBool = AtomicBool::new(false);
static PRESSED: AtomicBool = AtomicBool::new(false);

/// Install the hook once the window exists, and follow the buttons' state.
pub fn wire(ui: &AppWindow) {
    ui.on_caption_buttons_changed(|shown| {
        SHOWN.store(shown, Ordering::Relaxed);
    });
    #[cfg(windows)]
    {
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::ZERO, move || {
            let Some(ui) = weak.upgrade() else { return };
            let hooked = hook(&ui);
            tracing::debug!(hooked, "window: maximize button hook");
        });
    }
}

/// Show the button's hover and press on the Slint side, after the message
/// that changed them (never from inside the window procedure).
fn show_state(ui: slint::Weak<AppWindow>) {
    let (hover, pressed) = (HOVER.load(Ordering::Relaxed), PRESSED.load(Ordering::Relaxed));
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui.upgrade() {
            let chrome = ui.global::<WindowChrome>();
            chrome.set_max_hover(hover);
            chrome.set_max_pressed(pressed);
        }
    });
}

/// Is a point (client pixels, at `scale`) on the maximize button? It's the
/// middle of the three buttons in the top-right corner.
fn on_maximize(x: f64, y: f64, width: f64, scale: f64, maximized: bool) -> bool {
    let (w, h) = (BUTTON.0 * scale, BUTTON.1 * scale);
    let top = if maximized { 0.0 } else { RESIZE_BORDER * scale };
    let right = width - w;
    x >= right - w && x < right && y >= top && y < h
}

#[cfg(windows)]
thread_local! {
    static UI: std::cell::RefCell<Option<slint::Weak<AppWindow>>> = const { std::cell::RefCell::new(None) };
}

#[cfg(windows)]
fn hook(ui: &AppWindow) -> bool {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let Some(hwnd) = crate::mini::hwnd(ui) else { return false };
    UI.with(|u| *u.borrow_mut() = Some(ui.as_weak()));
    // SAFETY: a live window handle, on its own thread; the procedure is
    // 'static and forwards everything it doesn't handle.
    unsafe { SetWindowSubclass(hwnd, Some(on_message), 0x63617074, 0) != 0 }
}

#[cfg(windows)]
unsafe extern "system" fn on_message(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::Foundation::{POINT, RECT};
    use windows_sys::Win32::Graphics::Gdi::ScreenToClient;
    use windows_sys::Win32::UI::HiDpi::GetDpiForWindow;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT};
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetClientRect, IsZoomed, HTMAXBUTTON, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN, WM_NCLBUTTONUP,
        WM_NCMOUSELEAVE, WM_NCMOUSEMOVE,
    };
    let ui = || UI.with(|u| u.borrow().clone());
    let on_button = wparam == HTMAXBUTTON as usize;
    match msg {
        WM_NCHITTEST if SHOWN.load(Ordering::Relaxed) => {
            // SAFETY: forwarding the message as received.
            let hit = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
            let mut p = POINT { x: (lparam & 0xffff) as i16 as i32, y: ((lparam >> 16) & 0xffff) as i16 as i32 };
            let mut client = RECT { left: 0, top: 0, right: 0, bottom: 0 };
            // SAFETY: a live window handle; a POINT and a RECT to fill.
            let (ok, dpi, maximized) = unsafe {
                (
                    ScreenToClient(hwnd, &mut p) != 0 && GetClientRect(hwnd, &mut client) != 0,
                    GetDpiForWindow(hwnd),
                    IsZoomed(hwnd) != 0,
                )
            };
            let scale = if dpi > 0 { dpi as f64 / 96.0 } else { 1.0 };
            if ok && on_maximize(p.x as f64, p.y as f64, client.right as f64, scale, maximized) {
                return HTMAXBUTTON as isize;
            }
            hit
        }
        WM_NCMOUSEMOVE => {
            if on_button != HOVER.load(Ordering::Relaxed) {
                HOVER.store(on_button, Ordering::Relaxed);
                if !on_button {
                    PRESSED.store(false, Ordering::Relaxed);
                }
                if on_button {
                    // Hear when the pointer leaves it for the client area or
                    // the desktop.
                    let mut track = TRACKMOUSEEVENT {
                        cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                        dwFlags: TME_LEAVE | TME_NONCLIENT,
                        hwndTrack: hwnd,
                        dwHoverTime: 0,
                    };
                    // SAFETY: a sized TRACKMOUSEEVENT for a live window.
                    unsafe { TrackMouseEvent(&mut track) };
                }
                if let Some(ui) = ui() {
                    show_state(ui);
                }
            }
            // SAFETY: forwarding the message as received.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        WM_NCMOUSELEAVE => {
            if HOVER.swap(false, Ordering::Relaxed) | PRESSED.swap(false, Ordering::Relaxed) {
                if let Some(ui) = ui() {
                    show_state(ui);
                }
            }
            // SAFETY: forwarding the message as received.
            unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
        }
        // Down / up / double-click on the button are ours: Windows' default
        // would run its own button loop (and draw a classic button).
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK if on_button => {
            PRESSED.store(true, Ordering::Relaxed);
            if let Some(ui) = ui() {
                show_state(ui);
            }
            0
        }
        WM_NCLBUTTONUP if on_button => {
            if PRESSED.swap(false, Ordering::Relaxed) {
                if let Some(ui) = ui() {
                    show_state(ui.clone());
                    let _ = slint::invoke_from_event_loop(move || {
                        if let Some(ui) = ui.upgrade() {
                            ui.global::<WindowChrome>().invoke_toggle_maximize();
                        }
                    });
                }
            }
            0
        }
        // SAFETY: forwarding the message as received.
        _ => unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) },
    }
}

#[cfg(test)]
mod tests {
    use super::on_maximize;

    #[test]
    fn maximize_is_the_middle_button() {
        // 1000px wide at 100%: minimize 862..908, maximize 908..954, close 954..1000.
        assert!(on_maximize(908.0, 10.0, 1000.0, 1.0, false));
        assert!(on_maximize(953.0, 31.0, 1000.0, 1.0, false));
        assert!(!on_maximize(907.0, 10.0, 1000.0, 1.0, false));
        assert!(!on_maximize(954.0, 10.0, 1000.0, 1.0, false));
        assert!(!on_maximize(920.0, 32.0, 1000.0, 1.0, false));
        // At 150%: 69px buttons.
        assert!(on_maximize(1500.0 - 138.0, 20.0, 1500.0, 1.5, false));
        assert!(!on_maximize(1500.0 - 139.0, 20.0, 1500.0, 1.5, false));
    }

    #[test]
    fn top_edge_stays_a_resize_handle() {
        assert!(!on_maximize(920.0, 5.0, 1000.0, 1.0, false));
        assert!(on_maximize(920.0, 6.0, 1000.0, 1.0, false));
        // Maximized windows don't resize: the button reaches the top.
        assert!(on_maximize(920.0, 0.0, 1000.0, 1.0, true));
    }
}
