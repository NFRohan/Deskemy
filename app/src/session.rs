//! Lecture-aware playback: which lecture of which course is playing, the
//! course playlist, and everything persisted while watching — progress,
//! completion, watch time, the resume pointer and per-course speed/track
//! prefs. The rules match the Tauri player (`src-tauri/src/player/mod.rs`).

use crate::settings::{self, Config};
use deskemy_core::db::{queries, Connection};
use deskemy_core::domain::{Attachment, Bookmark, CourseDetail};
use deskemy_core::importer::structure::clean_title;
use deskemy_core::mpv::Mpv;
use deskemy_core::playback::{resume_start, watched_enough};
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

pub type Db = Arc<Mutex<Connection>>;

/// How often progress and watch time are written while playing.
const SAVE_EVERY: Duration = Duration::from_secs(5);

/// Header / "Up next" text for the loaded lecture.
#[derive(Clone, Default)]
pub struct NowPlaying {
    pub title: String,
    pub section: String,
    /// The course ("" and "" for a bare file).
    pub course: String,
    pub course_id: String,
    pub up_next: Option<String>,
    pub has_previous: bool,
    pub has_next: bool,
    /// A library lecture (a bare file has no bookmarks or progress).
    pub in_library: bool,
    /// Autoplay stopped at the end of this lecture to offer its resources
    /// ("Pause on exercises and resources").
    pub resources_waiting: bool,
    /// The course plays at its own saved speed rather than the default: the
    /// default's label ("1.25×"), for the speed menu's "Use default". Empty
    /// when the default applies.
    pub speed_default: String,
}

/// The sleep timer: pause after a while, or when the playing lecture ends.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Sleep {
    #[default]
    Off,
    /// Pause at `deadline`; `minutes` is what was asked for (for the menu).
    At { deadline: Instant, minutes: u32 },
    /// Stop at the end of the playing lecture instead of moving on.
    EndOfLecture,
}

impl Sleep {
    /// Pause after `minutes` (clamped to 1..=600, as in the Tauri player).
    pub fn after(minutes: u32) -> Self {
        let minutes = minutes.clamp(1, 600);
        Sleep::At {
            deadline: Instant::now() + Duration::from_secs(minutes as u64 * 60),
            minutes,
        }
    }
}

/// Where a lecture starts playing.
#[derive(Clone, Copy)]
enum Start {
    /// Where it was left (see `resume_start`).
    Resume,
    /// From the top — previous / next / autoplay / replay.
    Beginning,
    At(f64),
}

#[derive(Clone)]
struct Item {
    lecture_id: String,
    path: String,
}

#[derive(Default)]
struct Inner {
    course_id: Option<String>,
    items: Vec<Item>,
    index: usize,
    /// None for a bare file opened outside the library (nothing is saved).
    lecture_id: Option<String>,
    duration: f64,
    now: NowPlaying,
    /// Bumped whenever `now` changes, so the UI only re-reads it then.
    revision: u64,
    /// Reached the end without advancing; play restarts the lecture.
    ended: bool,
    last_save: Option<Instant>,
    last_tick: Option<Instant>,
    /// Real seconds watched since the last flush to `daily_activity`.
    watch_accum: f64,
    /// Lectures already counted as completed today (count each once).
    completed: HashSet<String>,
    sleep: Sleep,
}

/// Speeds from the menu and from config: equal, give or take float noise.
fn same_speed(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-6
}

pub struct Session {
    mpv: Arc<Mpv>,
    db: Db,
    config: Config,
    // Lock order: `inner`, then `db`. Never the other way around.
    inner: Mutex<Inner>,
}

impl Session {
    pub fn new(mpv: Arc<Mpv>, db: Db, config: Config) -> Self {
        Session {
            mpv,
            db,
            config,
            inner: Mutex::new(Inner::default()),
        }
    }

    fn inner(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn db(&self) -> MutexGuard<'_, Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Open a lecture — resuming where it was left — with its course as the
    /// playlist.
    pub fn open(&self, lecture_id: &str) -> Result<(), String> {
        self.open_at(lecture_id, None)
    }

    /// Open a lecture at `start` seconds (a bookmark, or history's "watch
    /// again" from 0); `None` resumes where it was left.
    pub fn open_at(&self, lecture_id: &str, start: Option<f64>) -> Result<(), String> {
        self.save_now();
        let (course_id, items) = {
            let db = self.db();
            let (_, course_id, _) = queries::get_lecture_playback(&db, lecture_id)
                .map_err(|e| e.to_string())?
                .ok_or_else(|| format!("lecture {lecture_id} not found"))?;
            let items = queries::list_course_playlist(&db, &course_id).map_err(|e| e.to_string())?;
            (course_id, items)
        };
        {
            let mut inner = self.inner();
            inner.index = items.iter().position(|(id, _)| id == lecture_id).unwrap_or(0);
            inner.items = items
                .into_iter()
                .map(|(lecture_id, path)| Item { lecture_id, path })
                .collect();
            inner.course_id = Some(course_id);
        }
        self.load_current(start.map_or(Start::Resume, Start::At))
    }

    /// Play a file that isn't in the library. Nothing is persisted.
    pub fn open_file(&self, path: &Path) -> Result<(), String> {
        self.save_now();
        let path = path.to_string_lossy().into_owned();
        self.mpv.command(&["loadfile", &path]).map_err(|e| e.to_string())?;
        let mut inner = self.inner();
        *inner = Inner {
            completed: std::mem::take(&mut inner.completed),
            sleep: inner.sleep,
            revision: inner.revision + 1,
            now: NowPlaying {
                title: file_title(&path),
                ..NowPlaying::default()
            },
            ..Inner::default()
        };
        Ok(())
    }

    /// Previous / next lecture in the course, from the start.
    pub fn step(&self, delta: i32) -> Result<(), String> {
        let target = {
            let inner = self.inner();
            let target = inner.index as i64 + delta as i64;
            if target < 0 || target as usize >= inner.items.len() {
                return Ok(());
            }
            target as usize
        };
        self.save_now();
        self.inner().index = target;
        self.load_current(Start::Beginning)
    }

    /// Play again after reaching the end (mpv is idle by then).
    pub fn replay_if_ended(&self) -> bool {
        if !self.inner().ended {
            return false;
        }
        self.load_current(Start::Beginning).is_ok()
    }

    fn load_current(&self, from: Start) -> Result<(), String> {
        let mut inner = self.inner();
        let item = inner.items.get(inner.index).cloned().ok_or("empty playlist")?;
        let course_id = inner.course_id.clone().unwrap_or_default();

        let db = self.db();
        let (saved, completed, duration) =
            queries::get_progress(&db, &item.lecture_id).unwrap_or((0.0, false, None));
        let start = match from {
            Start::Resume => resume_start(true, saved, completed, duration),
            Start::Beginning => 0.0,
            Start::At(t) => t.max(0.0),
        };
        // A course's saved speed overrides the default where present.
        let prefs = queries::get_course_prefs(&db, &course_id).ok().flatten();
        let default_speed = settings::lock(&self.config).default_speed;
        let saved_speed = prefs.and_then(|p| p.0);
        let speed = saved_speed.unwrap_or(default_speed);
        let view = queries::get_lecture_view(&db, &item.lecture_id).ok().flatten();
        let up_next = inner
            .items
            .get(inner.index + 1)
            .and_then(|n| queries::get_lecture_view(&db, &n.lecture_id).ok().flatten())
            .map(|v| v.0);
        let _ = queries::set_last_lecture(&db, &course_id, &item.lecture_id);
        drop(db);

        // loadfile <url> [<flags> [<index> [<options>]]] — options is the 4th arg.
        let loaded = if start > 1.0 {
            self.mpv
                .command(&["loadfile", &item.path, "replace", "0", &format!("start={start}")])
        } else {
            self.mpv.command(&["loadfile", &item.path])
        };
        loaded.map_err(|e| e.to_string())?;
        let _ = self.mpv.set_property("pause", "no");
        let _ = self.mpv.set_property("speed", &speed.to_string());
        // Remembered audio/subtitle choice (mpv applies it once tracks load).
        if let Some((_, sub_id, subs_on, audio_id)) = prefs {
            if let Some(a) = audio_id {
                let _ = self.mpv.set_property("aid", &a.to_string());
            }
            match (subs_on, sub_id) {
                (true, Some(s)) => {
                    let _ = self.mpv.set_property("sid", &s.to_string());
                }
                (false, _) => {
                    let _ = self.mpv.set_property("sid", "no");
                }
                _ => {}
            }
        }

        let (title, course_id, course, section) = match view {
            Some(view) => view,
            None => (file_title(&item.path), String::new(), String::new(), String::new()),
        };
        inner.now = NowPlaying {
            title,
            section,
            course,
            course_id,
            up_next,
            has_previous: inner.index > 0,
            has_next: inner.index + 1 < inner.items.len(),
            in_library: true,
            resources_waiting: false,
            speed_default: saved_speed
                .filter(|s| !same_speed(*s, default_speed))
                .map(|_| settings::speed_label(default_speed))
                .unwrap_or_default(),
        };
        inner.revision += 1;
        inner.lecture_id = Some(item.lecture_id);
        inner.duration = duration.unwrap_or(0.0);
        inner.ended = false;
        let now = Instant::now();
        inner.last_save = Some(now);
        inner.last_tick = Some(now);
        Ok(())
    }

    /// Called ~5×/s by the event thread with freshly sampled playback state:
    /// accumulates watch time and persists periodically. `position` is None
    /// while mpv has no file (stopped, or the next one still opening) — then
    /// there's nothing to save: writing 0 there wiped the lecture's real
    /// position.
    pub fn tick(&self, position: Option<f64>, duration: f64, paused: bool) {
        let mut inner = self.inner();
        if duration > 0.0 {
            inner.duration = duration;
        }
        let now = Instant::now();
        let elapsed = inner.last_tick.map_or(0.0, |t| now.duration_since(t).as_secs_f64());
        inner.last_tick = Some(now);
        if !paused && inner.lecture_id.is_some() {
            // Capped so a system suspend or long stall doesn't count as watching.
            inner.watch_accum += elapsed.min(2.0);
        }
        if let Sleep::At { deadline, .. } = inner.sleep {
            if now >= deadline {
                inner.sleep = Sleep::Off;
                let _ = self.mpv.set_property("pause", "yes");
                tracing::info!("sleep timer: paused");
            }
        }
        if inner.last_save.is_none_or(|t| now.duration_since(t) >= SAVE_EVERY) {
            inner.last_save = Some(now);
            if let Some(position) = position {
                self.save(&mut inner, position, false);
            }
            self.flush_watch(&mut inner);
        }
    }

    /// The lecture played to its end: mark it done, then advance if autoplay
    /// is on, there is a next lecture, and the sleep timer isn't set to stop
    /// here.
    pub fn on_eof(&self) {
        let advance = {
            let mut inner = self.inner();
            // The player was left as the lecture ended (its end-of-file event
            // was already queued): nothing to save, nothing to autoplay.
            if inner.lecture_id.is_none() {
                return;
            }
            let duration = inner.duration;
            self.save(&mut inner, duration, true);
            self.flush_watch(&mut inner);
            let sleep_here = inner.sleep == Sleep::EndOfLecture;
            if sleep_here {
                inner.sleep = Sleep::Off;
            }
            let mut advance =
                !sleep_here && settings::lock(&self.config).autoplay_next && inner.index + 1 < inner.items.len();
            // Stop at a lecture with exercises / notes, to offer them first.
            if advance && settings::lock(&self.config).pause_at_resources && !self.resources_of(&inner).is_empty() {
                advance = false;
                inner.now.resources_waiting = true;
            }
            if advance {
                inner.index += 1;
            } else {
                inner.ended = true;
                inner.revision += 1;
            }
            advance
        };
        if advance {
            if let Err(e) = self.load_current(Start::Beginning) {
                tracing::warn!(error = %e, "autoplay next lecture");
            }
        }
    }

    /// Persist everything now — before switching lectures, leaving the player
    /// or quitting. Reads the position live from mpv, so a seek made just
    /// before is not lost.
    pub fn save_now(&self) {
        let position = self.mpv.get_f64("time-pos");
        let mut inner = self.inner();
        if let Some(position) = position {
            self.save(&mut inner, position, false);
        }
        self.flush_watch(&mut inner);
    }

    /// Leaving the player: save, then let go of the lecture, so nothing more
    /// is written to it (mpv is stopped next and reports no position).
    pub fn unload(&self) {
        self.save_now();
        let mut inner = self.inner();
        inner.lecture_id = None;
        inner.last_tick = None;
    }

    /// Change speed and remember it for this course — or, picking the default,
    /// forget the course's own so the default applies again.
    pub fn set_speed(&self, speed: f64) {
        if !self.set("speed", &speed.to_string()) {
            return;
        }
        let default_speed = settings::lock(&self.config).default_speed;
        let own = !same_speed(speed, default_speed);
        self.remember(|db, course| match own {
            true => queries::set_pref_speed(db, course, speed),
            false => queries::clear_pref_speed(db, Some(course)).map(|_| ()),
        });
        let mut inner = self.inner();
        if inner.course_id.is_some() {
            inner.now.speed_default = if own { settings::speed_label(default_speed) } else { String::new() };
            inner.revision += 1;
        }
    }

    /// Back to the default speed (the speed menu's "Use default").
    pub fn use_default_speed(&self) {
        self.set_speed(settings::lock(&self.config).default_speed);
    }

    /// Pick a subtitle track (None = off) and remember it for this course.
    pub fn set_subtitle(&self, sid: Option<i64>) {
        let value = sid.map_or_else(|| "no".to_string(), |s| s.to_string());
        if self.set("sid", &value) {
            self.remember(|db, course| queries::set_pref_subtitle(db, course, sid));
        }
    }

    /// Pick an audio track and remember it for this course.
    pub fn set_audio(&self, aid: i64) {
        if self.set("aid", &aid.to_string()) {
            self.remember(|db, course| queries::set_pref_audio(db, course, Some(aid)));
        }
    }

    pub fn set_chapter(&self, index: i64) {
        self.set("chapter", &index.to_string());
    }

    /// Set an mpv property; only a change mpv accepted is worth remembering.
    fn set(&self, name: &str, value: &str) -> bool {
        match self.mpv.set_property(name, value) {
            Ok(()) => true,
            Err(e) => {
                tracing::warn!(error = %e, "set {name}={value}");
                false
            }
        }
    }

    /// Persist a per-course preference for the loaded course (best-effort).
    fn remember(&self, f: impl FnOnce(&Connection, &str) -> deskemy_core::error::Result<()>) {
        let inner = self.inner();
        let Some(course) = inner.course_id.as_deref() else { return };
        if let Err(e) = f(&self.db(), course) {
            tracing::warn!(error = %e, "save course pref");
        }
    }

    pub fn lecture_id(&self) -> Option<String> {
        self.inner().lecture_id.clone()
    }

    /// The loaded course, with its sections, lectures and their progress.
    pub fn course(&self) -> Option<CourseDetail> {
        let course = self.inner().course_id.clone()?;
        queries::get_course_detail(&self.db(), &course).ok().flatten()
    }

    /// Resource files of the loaded course.
    pub fn attachments(&self) -> Vec<Attachment> {
        let Some(course) = self.inner().course_id.clone() else { return Vec::new() };
        queries::list_course_attachments(&self.db(), &course).unwrap_or_default()
    }

    /// The playing (or just ended) lecture's resources.
    pub fn lecture_resources(&self) -> Vec<Attachment> {
        let inner = self.inner();
        self.resources_of(&inner)
    }

    /// The playing lecture's resources — placed as the curriculum shows them
    /// (its own, plus numbered section resources in lecture order), so the
    /// end-of-lecture pause offers what's listed under it.
    fn resources_of(&self, inner: &Inner) -> Vec<Attachment> {
        let (Some(course), Some(lecture)) = (inner.course_id.as_deref(), inner.lecture_id.as_deref()) else {
            return Vec::new();
        };
        let db = self.db();
        let attachments = queries::list_course_attachments(&db, course).unwrap_or_default();
        let sections = queries::get_course_detail(&db, course).ok().flatten().map(|c| c.sections).unwrap_or_default();
        let (by_lecture, _) = crate::course_page::inline_resources(&sections, &attachments);
        by_lecture.get(lecture).map(|list| list.iter().map(|a| (*a).clone()).collect()).unwrap_or_default()
    }

    /// Mark a resource done or not.
    pub fn set_resource_done(&self, id: &str, done: bool) {
        if let Err(e) = queries::set_attachment_completed(&self.db(), id, done) {
            tracing::warn!(error = %e, "mark resource");
        }
    }

    /// "Keep videos and resources together" (resources under their lectures).
    pub fn resources_inline(&self) -> bool {
        settings::lock(&self.config).resources_inline
    }

    /// Bookmarks of the playing lecture, in time order.
    pub fn bookmarks(&self) -> Vec<Bookmark> {
        let inner = self.inner();
        let Some(lecture) = inner.lecture_id.as_deref() else { return Vec::new() };
        queries::list_bookmarks(&self.db(), lecture).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "list bookmarks");
            Vec::new()
        })
    }

    /// Bookmark `position` in the playing lecture.
    pub fn add_bookmark(&self, position: f64, label: Option<&str>) -> Result<(), String> {
        let inner = self.inner();
        let lecture = inner.lecture_id.as_deref().ok_or("nothing from the library is playing")?;
        queries::add_bookmark(&self.db(), lecture, position, label)
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    pub fn delete_bookmark(&self, id: &str) {
        if let Err(e) = queries::delete_bookmark(&self.db(), id) {
            tracing::warn!(error = %e, "delete bookmark");
        }
    }

    pub fn set_sleep(&self, sleep: Sleep) {
        self.inner().sleep = sleep;
    }

    pub fn sleep(&self) -> Sleep {
        self.inner().sleep
    }

    /// `(revision, header)` — re-read the header when the revision changes.
    pub fn now_playing(&self) -> (u64, NowPlaying) {
        let inner = self.inner();
        (inner.revision, inner.now.clone())
    }

    pub fn is_loaded(&self) -> bool {
        self.inner().lecture_id.is_some()
    }

    fn save(&self, inner: &mut Inner, position: f64, reached_end: bool) {
        let Some(lecture_id) = inner.lecture_id.clone() else { return };
        let done = reached_end || watched_enough(position, inner.duration);
        let db = self.db();
        if let Err(e) = queries::save_progress(&db, &lecture_id, position, done) {
            tracing::warn!(error = %e, "save progress");
        }
        if done && inner.completed.insert(lecture_id) {
            let _ = queries::add_completion(&db);
        }
    }

    fn flush_watch(&self, inner: &mut Inner) {
        let secs = std::mem::take(&mut inner.watch_accum);
        if secs > 0.0 {
            let _ = queries::add_watch_seconds(&self.db(), secs);
        }
    }
}

/// Title for a bare file: its cleaned name, the way the library shows lectures.
fn file_title(path: &str) -> String {
    Path::new(path)
        .file_name()
        .map(|n| clean_title(&n.to_string_lossy()))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskemy_core::config::AppConfig;
    use deskemy_core::importer::Importer;
    use deskemy_core::media::stub::StubProber;

    /// A three-lecture course on disk, imported into a scratch database.
    fn library() -> (tempfile::TempDir, Db, Vec<String>) {
        library_with(&["001 First.mp4", "002 Second.mp4", "003 Third.mp4"])
    }

    /// A scratch library with one course of these files (stub-probed).
    fn library_with(files: &[&str]) -> (tempfile::TempDir, Db, Vec<String>) {
        let tmp = tempfile::tempdir().unwrap();
        let course = tmp.path().join("Course");
        std::fs::create_dir_all(&course).unwrap();
        for &name in files {
            std::fs::write(course.join(name), b"x").unwrap();
        }
        let mut conn = deskemy_core::db::open(&tmp.path().join("deskemy.db")).unwrap();
        let course_id = Importer::new(Box::new(StubProber))
            .import_course(&mut conn, None, &course)
            .unwrap();
        let lectures = queries::list_course_playlist(&conn, &course_id)
            .unwrap()
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        (tmp, Arc::new(Mutex::new(conn)), lectures)
    }

    /// A real mpv core with no video/audio output, or None without libmpv.
    /// With DESKEMY_REQUIRE_LIBMPV set (CI, release QA) a missing or broken
    /// libmpv fails the test instead of skipping it.
    fn headless_mpv() -> Option<Arc<Mpv>> {
        let required = std::env::var_os("DESKEMY_REQUIRE_LIBMPV").is_some();
        let skip = |why: &str| {
            assert!(!required, "{why} (DESKEMY_REQUIRE_LIBMPV is set)");
            eprintln!("{why} — skipping");
        };
        if !deskemy_core::mpv::is_available() {
            skip("libmpv not found");
            return None;
        }
        let mpv = match Mpv::new() {
            Ok(mpv) => mpv,
            Err(e) => {
                skip(&format!("libmpv didn't start: {e}"));
                return None;
            }
        };
        for (name, value) in [("vo", "null"), ("ao", "null"), ("idle", "yes"), ("config", "no")] {
            if let Err(e) = mpv.set_option(name, value) {
                skip(&format!("mpv option {name}: {e}"));
                return None;
            }
        }
        if let Err(e) = mpv.initialize() {
            skip(&format!("mpv didn't initialize: {e}"));
            return None;
        }
        Some(Arc::new(mpv))
    }

    fn progress(db: &Db, lecture: &str) -> (f64, bool) {
        let (position, completed, _) = queries::get_progress(&db.lock().unwrap(), lecture).unwrap();
        (position, completed)
    }

    /// (watch seconds, lectures completed) recorded today.
    fn today(db: &Db) -> (f64, i64) {
        let rows = queries::daily_activity(&db.lock().unwrap()).unwrap();
        rows.last().map_or((0.0, 0), |r| (r.2, r.3))
    }

    #[test]
    fn opening_a_lecture_sets_up_the_course_playlist() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db, settings::shared(AppConfig::default()));

        session.open(&lectures[1]).unwrap();
        let (_, now) = session.now_playing();
        assert_eq!(now.title, "Second");
        assert_eq!(now.up_next.as_deref(), Some("Third"));
        assert!(now.has_previous && now.has_next);

        session.step(1).unwrap();
        let (_, now) = session.now_playing();
        assert_eq!(now.title, "Third");
        assert!(now.up_next.is_none() && !now.has_next);

        // Stepping past either end is a no-op.
        session.step(1).unwrap();
        assert_eq!(session.now_playing().1.title, "Third");
    }

    #[test]
    fn end_of_file_completes_the_lecture_and_autoplays_the_next() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db.clone(), settings::shared(AppConfig::default()));

        session.open(&lectures[0]).unwrap();
        session.tick(Some(99.0), 100.0, false);
        session.on_eof();

        assert_eq!(progress(&db, &lectures[0]), (100.0, true));
        assert_eq!(session.now_playing().1.title, "Second");
        assert_eq!(today(&db).1, 1);
        assert!(today(&db).0 > 0.0, "watch time is flushed at the end");
    }

    #[test]
    // Resuming loads with `loadfile <url> replace 0 start=…`: the index
    // argument is mpv 0.38+, and Ubuntu 24.04 has 0.37 (roadmap, Linux).
    #[cfg_attr(not(windows), ignore = "resume needs mpv 0.38's loadfile; see the roadmap's Linux list")]
    fn no_position_or_leaving_the_player_never_wipes_progress() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db.clone(), settings::shared(AppConfig::default()));
        queries::save_progress(&db.lock().unwrap(), &lectures[0], 42.0, false).unwrap();

        session.open(&lectures[0]).unwrap();
        // mpv has no position yet (or any more): a due save writes nothing.
        session.inner().last_save = None;
        session.tick(None, 100.0, false);
        assert_eq!(progress(&db, &lectures[0]).0, 42.0);

        // After leaving the player, saves stop: a stopped mpv's "0" isn't one.
        session.unload();
        session.inner().last_save = None;
        session.tick(Some(0.0), 0.0, true);
        assert_eq!(progress(&db, &lectures[0]).0, 42.0);
        assert!(!session.is_loaded());
    }

    #[test]
    fn leaving_as_a_lecture_ends_doesnt_autoplay_the_next() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db, settings::shared(AppConfig::default()));
        session.open(&lectures[0]).unwrap();
        session.unload();
        session.on_eof();
        assert!(!session.is_loaded(), "the next lecture wasn't loaded");
    }

    #[test]
    fn a_completion_counts_once_per_session() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db.clone(), settings::shared(AppConfig::default()));

        session.open(&lectures[0]).unwrap();
        session.tick(Some(99.0), 100.0, false);
        session.on_eof();
        session.step(-1).unwrap();
        session.tick(Some(99.0), 100.0, false);
        session.on_eof();

        assert_eq!(today(&db).1, 1);
    }

    #[test]
    fn autoplay_pauses_at_a_lecture_with_resources_when_asked() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) =
            library_with(&["001 First.mp4", "001 First exercises.pdf", "002 Second.mp4", "003 Third.mp4"]);
        let config = settings::shared(AppConfig { pause_at_resources: true, ..AppConfig::default() });
        let session = Session::new(mpv, db, config.clone());

        session.open(&lectures[0]).unwrap();
        assert_eq!(session.lecture_resources().len(), 1);
        session.on_eof();
        let (_, now) = session.now_playing();
        assert_eq!(now.title, "First", "stays on the lecture with the exercise");
        assert!(now.resources_waiting);
        assert!(session.replay_if_ended(), "and play watches it again");

        // Lectures without resources advance as usual…
        session.open(&lectures[1]).unwrap();
        session.on_eof();
        assert_eq!(session.now_playing().1.title, "Third");
        assert!(!session.now_playing().1.resources_waiting);

        // …and with the option off, so does the one with resources.
        settings::lock(&config).pause_at_resources = false;
        session.open(&lectures[0]).unwrap();
        session.on_eof();
        assert_eq!(session.now_playing().1.title, "Second");
    }

    #[test]
    fn without_autoplay_the_last_position_is_kept_and_play_restarts() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let config = AppConfig {
            autoplay_next: false,
            ..AppConfig::default()
        };
        let session = Session::new(mpv, db.clone(), settings::shared(config));

        session.open(&lectures[0]).unwrap();
        session.tick(Some(99.0), 100.0, false);
        session.on_eof();

        assert_eq!(session.now_playing().1.title, "First", "stays on the lecture");
        assert!(session.replay_if_ended());
        assert!(!session.replay_if_ended(), "only once the lecture has ended");
    }

    #[test]
    fn sleep_at_end_of_lecture_stops_instead_of_autoplaying() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db.clone(), settings::shared(AppConfig::default()));

        session.open(&lectures[0]).unwrap();
        session.set_sleep(Sleep::EndOfLecture);
        session.tick(Some(99.0), 100.0, false);
        session.on_eof();

        assert_eq!(session.now_playing().1.title, "First", "did not advance");
        assert_eq!(progress(&db, &lectures[0]), (100.0, true), "still completed");
        assert_eq!(session.sleep(), Sleep::Off, "disarmed after firing");
    }

    #[test]
    fn a_sleep_countdown_pauses_when_it_runs_out() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv.clone(), db, settings::shared(AppConfig::default()));

        session.open(&lectures[0]).unwrap();
        mpv.set_property("pause", "no").unwrap();
        session.set_sleep(Sleep::At { deadline: Instant::now(), minutes: 15 });
        session.tick(Some(10.0), 100.0, false);

        assert_eq!(mpv.get_property_string("pause").as_deref(), Some("yes"));
        assert_eq!(session.sleep(), Sleep::Off);
    }

    #[test]
    fn sleep_minutes_are_clamped() {
        let minutes = |s| match s {
            Sleep::At { minutes, .. } => minutes,
            _ => 0,
        };
        assert_eq!(minutes(Sleep::after(0)), 1);
        assert_eq!(minutes(Sleep::after(45)), 45);
        assert_eq!(minutes(Sleep::after(9999)), 600);
    }

    #[test]
    fn bookmarks_belong_to_the_playing_lecture() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db, settings::shared(AppConfig::default()));

        assert!(session.add_bookmark(1.0, None).is_err(), "nothing playing yet");
        session.open(&lectures[0]).unwrap();
        session.add_bookmark(42.0, Some("Policy JSON")).unwrap();
        session.add_bookmark(7.0, None).unwrap();

        let marks = session.bookmarks();
        let seen: Vec<(f64, Option<&str>)> =
            marks.iter().map(|b| (b.position_seconds, b.label.as_deref())).collect();
        assert_eq!(seen, vec![(7.0, None), (42.0, Some("Policy JSON"))]);

        session.step(1).unwrap();
        assert!(session.bookmarks().is_empty(), "the next lecture has its own");

        session.step(-1).unwrap();
        session.delete_bookmark(&marks[0].id);
        assert_eq!(session.bookmarks().len(), 1);
    }

    #[test]
    fn a_missing_resume_lecture_falls_back_to_the_first() {
        let Some(mpv) = headless_mpv() else { return };
        let (_tmp, db, lectures) = library();
        let session = Session::new(mpv, db.clone(), settings::shared(AppConfig::default()));

        let course_id = {
            let conn = db.lock().unwrap();
            queries::get_lecture_playback(&conn, &lectures[0]).unwrap().unwrap().1
        };
        let candidates = crate::library::lectures_to_open(&db, &course_id, "gone-after-a-rescan");
        assert_eq!(candidates, vec!["gone-after-a-rescan".to_string(), lectures[0].clone()]);
        assert!(session.open(&candidates[0]).is_err());
        assert!(session.open(&candidates[1]).is_ok());
    }
}
