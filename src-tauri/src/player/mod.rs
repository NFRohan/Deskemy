//! Embedded mpv player. mpv renders into a native child window (`--wid`) sized
//! to the webview's video pane; we drive it via properties/commands and pump its
//! events on a background thread into a single `PlayerState` the UI subscribes to.

use crate::db::queries;
use crate::error::{DeskemyError, Result};
use crate::mpv::{
    Mpv, MpvEventEndFile, MPV_END_FILE_REASON_EOF, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED,
    MPV_EVENT_PROPERTY_CHANGE, MPV_EVENT_SHUTDOWN, MPV_FORMAT_DOUBLE, MPV_FORMAT_FLAG,
};
use crate::state::AppState;
use serde::Serialize;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// Single reactive snapshot the frontend subscribes to via the `player:state` event.
#[derive(Debug, Clone, Serialize)]
pub struct PlayerState {
    pub lecture_id: Option<String>,
    pub position: f64,
    pub duration: f64,
    pub paused: bool,
    pub speed: f64,
    pub eof: bool,
}

impl Default for PlayerState {
    fn default() -> Self {
        Self {
            lecture_id: None,
            position: 0.0,
            duration: 0.0,
            paused: true,
            speed: 1.0,
            eof: false,
        }
    }
}

#[derive(Clone)]
struct PlaylistItem {
    lecture_id: String,
    path: String,
}

#[derive(Default)]
struct Playlist {
    course_id: String,
    items: Vec<PlaylistItem>,
    index: usize,
}

/// Player operations, kept behind a trait so the mpv backend stays swappable.
pub trait PlayerService: Send + Sync {
    fn open(&self, lecture_id: &str) -> Result<()>;
    fn toggle_pause(&self) -> Result<()>;
    fn set_paused(&self, paused: bool) -> Result<()>;
    fn seek(&self, position: f64) -> Result<()>;
    fn set_speed(&self, speed: f64) -> Result<()>;
    fn next(&self) -> Result<()>;
    fn prev(&self) -> Result<()>;
    fn set_rect(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn state(&self) -> PlayerState;
}

pub struct MpvPlayer {
    inner: Arc<PlayerInner>,
}

struct PlayerInner {
    app: AppHandle,
    mpv: Mpv,
    child: isize, // native child HWND (as isize)
    state: Mutex<PlayerState>,
    playlist: Mutex<Playlist>,
    last_emit: Mutex<Instant>,
    last_save: Mutex<Instant>,
}

impl MpvPlayer {
    /// Create the embedded player: spawn the child window on the UI thread,
    /// initialize mpv into it, and start the event pump.
    pub fn new(app: AppHandle) -> Result<Self> {
        let main_hwnd = main_window_hwnd(&app)?;
        let child = create_child_on_main(&app, main_hwnd)?;

        let mpv = Mpv::new()?;
        mpv.set_option("wid", &child.to_string())?;
        mpv.set_option("hwdec", "auto-safe")?;
        mpv.set_option("keep-open", "no")?;
        mpv.set_option("osc", "no")?;
        mpv.set_option("osd-level", "0")?;
        mpv.set_option("config", "no")?;
        mpv.set_option("terminal", "no")?;
        mpv.set_option("input-default-bindings", "no")?;
        mpv.set_option("input-vo-keyboard", "no")?;
        mpv.set_option("force-window", "no")?;
        mpv.initialize()?;

        mpv.observe_property(1, "time-pos", MPV_FORMAT_DOUBLE)?;
        mpv.observe_property(2, "pause", MPV_FORMAT_FLAG)?;
        mpv.observe_property(3, "duration", MPV_FORMAT_DOUBLE)?;
        mpv.observe_property(4, "speed", MPV_FORMAT_DOUBLE)?;

        let now = Instant::now();
        let inner = Arc::new(PlayerInner {
            app,
            mpv,
            child,
            state: Mutex::new(PlayerState::default()),
            playlist: Mutex::new(Playlist::default()),
            last_emit: Mutex::new(now),
            last_save: Mutex::new(now),
        });

        spawn_pump(inner.clone());
        tracing::info!("mpv player initialized");
        Ok(MpvPlayer { inner })
    }
}

impl PlayerService for MpvPlayer {
    fn open(&self, lecture_id: &str) -> Result<()> {
        self.inner.open(lecture_id)
    }
    fn toggle_pause(&self) -> Result<()> {
        self.inner.mpv.command(&["cycle", "pause"])
    }
    fn set_paused(&self, paused: bool) -> Result<()> {
        self.inner
            .mpv
            .set_property("pause", if paused { "yes" } else { "no" })
    }
    fn seek(&self, position: f64) -> Result<()> {
        self.inner
            .mpv
            .command(&["seek", &position.to_string(), "absolute"])
    }
    fn set_speed(&self, speed: f64) -> Result<()> {
        self.inner.mpv.set_property("speed", &speed.to_string())
    }
    fn next(&self) -> Result<()> {
        self.inner.step(1)
    }
    fn prev(&self) -> Result<()> {
        self.inner.step(-1)
    }
    fn set_rect(&self, x: i32, y: i32, w: i32, h: i32) -> Result<()> {
        self.inner.set_rect(x, y, w, h);
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.inner.save_progress(false);
        self.inner.mpv.set_property("pause", "yes").ok();
        self.inner.show(false);
        Ok(())
    }
    fn state(&self) -> PlayerState {
        self.inner.state.lock().unwrap().clone()
    }
}

impl PlayerInner {
    fn open(&self, lecture_id: &str) -> Result<()> {
        let (course_id, items) = {
            let st = self.app.state::<AppState>();
            let db = st
                .db
                .lock()
                .map_err(|_| DeskemyError::Other("db lock poisoned".into()))?;
            let (_, course_id, _) = queries::get_lecture_playback(&db, lecture_id)?
                .ok_or_else(|| DeskemyError::NotFound(format!("lecture {lecture_id}")))?;
            let items = queries::list_course_playlist(&db, &course_id)?;
            (course_id, items)
        };

        let index = items
            .iter()
            .position(|(id, _)| id == lecture_id)
            .unwrap_or(0);

        {
            let mut pl = self.playlist.lock().unwrap();
            pl.course_id = course_id;
            pl.items = items
                .into_iter()
                .map(|(id, path)| PlaylistItem {
                    lecture_id: id,
                    path,
                })
                .collect();
            pl.index = index;
        }

        self.load_current(true)
    }

    fn step(&self, delta: i32) -> Result<()> {
        let next = {
            let pl = self.playlist.lock().unwrap();
            let n = pl.index as i32 + delta;
            if n < 0 || n as usize >= pl.items.len() {
                return Ok(());
            }
            n as usize
        };
        self.playlist.lock().unwrap().index = next;
        self.load_current(false)
    }

    fn load_current(&self, resume: bool) -> Result<()> {
        let (lecture_id, path, course_id) = {
            let pl = self.playlist.lock().unwrap();
            let item = pl
                .items
                .get(pl.index)
                .ok_or_else(|| DeskemyError::Other("empty playlist".into()))?;
            (item.lecture_id.clone(), item.path.clone(), pl.course_id.clone())
        };

        let (saved_pos, completed) = {
            let st = self.app.state::<AppState>();
            let db = st.db.lock().unwrap();
            queries::get_progress(&db, &lecture_id).unwrap_or((0.0, false))
        };
        let start = if resume && !completed { saved_pos } else { 0.0 };
        let speed = {
            let st = self.app.state::<AppState>();
            let cfg = st.config.lock().unwrap();
            cfg.default_speed
        };

        if start > 1.0 {
            self.mpv
                .command(&["loadfile", &path, "replace", &format!("start={start}")])?;
        } else {
            self.mpv.command(&["loadfile", &path, "replace"])?;
        }
        self.mpv.set_property("pause", "no").ok();
        self.mpv.set_property("speed", &speed.to_string()).ok();
        self.show(true);

        {
            let mut s = self.state.lock().unwrap();
            s.lecture_id = Some(lecture_id.clone());
            s.position = start;
            s.paused = false;
            s.speed = speed;
            s.eof = false;
        }

        {
            let st = self.app.state::<AppState>();
            let db = st.db.lock().unwrap();
            let _ = queries::set_last_lecture(&db, &course_id, &lecture_id);
        }

        self.emit();
        Ok(())
    }

    /// Refresh the observable state from mpv and emit (throttled).
    fn tick(&self, force: bool) {
        {
            let mut s = self.state.lock().unwrap();
            if let Some(p) = self.mpv.get_f64("time-pos") {
                s.position = p;
            }
            if let Some(d) = self.mpv.get_f64("duration") {
                s.duration = d;
            }
            if let Some(sp) = self.mpv.get_f64("speed") {
                s.speed = sp;
            }
            s.paused = self
                .mpv
                .get_property_string("pause")
                .map(|v| v == "yes")
                .unwrap_or(s.paused);
        }

        let should_emit = force || {
            let mut last = self.last_emit.lock().unwrap();
            if last.elapsed() >= Duration::from_millis(200) {
                *last = Instant::now();
                true
            } else {
                false
            }
        };
        if should_emit {
            self.emit();
        }

        // Periodically persist progress.
        let do_save = {
            let mut last = self.last_save.lock().unwrap();
            if last.elapsed() >= Duration::from_secs(5) {
                *last = Instant::now();
                true
            } else {
                false
            }
        };
        if do_save {
            self.save_progress(false);
        }
    }

    fn on_ended(&self) {
        self.save_progress(true);

        let autoplay = {
            let st = self.app.state::<AppState>();
            let a = st.config.lock().unwrap().autoplay_next;
            a
        };
        let has_next = {
            let pl = self.playlist.lock().unwrap();
            pl.index + 1 < pl.items.len()
        };

        if autoplay && has_next {
            self.playlist.lock().unwrap().index += 1;
            let _ = self.load_current(false);
            let _ = self.app.emit("player:advanced", ());
        } else {
            {
                let mut s = self.state.lock().unwrap();
                s.eof = true;
                s.paused = true;
            }
            self.emit();
        }
    }

    fn save_progress(&self, completed: bool) {
        let (lecture_id, position, duration) = {
            let s = self.state.lock().unwrap();
            (s.lecture_id.clone(), s.position, s.duration)
        };
        let Some(lecture_id) = lecture_id else {
            return;
        };
        let done = completed || (duration > 0.0 && position / duration >= 0.9);
        let st = self.app.state::<AppState>();
        let db = match st.db.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let _ = queries::save_progress(&db, &lecture_id, position, done);
    }

    fn emit(&self) {
        let snapshot = self.state.lock().unwrap().clone();
        let _ = self.app.emit("player:state", snapshot);
    }

    fn set_rect(&self, x: i32, y: i32, w: i32, h: i32) {
        let child = self.child;
        let _ = self
            .app
            .run_on_main_thread(move || move_child(child, x, y, w.max(1), h.max(1)));
    }

    fn show(&self, visible: bool) {
        let child = self.child;
        let _ = self
            .app
            .run_on_main_thread(move || show_child(child, visible));
    }
}

fn spawn_pump(inner: Arc<PlayerInner>) {
    std::thread::spawn(move || loop {
        let ev = inner.mpv.wait_event(1.0);
        if ev.is_null() {
            continue;
        }
        let id = unsafe { (*ev).event_id };
        match id {
            MPV_EVENT_SHUTDOWN => break,
            MPV_EVENT_FILE_LOADED => inner.tick(true),
            MPV_EVENT_PROPERTY_CHANGE => inner.tick(false),
            MPV_EVENT_END_FILE => {
                let data = unsafe { (*ev).data } as *const MpvEventEndFile;
                let reason = if data.is_null() {
                    -1
                } else {
                    unsafe { (*data).reason }
                };
                if reason == MPV_END_FILE_REASON_EOF {
                    inner.on_ended();
                }
            }
            _ => {}
        }
    });
}

// ---------------------------------------------------------------------------
// Native child window (Windows). Other platforms get inert stubs for now.
// ---------------------------------------------------------------------------

fn main_window_hwnd(app: &AppHandle) -> Result<isize> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| DeskemyError::Player("main window not found".into()))?;
    #[cfg(windows)]
    {
        let hwnd = window
            .hwnd()
            .map_err(|e| DeskemyError::Player(format!("hwnd: {e}")))?;
        Ok(hwnd.0 as isize)
    }
    #[cfg(not(windows))]
    {
        let _ = window;
        Ok(0)
    }
}

fn create_child_on_main(app: &AppHandle, parent: isize) -> Result<isize> {
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let _ = tx.send(create_child(parent));
    })
    .map_err(|e| DeskemyError::Player(format!("run_on_main_thread: {e}")))?;
    rx.recv()
        .map_err(|e| DeskemyError::Player(format!("child window channel: {e}")))
}

#[cfg(windows)]
fn create_child(parent: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, WS_CHILD, WS_CLIPSIBLINGS, WS_VISIBLE,
    };
    let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
    let name: Vec<u16> = "\0".encode_utf16().collect();
    unsafe {
        let h = CreateWindowExW(
            0,
            class.as_ptr(),
            name.as_ptr(),
            WS_CHILD | WS_VISIBLE | WS_CLIPSIBLINGS,
            0,
            0,
            1,
            1,
            parent as _,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
        );
        h as isize
    }
}

#[cfg(windows)]
fn move_child(hwnd: isize, x: i32, y: i32, w: i32, h: i32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, SWP_NOACTIVATE, SWP_NOZORDER,
    };
    unsafe {
        SetWindowPos(hwnd as _, std::ptr::null_mut(), x, y, w, h, SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

#[cfg(windows)]
fn show_child(hwnd: isize, visible: bool) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};
    unsafe {
        ShowWindow(hwnd as _, if visible { SW_SHOWNOACTIVATE } else { SW_HIDE });
    }
}

#[cfg(not(windows))]
fn create_child(_parent: isize) -> isize {
    0
}
#[cfg(not(windows))]
fn move_child(_hwnd: isize, _x: i32, _y: i32, _w: i32, _h: i32) {}
#[cfg(not(windows))]
fn show_child(_hwnd: isize, _visible: bool) {}
