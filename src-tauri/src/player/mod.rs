//! Embedded mpv player. mpv renders into a native child window (`--wid`) sized
//! to the webview's video pane; we drive it via properties/commands and pump its
//! events on a background thread into a single `PlayerState` the UI subscribes to.

use crate::db::queries;
use crate::error::{DeskemyError, Result};
use crate::mpv::{
    Mpv, MpvEventEndFile, MPV_END_FILE_REASON_EOF, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED,
    MPV_EVENT_SHUTDOWN, MPV_FORMAT_DOUBLE, MPV_FORMAT_FLAG,
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
    /// Active subtitle track id (None = off).
    pub sid: Option<i64>,
    /// Active audio track id.
    pub aid: Option<i64>,
    /// Current chapter index (-1 if none).
    pub chapter: i64,
    /// Volume 0..100.
    pub volume: f64,
    pub muted: bool,
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
            sid: None,
            aid: None,
            chapter: -1,
            volume: 100.0,
            muted: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct TrackInfo {
    pub id: i64,
    pub kind: String, // "audio" | "sub"
    pub lang: Option<String>,
    pub title: Option<String>,
    pub codec: Option<String>,
    pub selected: bool,
    /// For external tracks (e.g. sidecar .srt): the file's base name.
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChapterInfo {
    pub index: i64,
    pub title: Option<String>,
    pub time: f64,
}

#[derive(Debug, Clone, Serialize, Default)]
pub struct MediaTracks {
    pub audio: Vec<TrackInfo>,
    pub subtitle: Vec<TrackInfo>,
    pub chapters: Vec<ChapterInfo>,
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
    /// Position the video pane in CSS pixels (converted to device pixels here).
    fn set_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<()>;
    fn stop(&self) -> Result<()>;
    fn state(&self) -> PlayerState;
    /// Audio/subtitle tracks + chapters for the current file.
    fn tracks(&self) -> MediaTracks;
    fn set_subtitle(&self, sid: Option<i64>) -> Result<()>;
    fn set_audio(&self, aid: Option<i64>) -> Result<()>;
    fn set_chapter(&self, index: i64) -> Result<()>;
    fn set_volume(&self, volume: f64) -> Result<()>;
    fn set_muted(&self, muted: bool) -> Result<()>;
    /// Write the current video frame (no OSD/subs) to `path`.
    fn screenshot(&self, path: &str) -> Result<()>;
}

pub struct MpvPlayer {
    inner: Arc<PlayerInner>,
}

struct PlayerInner {
    app: AppHandle,
    mpv: Mpv,
    child: isize, // native child HWND (as isize); 0 in compositor mode
    // When present, mpv renders into a DirectComposition visual instead of the
    // child window — the video shares the webview surface (no airspace lag).
    #[cfg(windows)]
    compositor: Option<crate::compositor::Compositor>,
    state: Mutex<PlayerState>,
    playlist: Mutex<Playlist>,
    last_emit: Mutex<Instant>,
    last_save: Mutex<Instant>,
    // Watch-time telemetry (daily_activity): real seconds played since the last
    // flush, the tick timestamp, and lectures counted complete this session.
    watch_accum: Mutex<f64>,
    last_watch: Mutex<Instant>,
    completed_session: Mutex<std::collections::HashSet<String>>,
}

impl MpvPlayer {
    /// Create the embedded player: spawn the child window on the UI thread,
    /// initialize mpv into it, and start the event pump.
    pub fn new(app: AppHandle) -> Result<Self> {
        let main_hwnd = main_window_hwnd(&app)?;

        // Default compositing path (Windows): mpv renders into a DirectComposition
        // visual instead of a child window, so DOM + video resize atomically. The
        // decision was made and cached at startup (probe-gated, wid fallback); we
        // just read it so the player matches what the UI was told.
        #[cfg(windows)]
        let want_compositor = crate::compositor::is_active();
        #[cfg(not(windows))]
        let want_compositor = false;

        // No child window in compositor mode; mpv uses the render API (vo=libmpv).
        let child = if want_compositor {
            0
        } else {
            create_child_on_main(&app, main_hwnd)?
        };

        let mpv = Mpv::new()?;
        if want_compositor {
            mpv.set_option("vo", "libmpv")?;
        } else {
            mpv.set_option("wid", &child.to_string())?;
        }
        mpv.set_option("hwdec", "no")?; // rule out hw decode issues in embedded window
        mpv.set_option("keep-open", "no")?;
        mpv.set_option("osc", "no")?;
        mpv.set_option("osd-level", "0")?;
        mpv.set_option("config", "no")?;
        mpv.set_option("terminal", "no")?;
        mpv.set_option("input-default-bindings", "no")?;
        mpv.set_option("input-vo-keyboard", "no")?;
        mpv.set_option("force-window", "no")?;
        // Auto-load sidecar subtitle files (e.g. Udemy .srt) matching the video.
        mpv.set_option("sub-auto", "fuzzy")?;
        mpv.initialize()?;

        // The render context needs an initialized handle. If it can't be built,
        // the compositor path just shows nothing (opt-in flag; log and continue).
        #[cfg(windows)]
        let compositor = if want_compositor {
            match mpv.has_render_api() {
                true => match crate::mpv::MpvRenderContext::new_sw(&mpv) {
                    Ok(rctx) => {
                        tracing::info!("compositor: render context ready");
                        Some(crate::compositor::Compositor::new(
                            main_hwnd, rctx, 0, 0, 1280, 720,
                        ))
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "compositor render context failed");
                        None
                    }
                },
                false => {
                    tracing::warn!("compositor requested but libmpv lacks the render API");
                    None
                }
            }
        } else {
            None
        };

        mpv.observe_property(1, "time-pos", MPV_FORMAT_DOUBLE)?;
        mpv.observe_property(2, "pause", MPV_FORMAT_FLAG)?;
        mpv.observe_property(3, "duration", MPV_FORMAT_DOUBLE)?;
        mpv.observe_property(4, "speed", MPV_FORMAT_DOUBLE)?;

        let now = Instant::now();
        let inner = Arc::new(PlayerInner {
            app,
            mpv,
            child,
            #[cfg(windows)]
            compositor,
            state: Mutex::new(PlayerState::default()),
            playlist: Mutex::new(Playlist::default()),
            last_emit: Mutex::new(now),
            last_save: Mutex::new(now),
            watch_accum: Mutex::new(0.0),
            last_watch: Mutex::new(now),
            completed_session: Mutex::new(std::collections::HashSet::new()),
        });

        spawn_pump(inner.clone());
        tracing::info!("mpv player initialized");
        Ok(MpvPlayer { inner })
    }

    /// Native window-resize hook: retarget the compositor visual to the new
    /// window size (device px) without waiting for a JS rect report. No-op unless
    /// the compositing path is active.
    pub fn on_window_resize(&self, _w: i32, _h: i32) {
        #[cfg(windows)]
        if let Some(comp) = &self.inner.compositor {
            comp.resize(_w, _h);
        }
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
        let r = self.inner.mpv.set_property("speed", &speed.to_string());
        self.inner.remember(|db, cid| queries::set_pref_speed(db, cid, speed));
        r
    }
    fn next(&self) -> Result<()> {
        self.inner.step(1)
    }
    fn prev(&self) -> Result<()> {
        self.inner.step(-1)
    }
    fn set_rect(&self, x: f64, y: f64, w: f64, h: f64) -> Result<()> {
        self.inner.set_rect(x, y, w, h);
        Ok(())
    }
    fn stop(&self) -> Result<()> {
        self.inner.save_progress(false);
        self.inner.flush_watch();
        self.inner.mpv.set_property("pause", "yes").ok();
        self.inner.show(false);
        Ok(())
    }
    fn state(&self) -> PlayerState {
        self.inner.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
    fn tracks(&self) -> MediaTracks {
        self.inner.read_tracks()
    }
    fn set_subtitle(&self, sid: Option<i64>) -> Result<()> {
        let v = sid.map(|s| s.to_string()).unwrap_or_else(|| "no".into());
        let r = self.inner.mpv.set_property("sid", &v);
        self.inner.remember(|db, cid| queries::set_pref_subtitle(db, cid, sid));
        r
    }
    fn set_audio(&self, aid: Option<i64>) -> Result<()> {
        let v = aid.map(|s| s.to_string()).unwrap_or_else(|| "no".into());
        let r = self.inner.mpv.set_property("aid", &v);
        self.inner.remember(|db, cid| queries::set_pref_audio(db, cid, aid));
        r
    }
    fn set_chapter(&self, index: i64) -> Result<()> {
        self.inner.mpv.set_property("chapter", &index.to_string())
    }
    fn set_volume(&self, volume: f64) -> Result<()> {
        self.inner
            .mpv
            .set_property("volume", &volume.clamp(0.0, 100.0).to_string())
    }
    fn set_muted(&self, muted: bool) -> Result<()> {
        self.inner
            .mpv
            .set_property("mute", if muted { "yes" } else { "no" })
    }
    fn screenshot(&self, path: &str) -> Result<()> {
        // "video" = the decoded frame at source resolution, without OSD/subs.
        self.inner
            .mpv
            .command(&["screenshot-to-file", path, "video"])
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
            let mut pl = self.playlist.lock().unwrap_or_else(|e| e.into_inner());
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
            let pl = self.playlist.lock().unwrap_or_else(|e| e.into_inner());
            let n = pl.index as i32 + delta;
            if n < 0 || n as usize >= pl.items.len() {
                return Ok(());
            }
            n as usize
        };
        self.playlist.lock().unwrap_or_else(|e| e.into_inner()).index = next;
        self.load_current(false)
    }

    fn load_current(&self, resume: bool) -> Result<()> {
        let (lecture_id, path, course_id) = {
            let pl = self.playlist.lock().unwrap_or_else(|e| e.into_inner());
            let item = pl
                .items
                .get(pl.index)
                .ok_or_else(|| DeskemyError::Other("empty playlist".into()))?;
            (item.lecture_id.clone(), item.path.clone(), pl.course_id.clone())
        };

        let (saved_pos, completed) = {
            let st = self.app.state::<AppState>();
            let db = st.db.lock().unwrap_or_else(|e| e.into_inner());
            queries::get_progress(&db, &lecture_id).unwrap_or((0.0, false))
        };
        let start = if resume && !completed { saved_pos } else { 0.0 };
        // Per-course playback prefs override the global default where present.
        let prefs = {
            let st = self.app.state::<AppState>();
            let db = st.db.lock().unwrap_or_else(|e| e.into_inner());
            queries::get_course_prefs(&db, &course_id).ok().flatten()
        };
        let speed = prefs.as_ref().and_then(|p| p.0).unwrap_or_else(|| {
            let st = self.app.state::<AppState>();
            let cfg = st.config.lock().unwrap_or_else(|e| e.into_inner());
            cfg.default_speed
        });

        tracing::debug!(path = path.as_str(), start, "loadfile");
        // loadfile <url> [<flags> [<index> [<options>]]] — options is the 4th arg.
        if start > 1.0 {
            self.mpv.command(&[
                "loadfile",
                &path,
                "replace",
                "0",
                &format!("start={start}"),
            ])?;
        } else {
            self.mpv.command(&["loadfile", &path])?;
        }
        self.mpv.set_property("pause", "no").ok();
        self.mpv.set_property("speed", &speed.to_string()).ok();
        // Apply remembered audio/subtitle selection (mpv defers until tracks load).
        if let Some((_, sub_id, subs_on, aud_id)) = &prefs {
            if let Some(a) = aud_id {
                self.mpv.set_property("aid", &a.to_string()).ok();
            }
            if *subs_on {
                if let Some(s) = sub_id {
                    self.mpv.set_property("sid", &s.to_string()).ok();
                }
            } else {
                self.mpv.set_property("sid", "no").ok();
            }
        }
        self.show(true);

        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
            s.lecture_id = Some(lecture_id.clone());
            s.position = start;
            s.paused = false;
            s.speed = speed;
            s.eof = false;
        }

        {
            let st = self.app.state::<AppState>();
            let db = st.db.lock().unwrap_or_else(|e| e.into_inner());
            let _ = queries::set_last_lecture(&db, &course_id, &lecture_id);
        }

        self.emit();
        Ok(())
    }

    /// Refresh the observable state from mpv and emit (throttled).
    fn tick(&self, force: bool) {
        {
            let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
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
            s.sid = self.mpv.get_i64("sid");
            s.aid = self.mpv.get_i64("aid");
            s.chapter = self.mpv.get_i64("chapter").unwrap_or(-1);
            if let Some(v) = self.mpv.get_f64("volume") {
                s.volume = v;
            }
            s.muted = self
                .mpv
                .get_property_string("mute")
                .map(|v| v == "yes")
                .unwrap_or(s.muted);
        }

        // Accumulate real watch time while playing (capped per tick so a system
        // suspend or long stall doesn't inflate it).
        {
            let paused = self.state.lock().unwrap_or_else(|e| e.into_inner()).paused;
            let mut lw = self.last_watch.lock().unwrap_or_else(|e| e.into_inner());
            let delta = lw.elapsed().as_secs_f64();
            *lw = Instant::now();
            if !paused {
                *self.watch_accum.lock().unwrap_or_else(|e| e.into_inner()) += delta.min(2.0);
            }
        }

        let should_emit = force || {
            let mut last = self.last_emit.lock().unwrap_or_else(|e| e.into_inner());
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
            let mut last = self.last_save.lock().unwrap_or_else(|e| e.into_inner());
            if last.elapsed() >= Duration::from_secs(5) {
                *last = Instant::now();
                true
            } else {
                false
            }
        };
        if do_save {
            self.save_progress(false);
            self.flush_watch();
        }
    }

    /// Persist a per-course playback preference for the loaded course (best-effort).
    fn remember<F: FnOnce(&rusqlite::Connection, &str) -> Result<()>>(&self, f: F) {
        let cid = {
            let pl = self.playlist.lock().unwrap_or_else(|e| e.into_inner());
            if pl.items.is_empty() {
                return;
            }
            pl.course_id.clone()
        };
        let st = self.app.state::<AppState>();
        let db = match st.db.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let _ = f(&db, &cid);
    }

    /// Persist accumulated watch seconds to today's activity bucket.
    fn flush_watch(&self) {
        let secs = {
            let mut a = self.watch_accum.lock().unwrap_or_else(|e| e.into_inner());
            std::mem::replace(&mut *a, 0.0)
        };
        if secs <= 0.0 {
            return;
        }
        let st = self.app.state::<AppState>();
        let db = match st.db.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
        let _ = queries::add_watch_seconds(&db, secs);
    }

    fn on_ended(&self) {
        self.save_progress(true);

        let autoplay = {
            let st = self.app.state::<AppState>();
            let a = st.config.lock().unwrap_or_else(|e| e.into_inner()).autoplay_next;
            a
        };
        let has_next = {
            let pl = self.playlist.lock().unwrap_or_else(|e| e.into_inner());
            pl.index + 1 < pl.items.len()
        };

        if autoplay && has_next {
            self.playlist.lock().unwrap_or_else(|e| e.into_inner()).index += 1;
            let _ = self.load_current(false);
            let _ = self.app.emit("player:advanced", ());
        } else {
            {
                let mut s = self.state.lock().unwrap_or_else(|e| e.into_inner());
                s.eof = true;
                s.paused = true;
            }
            self.emit();
        }
    }

    fn save_progress(&self, completed: bool) {
        let (lecture_id, position, duration) = {
            let s = self.state.lock().unwrap_or_else(|e| e.into_inner());
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

        // Count each lecture's completion once per session for daily activity.
        if done && self.completed_session.lock().unwrap_or_else(|e| e.into_inner()).insert(lecture_id) {
            let _ = queries::add_completion(&db);
        }
    }

    fn emit(&self) {
        let snapshot = self.state.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let _ = self.app.emit("player:state", snapshot);
    }

    /// Read audio/subtitle tracks + chapters live from mpv.
    fn read_tracks(&self) -> MediaTracks {
        let mut audio = Vec::new();
        let mut subtitle = Vec::new();
        let n = self.mpv.get_i64("track-list/count").unwrap_or(0).max(0);
        for i in 0..n {
            let kind = self
                .mpv
                .get_property_string(&format!("track-list/{i}/type"))
                .unwrap_or_default();
            let track = TrackInfo {
                id: self.mpv.get_i64(&format!("track-list/{i}/id")).unwrap_or(0),
                kind: kind.clone(),
                lang: self.mpv.get_property_string(&format!("track-list/{i}/lang")),
                title: self.mpv.get_property_string(&format!("track-list/{i}/title")),
                codec: self.mpv.get_property_string(&format!("track-list/{i}/codec")),
                selected: self
                    .mpv
                    .get_property_string(&format!("track-list/{i}/selected"))
                    .map(|v| v == "yes")
                    .unwrap_or(false),
                filename: self
                    .mpv
                    .get_property_string(&format!("track-list/{i}/external-filename"))
                    .and_then(|p| {
                        std::path::Path::new(&p)
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                    }),
            };
            match kind.as_str() {
                "audio" => audio.push(track),
                "sub" => subtitle.push(track),
                _ => {}
            }
        }

        let count = self.mpv.get_i64("chapters").unwrap_or(0).max(0);
        let chapters: Vec<ChapterInfo> = (0..count)
            .map(|i| ChapterInfo {
                index: i,
                title: self.mpv.get_property_string(&format!("chapter-list/{i}/title")),
                time: self
                    .mpv
                    .get_f64(&format!("chapter-list/{i}/time"))
                    .unwrap_or(0.0),
            })
            .collect();

        MediaTracks {
            audio,
            subtitle,
            chapters,
        }
    }

    fn set_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        // CSS px → device px using the window's authoritative scale factor.
        let scale = self
            .app
            .get_webview_window("main")
            .and_then(|w| w.scale_factor().ok())
            .unwrap_or(1.0);
        let px = (x * scale).round() as i32;
        let py = (y * scale).round() as i32;
        let pw = ((w * scale).round() as i32).max(1);
        let ph = ((h * scale).round() as i32).max(1);
        tracing::trace!(?x, ?y, ?w, ?h, scale, px, py, pw, ph, "player set_rect");
        // Compositing path: move a DirectComposition visual (atomic, no airspace).
        // Also derive the pane's margins from the window so the Rust-side resize
        // handler can recompute the pane during a fullscreen/edge-drag animation.
        #[cfg(windows)]
        if let Some(comp) = &self.compositor {
            let (win_w, win_h) = self
                .app
                .get_webview_window("main")
                .and_then(|w| w.inner_size().ok())
                .map(|s| (s.width as i32, s.height as i32))
                .unwrap_or((px + pw, py + ph));
            let insets = (px, py, (win_w - (px + pw)).max(0), (win_h - (py + ph)).max(0));
            comp.set_rect(px, py, pw, ph, insets);
            return;
        }
        let child = self.child;
        let _ = self
            .app
            .run_on_main_thread(move || move_child(child, px, py, pw, ph));
    }

    fn show(&self, visible: bool) {
        let child = self.child;
        let _ = self
            .app
            .run_on_main_thread(move || show_child(child, visible));
    }
}

fn spawn_pump(inner: Arc<PlayerInner>) {
    std::thread::spawn(move || {
        let mut last_poll = Instant::now();
        loop {
            let ev = inner.mpv.wait_event(0.1);
            if !ev.is_null() {
                let id = unsafe { (*ev).event_id };
                match id {
                    MPV_EVENT_SHUTDOWN => return,
                    MPV_EVENT_FILE_LOADED => {
                        // Ensure playback starts (pause set before load can be reset).
                        inner.mpv.set_property("pause", "no").ok();
                        let duration = inner.mpv.get_f64("duration").unwrap_or(0.0);
                        tracing::debug!(duration, "mpv file loaded");
                    }
                    MPV_EVENT_END_FILE => {
                        let data = unsafe { (*ev).data } as *const MpvEventEndFile;
                        let reason = if data.is_null() {
                            -1
                        } else {
                            unsafe { (*data).reason }
                        };
                        tracing::debug!(reason, "mpv end-file");
                        if reason == MPV_END_FILE_REASON_EOF {
                            inner.on_ended();
                        }
                    }
                    _ => {}
                }
            }
            // Poll mpv ~5x/sec and push state to the UI. Robust to missed
            // property-change events (which were unreliable when embedded).
            if last_poll.elapsed() >= Duration::from_millis(200) {
                last_poll = Instant::now();
                inner.tick(true);
            }
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
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowPos, SWP_NOACTIVATE};
    // HWND_TOP (null) + no SWP_NOZORDER → raise above the WebView2 sibling so
    // the video is visible (otherwise it renders behind the webview → black).
    unsafe {
        SetWindowPos(hwnd as _, std::ptr::null_mut(), x, y, w, h, SWP_NOACTIVATE);
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
