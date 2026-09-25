//! Filesystem watcher for auto-rescan. Watches library roots (recursively) and
//! standalone course folders; on a debounced change it re-imports the affected
//! course(s) — which preserves user data (see importer) — and tells the host,
//! so its UI can refresh.

use crate::db::queries;
use crate::importer::Importer;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// What a rescan needs from the app around it.
pub trait RescanHost: Send + Sync + 'static {
    fn db(&self) -> &Mutex<Connection>;
    fn importer(&self) -> &Importer;
    /// The auto-rescan setting (off by default — see `AppConfig::auto_rescan`).
    fn enabled(&self) -> bool;
    fn clean_titles(&self) -> bool;
    /// The lecture playing now: its course is never re-imported mid-session,
    /// as that would invalidate the player's lecture ids. Called before the
    /// database is locked.
    fn active_lecture(&self) -> Option<String>;
    /// Courses were re-imported.
    fn changed(&self);
}

pub struct LibraryWatcher {
    debouncer: Debouncer<RecommendedWatcher>,
    watched: HashSet<PathBuf>,
}

impl LibraryWatcher {
    /// Create the watcher; debounced changes are rescanned on its thread.
    pub fn start(host: Arc<dyn RescanHost>) -> Result<Self, notify_debouncer_mini::notify::Error> {
        let debouncer = new_debouncer(Duration::from_secs(2), move |res: DebounceEventResult| {
            if let Ok(events) = res {
                let paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
                if rescan(&*host, &paths) {
                    host.changed();
                }
            }
        })?;
        Ok(Self { debouncer, watched: HashSet::new() })
    }

    /// Watch a path recursively if not already watched.
    pub fn watch(&mut self, path: &Path) {
        if self.watched.contains(path) || !path.exists() {
            return;
        }
        if self.debouncer.watcher().watch(path, RecursiveMode::Recursive).is_ok() {
            self.watched.insert(path.to_path_buf());
        }
    }

    /// Watch every library root + standalone course folder currently in the db.
    pub fn sync(&mut self, conn: &Connection) {
        if let Ok(roots) = queries::list_library_roots(conn) {
            for (_, path) in roots {
                self.watch(Path::new(&path));
            }
        }
        if let Ok(courses) = queries::all_course_folders(conn) {
            for (_id, folder, root_id) in courses {
                // Courses under a root are already covered by the root watch.
                if root_id.is_none() {
                    self.watch(Path::new(&folder));
                }
            }
        }
        tracing::info!(paths = self.watched.len(), "library watcher active");
    }
}

fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
}

/// Map changed paths to owning courses (or new course folders under a root)
/// and re-import them. Returns whether anything was re-imported.
pub fn rescan(host: &dyn RescanHost, paths: &[PathBuf]) -> bool {
    if !host.enabled() {
        return false;
    }
    let active_lecture = host.active_lecture();

    tracing::info!(count = paths.len(), "auto-rescan: filesystem change detected");
    // Gather what to re-import under one brief lock (reads only).
    let (active_course, courses, roots) = {
        let conn = lock(host.db());
        let active_course = active_lecture
            .as_deref()
            .and_then(|lid| queries::get_lecture_playback(&conn, lid).ok().flatten())
            .map(|(_, course_id, _)| course_id);
        let courses = queries::all_course_folders(&conn).unwrap_or_default();
        let roots = queries::list_library_roots(&conn).unwrap_or_default();
        (active_course, courses, roots)
    };

    // Dedup to the set of course folders that need re-importing.
    let mut to_import: HashMap<PathBuf, Option<String>> = HashMap::new();
    for p in paths {
        if let Some((id, folder, root_id)) = courses.iter().find(|(_, f, _)| p.starts_with(f)) {
            if Some(id.as_str()) == active_course.as_deref() {
                continue; // skip the course being watched
            }
            to_import.entry(PathBuf::from(folder)).or_insert_with(|| root_id.clone());
        } else if let Some((rid, rpath)) = roots.iter().find(|(_, rp)| p.starts_with(rp)) {
            if let Some(child) = immediate_child(Path::new(rpath), p) {
                if child.is_dir() {
                    to_import.entry(child).or_insert_with(|| Some(rid.clone()));
                }
            }
        }
    }

    // Re-import each folder in three phases so the probe (phase 2) runs without
    // the DB lock — a background rescan doesn't freeze the UI.
    let importer = host.importer();
    let mut changed = false;
    for (folder, root_id) in &to_import {
        let snap = match importer.read_snapshot(&lock(host.db()), folder) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan snapshot failed");
                continue;
            }
        };
        let plan = match importer.build(folder, &snap, host.clean_titles(), |_, _| {}) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan build failed");
                continue;
            }
        };
        match importer.persist(&mut lock(host.db()), root_id.as_deref(), &snap, &plan) {
            Ok(_) => {
                changed = true;
                tracing::info!(folder = %folder.display(), "auto-rescan re-imported course");
            }
            Err(e) => tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan re-import failed"),
        }
    }
    changed
}

/// The immediate child of `root` on the way to `target` (a possibly-new course).
fn immediate_child(root: &Path, target: &Path) -> Option<PathBuf> {
    let rest = target.strip_prefix(root).ok()?;
    let first = rest.components().next()?;
    Some(root.join(first))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::stub::StubProber;
    use std::sync::atomic::{AtomicBool, Ordering};

    struct Host {
        db: Mutex<Connection>,
        importer: Importer,
        enabled: AtomicBool,
        active: Mutex<Option<String>>,
    }

    impl RescanHost for Host {
        fn db(&self) -> &Mutex<Connection> {
            &self.db
        }
        fn importer(&self) -> &Importer {
            &self.importer
        }
        fn enabled(&self) -> bool {
            self.enabled.load(Ordering::SeqCst)
        }
        fn clean_titles(&self) -> bool {
            true
        }
        fn active_lecture(&self) -> Option<String> {
            self.active.lock().unwrap().clone()
        }
        fn changed(&self) {}
    }

    fn lectures(host: &Host, course: &str) -> usize {
        let detail = queries::get_course_detail(&lock(&host.db), course).unwrap().unwrap();
        detail.sections.iter().map(|s| s.lectures.len()).sum()
    }

    #[test]
    fn rescans_changed_courses_and_new_ones_under_a_root() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("courses");
        let rust = root.join("Rust");
        std::fs::create_dir_all(&rust).unwrap();
        std::fs::write(rust.join("001 Hello.mp4"), b"x").unwrap();

        let host = Host {
            db: Mutex::new(crate::db::open_in_memory().unwrap()),
            importer: Importer::new(Box::new(StubProber)),
            enabled: AtomicBool::new(false),
            active: Mutex::new(None),
        };
        let course = {
            let mut conn = lock(&host.db);
            let root_id = queries::add_library_root(&conn, &root.to_string_lossy()).unwrap();
            host.importer.import_course(&mut conn, Some(&root_id), &rust).unwrap()
        };

        // A new lecture appears.
        std::fs::write(rust.join("002 Types.mp4"), b"x").unwrap();
        let changed = [rust.join("002 Types.mp4")];
        assert!(!rescan(&host, &changed), "off unless the setting is on");

        host.enabled.store(true, Ordering::SeqCst);
        // The course playing now is left alone.
        let first = queries::get_course_detail(&lock(&host.db), &course).unwrap().unwrap().sections[0].lectures[0]
            .id
            .clone();
        *host.active.lock().unwrap() = Some(first);
        assert!(!rescan(&host, &changed));
        assert_eq!(lectures(&host, &course), 1);

        *host.active.lock().unwrap() = None;
        assert!(rescan(&host, &changed));
        // Re-importing replaces the course (ids change), so find it by folder.
        let folders = queries::all_course_folders(&lock(&host.db)).unwrap();
        let (id, _, _) = folders.iter().find(|(_, f, _)| Path::new(f) == rust).unwrap();
        assert_eq!(lectures(&host, id), 2);

        // A new course folder under the root is imported.
        let go = root.join("Go");
        std::fs::create_dir_all(&go).unwrap();
        std::fs::write(go.join("001 Intro.mp4"), b"x").unwrap();
        assert!(rescan(&host, &[go.join("001 Intro.mp4")]));
        assert_eq!(queries::all_course_folders(&lock(&host.db)).unwrap().len(), 2);
    }

    #[test]
    fn immediate_child_is_the_course_folder() {
        let root = Path::new("/lib");
        assert_eq!(immediate_child(root, Path::new("/lib/Go/s1/a.mp4")), Some(PathBuf::from("/lib/Go")));
        assert_eq!(immediate_child(root, Path::new("/elsewhere/x")), None);
    }
}
