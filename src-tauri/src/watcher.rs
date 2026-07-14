//! Filesystem watcher for auto-rescan. Watches library roots (recursively) and
//! standalone course folders; on a debounced change it re-imports the affected
//! course(s) — which preserves user data (see importer) — and emits
//! `library:changed` so the UI refreshes.

use crate::db::queries;
use crate::player::PlayerService;
use crate::state::AppState;
use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{new_debouncer, DebounceEventResult, Debouncer};
use rusqlite::Connection;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

pub struct LibraryWatcher {
    debouncer: Debouncer<RecommendedWatcher>,
    watched: Mutex<HashSet<PathBuf>>,
}

impl LibraryWatcher {
    /// Create the watcher and wire the debounced handler to the given app.
    pub fn start(app: AppHandle) -> Result<Self, notify_debouncer_mini::notify::Error> {
        let handler_app = app.clone();
        let debouncer = new_debouncer(
            Duration::from_secs(2),
            move |res: DebounceEventResult| {
                if let Ok(events) = res {
                    let paths: Vec<PathBuf> = events.into_iter().map(|e| e.path).collect();
                    handle_changes(&handler_app, paths);
                }
            },
        )?;
        Ok(Self {
            debouncer,
            watched: Mutex::new(HashSet::new()),
        })
    }

    /// Watch a path recursively if not already watched.
    pub fn watch(&mut self, path: &Path) {
        let mut set = match self.watched.lock() {
            Ok(s) => s,
            Err(_) => return,
        };
        if set.contains(path) || !path.exists() {
            return;
        }
        if self
            .debouncer
            .watcher()
            .watch(path, RecursiveMode::Recursive)
            .is_ok()
        {
            set.insert(path.to_path_buf());
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
        let n = self.watched.lock().map(|s| s.len()).unwrap_or(0);
        tracing::info!(paths = n, "library watcher active");
    }
}

/// Map changed paths to owning courses (or new course folders under a root) and
/// re-import them, then notify the UI.
fn handle_changes(app: &AppHandle, paths: Vec<PathBuf>) {
    let state = app.state::<AppState>();

    // Auto-rescan is opt-in (off by default) — see AppConfig::auto_rescan.
    if !state
        .config
        .lock()
        .map(|c| c.auto_rescan)
        .unwrap_or(false)
    {
        return;
    }

    // Never re-import the course currently playing — it would invalidate the
    // live player's lecture ids mid-session. Lock player BEFORE db (matching the
    // app-wide player→db order) so we don't invert lock ordering.
    let active_lecture = state
        .player
        .lock()
        .ok()
        .and_then(|g| g.as_ref().and_then(|p| p.state().lecture_id));

    tracing::info!(count = paths.len(), "auto-rescan: filesystem change detected");
    // Gather what to re-import under one brief lock (reads only).
    let (active_course, courses, roots) = {
        let conn = match state.db.lock() {
            Ok(g) => g,
            Err(_) => return,
        };
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
    for p in &paths {
        if let Some((id, folder, root_id)) = courses.iter().find(|(_, f, _)| p.starts_with(f)) {
            if Some(id.as_str()) == active_course.as_deref() {
                continue; // skip the course being watched
            }
            to_import
                .entry(PathBuf::from(folder))
                .or_insert_with(|| root_id.clone());
        } else if let Some((rid, rpath)) = roots.iter().find(|(_, rp)| p.starts_with(rp)) {
            if let Some(child) = immediate_child(Path::new(rpath), p) {
                if child.is_dir() {
                    to_import.entry(child).or_insert_with(|| Some(rid.clone()));
                }
            }
        }
    }

    // Re-import each folder in three phases so the probe (phase 2) runs without
    // the DB lock — a background rescan no longer freezes the UI.
    let mut changed = false;
    for (folder, root_id) in &to_import {
        let snap = {
            let conn = match state.db.lock() {
                Ok(g) => g,
                Err(_) => continue,
            };
            match state.importer.read_snapshot(&conn, folder) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan snapshot failed");
                    continue;
                }
            }
        };
        let plan = match state.importer.build(folder, &snap) {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan build failed");
                continue;
            }
        };
        let mut guard = match state.db.lock() {
            Ok(g) => g,
            Err(_) => continue,
        };
        let conn: &mut Connection = &mut guard;
        match state.importer.persist(conn, root_id.as_deref(), &snap, &plan) {
            Ok(_) => {
                changed = true;
                tracing::info!(folder = %folder.display(), "auto-rescan re-imported course");
            }
            Err(e) => {
                tracing::warn!(folder = %folder.display(), error = %e, "auto-rescan re-import failed")
            }
        }
    }

    if changed {
        let _ = app.emit("library:changed", ());
    }
}

/// The immediate child of `root` on the way to `target` (a possibly-new course).
fn immediate_child(root: &Path, target: &Path) -> Option<PathBuf> {
    let rest = target.strip_prefix(root).ok()?;
    let first = rest.components().next()?;
    Some(root.join(first))
}
