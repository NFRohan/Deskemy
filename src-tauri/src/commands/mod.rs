//! Namespaced Tauri commands — the thin IPC surface over db/importer/config.

pub mod player;

use crate::db::queries;
use crate::domain::{
    Attachment, Bookmark, BookmarkDetail, CourseDetail, CourseSummary, HistoryEntry, ImportPreview,
    LibraryStats, SearchHit, StorageStats, SubtitleHit, TrackDetail, TrackSummary,
};
use crate::error::{DeskemyError, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::MutexGuard;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::config::AppConfig;
use crate::state::AppState;

fn db<'a>(state: &'a State<AppState>) -> Result<MutexGuard<'a, Connection>> {
    state
        .db
        .lock()
        .map_err(|_| DeskemyError::Other("database lock poisoned".into()))
}

#[derive(Serialize)]
pub struct RootDto {
    pub id: String,
    pub path: String,
}

#[derive(Serialize)]
pub struct ScanResult {
    pub imported: usize,
    pub errors: Vec<String>,
}

pub use crate::maintenance::{GcReport, ReconcileReport};

// ---------------------------------------------------------------------------
// library_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn library_add_root(state: State<AppState>, path: String) -> Result<String> {
    let conn = db(&state)?;
    queries::add_library_root(&conn, &path)
}

#[tauri::command]
pub fn library_list_roots(state: State<AppState>) -> Result<Vec<RootDto>> {
    let conn = db(&state)?;
    Ok(queries::list_library_roots(&conn)?
        .into_iter()
        .map(|(id, path)| RootDto { id, path })
        .collect())
}

#[tauri::command]
pub fn library_remove_root(state: State<AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::remove_library_root(&conn, &id)
}

/// Add a path to the filesystem watcher if the watcher is running.
fn watch_path(app: &AppHandle, path: &str) {
    if let Some(w) = app.try_state::<std::sync::Mutex<crate::watcher::LibraryWatcher>>() {
        if let Ok(mut w) = w.lock() {
            w.watch(Path::new(path));
        }
    }
}

/// Import a single folder as one course.
#[tauri::command]
pub async fn library_import_course(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<String> {
    let course_dir = Path::new(&path);
    // Reuse a staged preview plan if the user previewed this folder first — the
    // probe already ran, so don't repeat it. Otherwise probe now (phases 1+2).
    let staged = state
        .pending_imports
        .lock()
        .ok()
        .and_then(|mut m| m.remove(&path));
    let (snap, plan) = match staged {
        Some(sp) => sp,
        None => {
            let snap = {
                let conn = db(&state)?;
                state.importer.read_snapshot(&conn, course_dir)?
            };
            let clean = state.config.lock().map(|c| c.clean_titles).unwrap_or(true);
            let plan = state.importer.build(course_dir, &snap, clean, |_, _| {})?;
            (snap, plan)
        }
    };
    // Phase 3 (brief lock): persist.
    let id = {
        let mut guard = db(&state)?;
        let conn: &mut Connection = &mut guard;
        state.importer.persist(conn, None, &snap, &plan)?
    };
    watch_path(&app, &path);
    Ok(id)
}

/// Repoint a course whose folder was moved or renamed to a new location, keeping
/// all progress/bookmarks/tags/track membership (only the stored paths change).
/// Refuses folders that don't actually contain the course's files.
#[tauri::command]
pub fn library_relocate_course(
    app: AppHandle,
    state: State<AppState>,
    course_id: String,
    new_folder: String,
) -> Result<()> {
    let new_folder = crate::courses::relocate(&*db(&state)?, &course_id, &new_folder)?;
    watch_path(&app, &new_folder);
    Ok(())
}

/// Dry-run an import: probe the folder and return what it would create, staging
/// the probed plan so a following `library_import_course` doesn't re-probe.
///
/// `async` so Tauri runs it on the async runtime rather than the main thread —
/// the probe is slow (one mpv open per video) and would otherwise freeze the UI.
#[tauri::command]
pub async fn library_preview_import(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportPreview> {
    let course_dir = Path::new(&path);
    // Phases 1+2 (probe is lock-free). Stream per-video progress to the UI.
    let snap = {
        let conn = db(&state)?;
        state.importer.read_snapshot(&conn, course_dir)?
    };
    let clean = state.config.lock().map(|c| c.clean_titles).unwrap_or(true);
    let plan = state.importer.build(course_dir, &snap, clean, |done, total| {
        let _ = app.emit("import:progress", (done, total));
    })?;

    let preview = ImportPreview {
        title: snap.title().to_string(),
        is_reimport: snap.is_reimport(),
        sections: plan.section_count() as i64,
        lectures: plan.lecture_count() as i64,
        resources: plan.resource_count() as i64,
        subtitles: plan.subtitle_count() as i64,
        unplayable: plan.unplayable_count() as i64,
        total_duration: plan.total_duration(),
    };

    // Stage this plan (only the latest preview is kept) for confirm-without-reprobe.
    if let Ok(mut pending) = state.pending_imports.lock() {
        pending.clear();
        pending.insert(path, (snap, plan));
    }
    Ok(preview)
}

/// Scan a registered root: each immediate subfolder becomes a course.
#[tauri::command]
pub async fn library_scan_root(
    app: AppHandle,
    state: State<'_, AppState>,
    root_id: String,
) -> Result<ScanResult> {
    let root_path = {
        let conn = db(&state)?;
        queries::list_library_roots(&conn)?
            .into_iter()
            .find(|(id, _)| *id == root_id)
            .map(|(_, p)| p)
            .ok_or_else(|| DeskemyError::NotFound(format!("library root {root_id}")))?
    };
    // Watch the whole root so new courses under it are picked up too.
    watch_path(&app, &root_path);

    let mut imported = 0;
    let mut errors = Vec::new();
    for entry in std::fs::read_dir(&root_path)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                errors.push(e.to_string());
                continue;
            }
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        // Three-phase per course so each probe (phase 2) runs lock-free — a big
        // root scan no longer freezes the UI for its whole duration.
        let result = (|| -> Result<()> {
            let snap = {
                let conn = db(&state)?;
                state.importer.read_snapshot(&conn, &path)?
            };
            let clean = state.config.lock().map(|c| c.clean_titles).unwrap_or(true);
            let plan = state.importer.build(&path, &snap, clean, |_, _| {})?;
            let mut guard = db(&state)?;
            let conn: &mut Connection = &mut guard;
            state.importer.persist(conn, Some(&root_id), &snap, &plan)?;
            Ok(())
        })();
        match result {
            Ok(()) => imported += 1,
            Err(e) => errors.push(format!("{}: {e}", path.display())),
        }
    }

    Ok(ScanResult { imported, errors })
}

#[tauri::command]
pub fn library_list_courses(state: State<AppState>) -> Result<Vec<CourseSummary>> {
    let conn = db(&state)?;
    queries::list_course_summaries(&conn)
}

// ---------------------------------------------------------------------------
// course_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn course_get(state: State<AppState>, id: String) -> Result<Option<CourseDetail>> {
    let conn = db(&state)?;
    queries::get_course_detail(&conn, &id)
}

#[tauri::command]
pub fn course_set_favorite(state: State<AppState>, id: String, favorite: bool) -> Result<()> {
    let conn = db(&state)?;
    queries::set_favorite(&conn, &id, favorite)
}

#[tauri::command]
pub fn course_touch_opened(state: State<AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::touch_opened(&conn, &id)
}

/// Store image bytes as a course's thumbnail and record its path. Returns the
/// stored absolute path (for the frontend to display via the asset protocol).
fn set_course_thumb(
    state: &State<AppState>,
    course_id: &str,
    bytes: &[u8],
    ext_hint: Option<&str>,
) -> Result<String> {
    crate::courses::set_cover(&*db(state)?, &state.thumbnails_dir(), course_id, bytes, ext_hint)
}

/// Set a course thumbnail from a local image file (from the native picker).
#[tauri::command]
pub fn course_set_thumbnail_file(
    state: State<AppState>,
    id: String,
    src_path: String,
) -> Result<String> {
    let bytes = std::fs::read(&src_path)?;
    let ext = Path::new(&src_path).extension().and_then(|e| e.to_str());
    set_course_thumb(&state, &id, &bytes, ext)
}

/// Set a course thumbnail from base64-encoded image bytes (from clipboard paste).
#[tauri::command]
pub fn course_set_thumbnail_bytes(
    state: State<AppState>,
    id: String,
    data_base64: String,
    ext: Option<String>,
) -> Result<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(data_base64.as_bytes())
        .map_err(|e| DeskemyError::Other(format!("invalid image data: {e}")))?;
    set_course_thumb(&state, &id, &bytes, ext.as_deref())
}

/// Remove a course's thumbnail (reverts to the placeholder).
#[tauri::command]
pub fn course_clear_thumbnail(state: State<AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::set_thumbnail(&conn, &id, None)
}

/// A course's non-media resources (pdfs, archives, code files, …).
#[tauri::command]
pub fn course_attachments(state: State<AppState>, course_id: String) -> Result<Vec<Attachment>> {
    let conn = db(&state)?;
    queries::list_course_attachments(&conn, &course_id)
}

#[tauri::command]
pub fn course_tags(state: State<AppState>, course_id: String) -> Result<Vec<String>> {
    let conn = db(&state)?;
    queries::tags_for_course(&conn, &course_id)
}

/// Add a tag to a course; returns the course's updated tag list.
#[tauri::command]
pub fn course_add_tag(state: State<AppState>, course_id: String, tag: String) -> Result<Vec<String>> {
    let conn = db(&state)?;
    queries::add_tag(&conn, &course_id, &tag)
}

#[tauri::command]
pub fn course_remove_tag(
    state: State<AppState>,
    course_id: String,
    tag: String,
) -> Result<Vec<String>> {
    let conn = db(&state)?;
    queries::remove_tag(&conn, &course_id, &tag)
}

/// Open a resource file with the OS default application.
#[tauri::command]
pub fn open_resource(app: AppHandle, path: String) -> Result<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| DeskemyError::Other(e.to_string()))
}

/// Remove a course from the library (DB only — does not touch files on disk).
/// Cascades to its sections/lectures/progress/bookmarks and the search index.
#[tauri::command]
pub fn library_delete_course(state: State<AppState>, id: String) -> Result<()> {
    crate::courses::delete(&mut *db(&state)?, &id)
}

/// Manually mark a lecture complete/incomplete.
#[tauri::command]
pub fn lecture_set_completed(
    state: State<AppState>,
    lecture_id: String,
    completed: bool,
) -> Result<()> {
    let conn = db(&state)?;
    queries::set_completed(&conn, &lecture_id, completed)
}

// ---------------------------------------------------------------------------
// bookmark_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn bookmark_add(
    state: State<AppState>,
    lecture_id: String,
    position_seconds: f64,
    label: Option<String>,
) -> Result<Bookmark> {
    let conn = db(&state)?;
    queries::add_bookmark(&conn, &lecture_id, position_seconds, label.as_deref())
}

#[tauri::command]
pub fn bookmark_list(state: State<AppState>, lecture_id: String) -> Result<Vec<Bookmark>> {
    let conn = db(&state)?;
    queries::list_bookmarks(&conn, &lecture_id)
}

#[tauri::command]
pub fn bookmark_delete(state: State<AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::delete_bookmark(&conn, &id)
}

/// All bookmarks across the library, for the global bookmarks page.
#[tauri::command]
pub fn bookmark_list_all(state: State<AppState>) -> Result<Vec<BookmarkDetail>> {
    let conn = db(&state)?;
    queries::list_all_bookmarks(&conn)
}

/// Recently-watched lectures (newest first) for the playback-history page.
#[tauri::command]
pub fn history_list(state: State<AppState>) -> Result<Vec<HistoryEntry>> {
    let conn = db(&state)?;
    queries::list_history(&conn, 500)
}

// ---------------------------------------------------------------------------
// search_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn search_query(state: State<AppState>, query: String) -> Result<Vec<SearchHit>> {
    let conn = db(&state)?;
    queries::search(&conn, &query, 50)
}

/// Rebuild the search index from the base tables; returns the new row count.
#[tauri::command]
pub fn search_reindex(state: State<AppState>) -> Result<i64> {
    crate::maintenance::reindex_search(&*db(&state)?)
}

/// Full-text search over subtitle text; returns snippets with jump timestamps.
#[tauri::command]
pub fn subtitle_search(state: State<AppState>, query: String) -> Result<Vec<SubtitleHit>> {
    let conn = db(&state)?;
    queries::subtitle_search(&conn, &query, 50)
}

/// (Re)build the subtitle text index by parsing every sidecar subtitle file.
/// Returns the number of indexed cues.
#[tauri::command]
pub fn subtitles_reindex(state: State<AppState>) -> Result<i64> {
    crate::maintenance::reindex_subtitles(&state.db)
}

/// Aggregate library + watch statistics for the stats page.
#[tauri::command]
pub fn stats_get(state: State<AppState>) -> Result<LibraryStats> {
    let goal = state
        .config
        .lock()
        .map(|c| c.daily_goal_minutes)
        .unwrap_or(30);
    let conn = db(&state)?;
    let mut stats = queries::stats(&conn)?;
    stats.daily_goal_minutes = goal;
    Ok(stats)
}

// ---------------------------------------------------------------------------
// maintenance
// ---------------------------------------------------------------------------

/// Check every lecture's file on disk and flag courses with missing files as
/// Missing (or clear the flag when the files are back).
#[tauri::command]
pub fn library_reconcile(state: State<AppState>) -> Result<ReconcileReport> {
    crate::maintenance::reconcile(&state.db)
}

/// Delete thumbnail-cache files no longer referenced by any course.
#[tauri::command]
pub fn thumbnails_gc(state: State<AppState>) -> Result<GcReport> {
    crate::maintenance::gc_thumbnails(&*db(&state)?, &state.thumbnails_dir())
}

// ---------------------------------------------------------------------------
// track_* — career tracks (ordered course groupings)
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn track_list(state: State<AppState>) -> Result<Vec<TrackSummary>> {
    let conn = db(&state)?;
    queries::list_tracks(&conn)
}

#[tauri::command]
pub fn track_get(state: State<AppState>, id: String) -> Result<Option<TrackDetail>> {
    let conn = db(&state)?;
    queries::get_track(&conn, &id)
}

#[tauri::command]
pub fn track_create(
    state: State<AppState>,
    name: String,
    description: Option<String>,
) -> Result<String> {
    let conn = db(&state)?;
    queries::create_track(&conn, name.trim(), description.as_deref())
}

#[tauri::command]
pub fn track_update(
    state: State<AppState>,
    id: String,
    name: String,
    description: Option<String>,
) -> Result<()> {
    let conn = db(&state)?;
    queries::update_track(&conn, &id, name.trim(), description.as_deref())
}

#[tauri::command]
pub fn track_delete(state: State<AppState>, id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::delete_track(&conn, &id)
}

#[tauri::command]
pub fn track_add_course(state: State<AppState>, track_id: String, course_id: String) -> Result<()> {
    let conn = db(&state)?;
    queries::add_course_to_track(&conn, &track_id, &course_id)
}

#[tauri::command]
pub fn track_remove_course(
    state: State<AppState>,
    track_id: String,
    course_id: String,
) -> Result<()> {
    let conn = db(&state)?;
    queries::remove_course_from_track(&conn, &track_id, &course_id)
}

#[tauri::command]
pub fn track_reorder_courses(
    state: State<AppState>,
    track_id: String,
    course_ids: Vec<String>,
) -> Result<()> {
    let conn = db(&state)?;
    queries::reorder_track_courses(&conn, &track_id, &course_ids)
}

// ---------------------------------------------------------------------------
// storage_* — Settings → Storage panel (sizes + reclaim actions)
// ---------------------------------------------------------------------------

/// Total size of the SQLite database files (main + WAL + shared-memory).
/// On-disk footprint of the local stores, for the Settings → Storage panel.
#[tauri::command]
pub fn storage_stats(state: State<AppState>) -> Result<StorageStats> {
    crate::maintenance::storage(&*db(&state)?, &state.data_dir, &state.thumbnails_dir())
}

/// VACUUM the database to reclaim pages freed by removed courses or cleared
/// indexes (SQLite never shrinks the file on its own). Returns the new size.
#[tauri::command]
pub fn db_compact(state: State<AppState>) -> Result<u64> {
    crate::maintenance::compact(&*db(&state)?, &state.data_dir)
}

/// Drop the subtitle full-text index — the largest reclaimable chunk. Returns
/// the number of cues removed; the file only shrinks after a compact.
#[tauri::command]
pub fn subtitle_index_clear(state: State<AppState>) -> Result<i64> {
    crate::maintenance::clear_subtitles(&*db(&state)?)
}

// ---------------------------------------------------------------------------
// config_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn config_get(state: State<AppState>) -> Result<AppConfig> {
    let cfg = state
        .config
        .lock()
        .map_err(|_| DeskemyError::Other("config lock poisoned".into()))?;
    Ok(cfg.clone())
}

#[tauri::command]
pub fn config_set(state: State<AppState>, config: AppConfig) -> Result<()> {
    config.save(&state.config_path)?;
    let mut cfg = state
        .config
        .lock()
        .map_err(|_| DeskemyError::Other("config lock poisoned".into()))?;
    *cfg = config;
    Ok(())
}

// ---------------------------------------------------------------------------
// data_* — export / import the app data (db + config + thumbnails) as one zip.
// Async so the VACUUM + zip run off the UI thread.

/// Write a `.zip` snapshot of the library to `dest` (a path from a save dialog).
#[tauri::command]
pub async fn data_export(app: AppHandle, state: State<'_, AppState>, dest: String) -> Result<()> {
    crate::maintenance::export_backup(
        &state.db,
        &state.data_dir,
        &state.config_path,
        &state.thumbnails_dir(),
        Path::new(&dest),
        &app.package_info().version.to_string(),
    )
}

/// Validate `src`, stage it, and restart so the swap is applied before the db is
/// reopened. On a bad/newer archive this returns an error and does NOT restart.
#[tauri::command]
pub async fn data_import(app: AppHandle, state: State<'_, AppState>, src: String) -> Result<()> {
    crate::backup::stage_import(&state.data_dir, Path::new(&src), crate::db::SCHEMA_VERSION)?;
    app.restart();
}
