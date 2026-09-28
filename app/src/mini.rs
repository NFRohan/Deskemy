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
}

impl MiniPlayer {
    pub fn new(ui: &AppWindow, config: Config) -> Rc<Self> {
        let mini = Rc::new(MiniPlayer { config, saved: Cell::new(None) });
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

        if fullscreen {
            window.set_fullscreen(false);
            playback.set_fullscreen(false);
        }
        if maximized {
            window.set_maximized(false);
        }
        playback.set_open_menu("".into());
        // Lifts the minimum size and puts the window on top.
        playback.set_mini(true);

        let remembered = settings::lock(&self.config).mini_player.map(Rect::from_array).filter(|r| on_screen(*r));
        let target = remembered.or_else(|| work.map(|w| default_bounds(w, scale)));
        // After the window has left maximized / fullscreen, or that would undo it.
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_millis(80), move || {
            if let (Some(ui), Some(r)) = (weak.upgrade(), target) {
                place(&ui, r);
            }
        });
        tracing::info!(remembered = remembered.is_some(), "mini player");
    }

    fn exit(&self, ui: &AppWindow) {
        self.remember(ui);
        ui.global::<Playback>().set_mini(false);
        let Some(saved) = self.saved.take() else { return };
        place(ui, saved.normal);
        if saved.maximized || saved.fullscreen {
            let weak = ui.as_weak();
            slint::Timer::single_shot(Duration::from_millis(80), move || {
                let Some(ui) = weak.upgrade() else { return };
                if saved.fullscreen {
                    ui.window().set_fullscreen(true);
                    ui.global::<Playback>().set_fullscreen(true);
                } else {
                    ui.window().set_maximized(true);
                }
            });
        }
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

fn current(ui: &AppWindow) -> Rect {
    let window = ui.window();
    let (p, s) = (window.position(), window.size());
    Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
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
