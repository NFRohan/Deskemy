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
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
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
/// How long a switch keeps the window out of sight: enough for the moves
/// (6–25ms in the logs) and a frame drawn at the new size.
const REVEAL_MS: u64 = 60;
/// How long leaving mini waits for Slint to see the restored size.
const SETTLE_MS: u64 = 30;
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

/// The playing video's display aspect (width / height) as f32 bits, 0 when
/// unknown; the event thread keeps it current.
static VIDEO_ASPECT: AtomicU32 = AtomicU32::new(0);
/// The aspect the window keeps while it's resized (mini mode), 0 for none.
static LOCKED_ASPECT: AtomicU32 = AtomicU32::new(0);
/// The window's minimum size in physical pixels (width << 32 | height), for
/// the resize hook.
static MIN_PHYSICAL: AtomicU64 = AtomicU64::new(0);

/// Called by the event thread with mpv's `video-params/aspect`. None (between
/// files, or audio) keeps the last one, so the mini player doesn't flip to
/// 16:9 and back at every lecture change.
pub fn set_video_aspect(aspect: Option<f64>) {
    if let Some(a) = aspect.filter(|a| (0.2..=5.0).contains(a)) {
        VIDEO_ASPECT.store((a as f32).to_bits(), Ordering::Relaxed);
    }
}

/// The mini player is on (its aspect lock is set): the window is the
/// video's shape, so the video can fill it.
pub fn active() -> bool {
    LOCKED_ASPECT.load(Ordering::Relaxed) != 0
}

/// The video's aspect, or 16:9 before one is known (or for audio).
fn video_aspect() -> f64 {
    match f32::from_bits(VIDEO_ASPECT.load(Ordering::Relaxed)) {
        a if a > 0.0 => a as f64,
        _ => 16.0 / 9.0,
    }
}

// WM_SIZING's edges (WMSZ_*).
const LEFT: u32 = 1;
const TOP: u32 = 3;
const TOP_LEFT: u32 = 4;
const TOP_RIGHT: u32 = 5;
const BOTTOM: u32 = 6;
const BOTTOM_LEFT: u32 = 7;

/// A rectangle being resized from `edge` (a WMSZ_* value), corrected to
/// `aspect` and to at least `min`: the dragged edge leads, the opposite one
/// stays put. Dragging the top or bottom sets the height, any other edge or
/// corner the width.
pub fn keep_aspect(r: Rect, edge: u32, aspect: f64, min: (i32, i32)) -> Rect {
    let (mut w, mut h) = if matches!(edge, TOP | BOTTOM) {
        ((r.h as f64 * aspect).round() as i32, r.h)
    } else {
        (r.w, (r.w as f64 / aspect).round() as i32)
    };
    if w < min.0 || h < min.1 {
        w = min.0.max((min.1 as f64 * aspect).ceil() as i32);
        h = ((w as f64 / aspect).round() as i32).max(min.1);
    }
    let x = if matches!(edge, LEFT | TOP_LEFT | BOTTOM_LEFT) { r.x + r.w - w } else { r.x };
    let y = if matches!(edge, TOP | TOP_LEFT | TOP_RIGHT) { r.y + r.h - h } else { r.y };
    Rect { x, y, w, h }
}

/// A mini player rectangle reshaped to `aspect`, keeping its width and the
/// edge nearest the screen's (bottom in the lower half), and kept on screen.
pub fn fit_aspect(r: Rect, aspect: f64, work: Option<Rect>, min: (i32, i32)) -> Rect {
    let mut out = keep_aspect(r, 2, aspect, min);
    let Some(work) = work else { return out };
    if r.y + r.h / 2 > work.y + work.h / 2 {
        out.y = r.y + r.h - out.h;
    }
    // Too big for the screen (a portrait video): shrink both, in shape.
    if out.h > work.h {
        out.h = work.h;
        out.w = (out.h as f64 * aspect).round() as i32;
    }
    if out.w > work.w {
        out.w = work.w;
        out.h = (out.w as f64 / aspect).round() as i32;
    }
    if r.y + r.h / 2 > work.y + work.h / 2 {
        out.y = r.y + r.h - out.h;
    }
    out.x = out.x.clamp(work.x, work.x + work.w - out.w);
    out.y = out.y.clamp(work.y, work.y + work.h - out.h);
    out
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
    /// Shows the window again after a switch (one timer for both ways, so a
    /// pending reveal can't fire in the middle of the next switch).
    reveal: Rc<slint::Timer>,
    /// Reshapes the mini player when the video's aspect changes (the next
    /// lecture).
    follow: slint::Timer,
    /// The resize hook is in (Windows).
    hooked: Cell<bool>,
    /// Finishes leaving mini once the window has settled (see `exit`).
    settle: slint::Timer,
    /// A switch is under way (until the window is shown again).
    switching: Rc<Cell<bool>>,
    /// Leaving mini goes back to fullscreen (if it came from there); the
    /// player closing mid-switch turns it off.
    to_fullscreen: Rc<Cell<bool>>,
}

impl MiniPlayer {
    pub fn new(ui: &AppWindow, config: Config) -> Rc<Self> {
        let mini = Rc::new(MiniPlayer {
            config,
            saved: Cell::new(None),
            reveal: Rc::new(slint::Timer::default()),
            follow: slint::Timer::default(),
            hooked: Cell::new(false),
            settle: slint::Timer::default(),
            switching: Rc::new(Cell::new(false)),
            to_fullscreen: Rc::new(Cell::new(false)),
        });
        let (m, weak) = (mini.clone(), ui.as_weak());
        ui.global::<Playback>().on_toggle_mini(move || {
            let Some(ui) = weak.upgrade() else { return };
            // Mid-switch (the window is settling out of sight): ignore.
            if m.switching.get() {
                return;
            }
            if ui.global::<Playback>().get_mini() {
                m.exit(&ui, true);
            } else {
                m.enter(&ui);
            }
        });
        // Closing the player from the mini one: back to the window, never to
        // fullscreen (even if a switch back there is already under way).
        let (m, weak) = (mini.clone(), ui.as_weak());
        ui.global::<Playback>().on_leave_mini(move || {
            if m.switching.get() {
                m.to_fullscreen.set(false);
            } else if let Some(ui) = weak.upgrade() {
                m.exit(&ui, false);
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
            work.map(|w| centred(w, scale)).unwrap_or(rect(ui))
        } else {
            rect(ui)
        };
        self.saved.set(Some(Saved { normal, maximized, fullscreen }));
        trace(ui, "enter: before");
        // Leaving fullscreen and un-maximizing each show the window somewhere
        // on the way (the restore rect) for a frame: switch out of sight.
        self.hide_while_switching(ui);

        if fullscreen {
            window.set_fullscreen(false);
            playback.set_fullscreen(false);
            trace(ui, "enter: left fullscreen");
        }
        if maximized {
            set_maximized(ui, false);
            trace(ui, "enter: un-maximized");
        }
        // Close any menu; the bookmark dialog through its own close, which
        // resumes the video it paused. The end-of-lecture resources card
        // stays: the mini player shows it as a chip, and it's there again
        // back in the full player.
        match playback.get_open_menu().as_str() {
            "bookmark" => playback.invoke_close_bookmark(),
            "exercise" => {}
            _ => playback.set_open_menu("".into()),
        }
        // Lifts the minimum size and puts the window on top.
        playback.set_mini(true);
        // Slint applies that minimum on its next update; the window has to be
        // allowed to shrink now, to move straight to its spot.
        set_min_size(ui, MIN);

        // The video's shape: no letterboxing, and resizing keeps it.
        let min = ((MIN.0 * scale).round() as i32, (MIN.1 * scale).round() as i32);
        let aspect = video_aspect();
        self.lock_aspect(ui, aspect, min);
        let remembered = settings::lock(&self.config).mini_player.map(Rect::from_array).filter(|r| on_screen(*r));
        // Fitted to the screen it's on: a spot on another monitor stays there.
        let target = match remembered {
            Some(r) => Some(fit_aspect(r, aspect, work_area_at(ui, r), min)),
            None => work.map(|w| fit_aspect(default_bounds(w, scale), aspect, Some(w), min)),
        };
        tracing::debug!(?target, ?work, aspect, remembered = remembered.is_some(), "mini: enter target");
        if let Some(r) = target {
            place(ui, r);
        }
        trace(ui, "enter: placed");
        self.follow_aspect(ui, min);
    }

    /// Back to the window as it was. `back_to_fullscreen`: false when the
    /// player is closing (the pages come back maximized or normal instead).
    fn exit(&self, ui: &AppWindow, back_to_fullscreen: bool) {
        self.remember(ui);
        self.follow.stop();
        LOCKED_ASPECT.store(0, Ordering::Relaxed);
        let Some(saved) = self.saved.take() else {
            ui.global::<Playback>().set_mini(false);
            return;
        };
        trace(ui, "exit: before");
        tracing::debug!(?saved, back_to_fullscreen, "mini: exit target");
        cloak(ui, true);
        self.reveal.stop();
        self.switching.set(true);
        self.to_fullscreen.set(back_to_fullscreen && saved.fullscreen);
        // The normal rect first, even on the way to maximized / fullscreen:
        // it's what the window returns to when those end.
        place(ui, saved.normal);
        trace(ui, "exit: placed");
        // Then leave mini mode once Slint has seen that size (the resize event
        // comes on its next pass): lifting mini raises the minimum to 900x600,
        // and against the old mini size Slint resized the window itself,
        // which un-maximized it again.
        let (weak, switching, to_fullscreen, reveal) =
            (ui.as_weak(), self.switching.clone(), self.to_fullscreen.clone(), self.reveal.clone());
        self.settle.start(slint::TimerMode::SingleShot, Duration::from_millis(SETTLE_MS), move || {
            let Some(ui) = weak.upgrade() else { return };
            let playback = ui.global::<Playback>();
            playback.set_mini(false);
            if saved.maximized {
                set_maximized(&ui, true);
                trace(&ui, "exit: maximized");
            }
            if to_fullscreen.get() {
                // In sight: the shell only puts the taskbar behind a window
                // it sees go fullscreen (done cloaked, the taskbar stayed on
                // top of the video until focus moved).
                cloak(&ui, false);
                ui.window().set_fullscreen(true);
                playback.set_fullscreen(true);
                switching.set(false);
                trace(&ui, "exit: fullscreen");
            } else {
                let (weak, switching) = (ui.as_weak(), switching.clone());
                reveal.start(slint::TimerMode::SingleShot, Duration::from_millis(REVEAL_MS), move || {
                    switching.set(false);
                    if let Some(ui) = weak.upgrade() {
                        cloak(&ui, false);
                        trace(&ui, "revealed");
                    }
                });
            }
        });
    }

    /// Keep the window at `aspect` while it's resized (the WM_SIZING hook).
    fn lock_aspect(&self, ui: &AppWindow, aspect: f64, min: (i32, i32)) {
        LOCKED_ASPECT.store((aspect as f32).to_bits(), Ordering::Relaxed);
        MIN_PHYSICAL.store(((min.0 as u64) << 32) | min.1 as u64, Ordering::Relaxed);
        if !self.hooked.get() {
            self.hooked.set(hook_resizing(ui));
        }
    }

    /// While mini: when the video's aspect changes (another lecture), take
    /// its shape.
    fn follow_aspect(&self, ui: &AppWindow, min: (i32, i32)) {
        let weak = ui.as_weak();
        self.follow.start(slint::TimerMode::Repeated, Duration::from_millis(500), move || {
            let Some(ui) = weak.upgrade() else { return };
            let aspect = video_aspect();
            let locked = f32::from_bits(LOCKED_ASPECT.load(Ordering::Relaxed)) as f64;
            if locked > 0.0 && (aspect / locked - 1.0).abs() > 0.01 {
                LOCKED_ASPECT.store((aspect as f32).to_bits(), Ordering::Relaxed);
                let r = rect(&ui);
                place(&ui, fit_aspect(r, aspect, work_area_at(&ui, r), min));
                tracing::debug!(aspect, "mini: video shape changed");
            }
        });
    }

    /// Take the window off screen (it keeps its place in the taskbar and
    /// its focus) until it has settled and Slint has drawn it at its new
    /// size, a few frames later.
    fn hide_while_switching(&self, ui: &AppWindow) {
        cloak(ui, true);
        self.switching.set(true);
        let (weak, switching) = (ui.as_weak(), self.switching.clone());
        self.reveal.start(slint::TimerMode::SingleShot, Duration::from_millis(REVEAL_MS), move || {
            switching.set(false);
            if let Some(ui) = weak.upgrade() {
                cloak(&ui, false);
                trace(&ui, "revealed");
            }
        });
    }

    /// Keep where the mini player is now for next time (on leaving it, and on
    /// quitting while in it).
    pub fn remember(&self, ui: &AppWindow) {
        if !ui.global::<Playback>().get_mini() {
            return;
        }
        let r = rect(ui);
        let mut config = settings::lock(&self.config);
        config.mini_player = Some([r.x, r.y, r.w, r.h]);
        if let Some(dir) = paths::data_dir() {
            if let Err(e) = config.save(&dir.join(paths::CONFIG_FILE)) {
                tracing::warn!(error = %e, "save mini player position");
            }
        }
    }
}

/// One debug line on the window's state at a step of a switch: the rectangle
/// Windows reports and what Slint thinks (they differ mid-switch).
fn trace(ui: &AppWindow, step: &str) {
    let window = ui.window();
    tracing::debug!(
        step,
        os = ?os_rect(ui),
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

#[cfg(not(windows))]
fn os_rect(_: &AppWindow) -> Option<Rect> {
    None
}

fn current(ui: &AppWindow) -> Rect {
    let window = ui.window();
    let (p, s) = (window.position(), window.size());
    Rect { x: p.x, y: p.y, w: s.width as i32, h: s.height as i32 }
}

/// Maximize or restore through winit, at once. Slint's `set_maximized` is
/// applied on its next update, and the resize event from placing the window
/// in between reset it: coming back from mini lost the maximized state.
fn set_maximized(ui: &AppWindow, on: bool) {
    use slint::winit_030::WinitWindowAccessor;
    ui.window().with_winit_window(|w| w.set_maximized(on));
}

/// Hook the window's WM_SIZING, where Windows lets a resize be corrected
/// before it's drawn (winit has no aspect ratio). Only acts in mini mode.
#[cfg(windows)]
fn hook_resizing(ui: &AppWindow) -> bool {
    use windows_sys::Win32::UI::Shell::SetWindowSubclass;
    let Some(hwnd) = hwnd(ui) else { return false };
    // SAFETY: a live window handle, on its own thread; the procedure is
    // 'static and forwards everything it doesn't handle.
    unsafe { SetWindowSubclass(hwnd, Some(on_sizing), 0x6d696e69, 0) != 0 }
}

#[cfg(not(windows))]
fn hook_resizing(_: &AppWindow) -> bool {
    false
}

#[cfg(windows)]
unsafe extern "system" fn on_sizing(
    hwnd: windows_sys::Win32::Foundation::HWND,
    msg: u32,
    wparam: windows_sys::Win32::Foundation::WPARAM,
    lparam: windows_sys::Win32::Foundation::LPARAM,
    _id: usize,
    _data: usize,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::Shell::DefSubclassProc;
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_SIZING;
    let aspect = f32::from_bits(LOCKED_ASPECT.load(Ordering::Relaxed)) as f64;
    if msg == WM_SIZING && aspect > 0.0 && lparam != 0 {
        let min = MIN_PHYSICAL.load(Ordering::Relaxed);
        let min = ((min >> 32) as i32, (min & 0xffff_ffff) as i32);
        // SAFETY: for WM_SIZING, lparam points at the window's proposed RECT.
        let rect = unsafe { &mut *(lparam as *mut RECT) };
        let r = Rect { x: rect.left, y: rect.top, w: rect.right - rect.left, h: rect.bottom - rect.top };
        let r = keep_aspect(r, wparam as u32, aspect, min);
        *rect = RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h };
        return 1;
    }
    // SAFETY: forwarding the message as received.
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}

/// Set the window's minimum size right away (Slint's own follows).
fn set_min_size(ui: &AppWindow, (w, h): (f32, f32)) {
    use slint::winit_030::{winit::dpi::LogicalSize, WinitWindowAccessor};
    ui.window().with_winit_window(|window| window.set_min_inner_size(Some(LogicalSize::new(w, h))));
}

/// The window's rectangle, outside edges: what `place` sets.
fn rect(ui: &AppWindow) -> Rect {
    os_rect(ui).unwrap_or_else(|| current(ui))
}

/// Move and resize the window to `r` (its outside edges). On Windows in one
/// SetWindowPos: after fullscreen, Slint's set_size gains an invisible frame
/// (480x270 became 496x309, and grew on every switch).
#[cfg(windows)]
fn place(ui: &AppWindow, r: Rect) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE, SWP_NOOWNERZORDER, SWP_NOZORDER};
    let flags = SWP_NOZORDER | SWP_NOOWNERZORDER | SWP_NOACTIVATE;
    // SAFETY: a live window handle; no z-order change.
    let placed = hwnd(ui)
        .is_some_and(|hwnd| unsafe { SetWindowPos(hwnd, std::ptr::null_mut(), r.x, r.y, r.w.max(1), r.h.max(1), flags) } != 0);
    if !placed {
        place_portably(ui, r);
    }
}

#[cfg(not(windows))]
fn place(ui: &AppWindow, r: Rect) {
    place_portably(ui, r);
}

fn place_portably(ui: &AppWindow, r: Rect) {
    let window = ui.window();
    window.set_position(PhysicalPosition::new(r.x, r.y));
    window.set_size(PhysicalSize::new(r.w.max(1) as u32, r.h.max(1) as u32));
}

/// Hide the window from the screen without hiding it from Windows (DWM
/// cloaking: no taskbar change, focus kept, no event-loop exit).
#[cfg(windows)]
fn cloak(ui: &AppWindow, on: bool) {
    use windows_sys::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_CLOAK};
    let Some(hwnd) = hwnd(ui) else { return };
    let value: i32 = on.into();
    // SAFETY: a live window handle; a BOOL-sized value for DWMWA_CLOAK.
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_CLOAK as u32, (&value as *const i32).cast(), std::mem::size_of::<i32>() as u32);
    }
}

/// Elsewhere the compositor animates or places windows itself.
#[cfg(not(windows))]
fn cloak(_: &AppWindow, _: bool) {}

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

/// The work area of the monitor `r` is (mostly) on.
#[cfg(windows)]
fn work_area_at(_: &AppWindow, r: Rect) -> Option<Rect> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromRect, MONITORINFO, MONITOR_DEFAULTTONEAREST};
    let rect = RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h };
    // SAFETY: plain values in; MONITORINFO is sized for the call.
    unsafe {
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONEAREST);
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        (GetMonitorInfoW(monitor, &mut info) != 0).then(|| {
            let w = info.rcWork;
            Rect { x: w.left, y: w.top, w: w.right - w.left, h: w.bottom - w.top }
        })
    }
}

#[cfg(not(windows))]
fn work_area_at(ui: &AppWindow, _: Rect) -> Option<Rect> {
    work_area(ui)
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
    fn resizing_keeps_the_shape_from_the_dragged_edge() {
        let r = Rect { x: 100, y: 100, w: 480, h: 270 };
        let wide = 16.0 / 9.0;
        // Right edge wider: the height follows, the top-left stays.
        assert_eq!(keep_aspect(Rect { w: 640, ..r }, 2, wide, (240, 135)), Rect { x: 100, y: 100, w: 640, h: 360 });
        // Top edge up: the width follows, the bottom stays.
        let taller = Rect { y: 10, h: 360, ..r };
        assert_eq!(keep_aspect(taller, TOP, wide, (240, 135)), Rect { x: 100, y: 10, w: 640, h: 360 });
        // Top-left corner: the width leads, the bottom-right corner stays.
        let dragged = Rect { x: 0, y: 50, w: 580, h: 320 };
        assert_eq!(keep_aspect(dragged, TOP_LEFT, wide, (240, 135)), Rect { x: 0, y: 44, w: 580, h: 326 });
        // Never below the minimum, still in shape (a 2.4:1 film).
        let tiny = Rect { w: 200, h: 90, ..r };
        let k = keep_aspect(tiny, 2, 2.4, (240, 135));
        assert!(k.w >= 240 && k.h >= 135 && ((k.w as f64 / k.h as f64) - 2.4).abs() < 0.02, "{k:?}");
    }

    #[test]
    fn fitting_keeps_the_width_and_the_near_edge() {
        let wide = 16.0 / 9.0;
        // A 4:3 remembered spot bottom-right becomes 16:9, still on the bottom edge.
        let r = Rect { x: 1416, y: 648, w: 480, h: 360 };
        assert_eq!(fit_aspect(r, wide, Some(WORK), (240, 135)), Rect { x: 1416, y: 738, w: 480, h: 270 });
        // Near the top, the top stays.
        let r = Rect { x: 24, y: 24, w: 480, h: 360 };
        assert_eq!(fit_aspect(r, wide, Some(WORK), (240, 135)), Rect { x: 24, y: 24, w: 480, h: 270 });
        // A portrait video too tall for the screen shrinks in shape, and
        // can't run off the bottom.
        let r = Rect { x: 1400, y: 700, w: 720, h: 405 };
        let f = fit_aspect(r, 9.0 / 16.0, Some(WORK), (240, 135));
        assert!(f.y + f.h <= WORK.h && f.h == WORK.h, "{f:?}");
        assert!(((f.w as f64 / f.h as f64) - 9.0 / 16.0).abs() < 0.01, "{f:?}");
    }

    #[test]
    fn comes_back_centred_and_never_larger_than_the_screen() {
        assert_eq!(centred(WORK, 1.0), Rect { x: 320, y: 116, w: 1280, h: 800 });
        let small = Rect { x: 0, y: 0, w: 1366, h: 728 };
        assert_eq!(centred(small, 1.25), Rect { x: 0, y: 0, w: 1366, h: 728 });
    }
}
