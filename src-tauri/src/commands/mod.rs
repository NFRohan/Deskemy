//! Namespaced Tauri commands — the thin IPC surface over db/importer/config.

pub mod player;

use crate::db::queries;
use crate::domain::{
    Attachment, Bookmark, BookmarkDetail, CourseDetail, CourseSummary, HistoryEntry, LibraryStats,
    SearchHit, StorageStats, SubtitleHit,
};
use crate::error::{DeskemyError, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::MutexGuard;
use tauri::{AppHandle, Manager, State};

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

#[derive(Serialize)]
pub struct ReconcileReport {
    pub courses_checked: i64,
    pub courses_missing: i64,
    pub files_missing: i64,
}

#[derive(Serialize)]
pub struct GcReport {
    pub removed: i64,
    pub freed_bytes: i64,
}

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
pub fn library_import_course(app: AppHandle, state: State<AppState>, path: String) -> Result<String> {
    let id = {
        let mut guard = db(&state)?;
        let conn: &mut Connection = &mut guard;
        state.importer.import_course(conn, None, Path::new(&path))?
    };
    watch_path(&app, &path);
    Ok(id)
}

/// Scan a registered root: each immediate subfolder becomes a course.
#[tauri::command]
pub fn library_scan_root(
    app: AppHandle,
    state: State<AppState>,
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
        let mut guard = db(&state)?;
        let conn: &mut Connection = &mut guard;
        match state.importer.import_course(conn, Some(&root_id), &path) {
            Ok(_) => imported += 1,
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
    let path = crate::thumbnails::store(&state.thumbnails_dir(), bytes, ext_hint)?;
    let path_str = path.to_string_lossy().into_owned();
    let conn = db(state)?;
    queries::set_thumbnail(&conn, course_id, Some(&path_str))?;
    Ok(path_str)
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
    let mut conn = db(&state)?;
    let tx = conn.transaction()?;
    queries::delete_course(&tx, &id)?;
    tx.commit()?;
    Ok(())
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
    let conn = db(&state)?;
    queries::rebuild_search_index(&conn)?;
    queries::search_index_count(&conn)
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
    let files = {
        let conn = db(&state)?;
        queries::all_subtitle_files(&conn)?
    };
    let mut conn = db(&state)?;
    let tx = conn.transaction()?;
    queries::clear_subtitle_index(&tx)?;
    let mut total = 0i64;
    for (lecture_id, course_id, path) in files {
        let content = match std::fs::read(&path) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(_) => continue, // skip missing/unreadable subtitle files
        };
        for (start_ms, text) in crate::subtitles::parse(&content) {
            queries::insert_subtitle_cue(&tx, &lecture_id, &course_id, start_ms, &text)?;
            total += 1;
        }
    }
    tx.commit()?;
    Ok(total)
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
    use std::collections::HashMap;
    let entries = {
        let conn = db(&state)?;
        queries::all_lecture_files(&conn)?
    };
    let mut total: HashMap<String, i64> = HashMap::new();
    let mut missing: HashMap<String, i64> = HashMap::new();
    let mut files_missing = 0i64;
    for (cid, path) in &entries {
        *total.entry(cid.clone()).or_default() += 1;
        if !Path::new(path).exists() {
            *missing.entry(cid.clone()).or_default() += 1;
            files_missing += 1;
        }
    }

    let conn = db(&state)?;
    let mut courses_missing = 0i64;
    for cid in total.keys() {
        let is_missing = missing.get(cid).copied().unwrap_or(0) > 0;
        queries::set_missing(&conn, cid, is_missing)?;
        if is_missing {
            courses_missing += 1;
        }
    }
    Ok(ReconcileReport {
        courses_checked: total.len() as i64,
        courses_missing,
        files_missing,
    })
}

/// Delete thumbnail-cache files no longer referenced by any course.
#[tauri::command]
pub fn thumbnails_gc(state: State<AppState>) -> Result<GcReport> {
    let referenced: std::collections::HashSet<String> = {
        let conn = db(&state)?;
        queries::all_thumbnail_paths(&conn)?
            .iter()
            .filter_map(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
            .collect()
    };

    let dir = state.thumbnails_dir();
    let mut removed = 0i64;
    let mut freed_bytes = 0i64;
    if dir.exists() {
        for entry in std::fs::read_dir(&dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().into_owned(),
                None => continue,
            };
            if !referenced.contains(&name) {
                let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
                if std::fs::remove_file(&path).is_ok() {
                    removed += 1;
                    freed_bytes += size;
                }
            }
        }
    }
    Ok(GcReport { removed, freed_bytes })
}

// ---------------------------------------------------------------------------
// storage_* — Settings → Storage panel (sizes + reclaim actions)
// ---------------------------------------------------------------------------

/// Total size of the SQLite database files (main + WAL + shared-memory).
fn db_file_bytes(data_dir: &Path) -> u64 {
    ["deskemy.db", "deskemy.db-wal", "deskemy.db-shm"]
        .iter()
        .filter_map(|f| std::fs::metadata(data_dir.join(f)).ok())
        .map(|m| m.len())
        .sum()
}

/// Total size of the (flat, content-addressed) files directly under `dir`.
fn dir_file_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// On-disk footprint of the local stores, for the Settings → Storage panel.
#[tauri::command]
pub fn storage_stats(state: State<AppState>) -> Result<StorageStats> {
    let subtitle_cues = {
        let conn = db(&state)?;
        queries::subtitle_index_count(&conn)?
    };
    Ok(StorageStats {
        db_bytes: db_file_bytes(&state.data_dir),
        thumbnail_bytes: dir_file_bytes(&state.thumbnails_dir()),
        subtitle_cues,
    })
}

/// VACUUM the database to reclaim pages freed by removed courses or cleared
/// indexes (SQLite never shrinks the file on its own). Returns the new size.
#[tauri::command]
pub fn db_compact(state: State<AppState>) -> Result<u64> {
    {
        let conn = db(&state)?;
        conn.execute_batch("VACUUM;")?;
    }
    Ok(db_file_bytes(&state.data_dir))
}

/// Drop the subtitle full-text index — the largest reclaimable chunk. Returns
/// the number of cues removed; the file only shrinks after a compact.
#[tauri::command]
pub fn subtitle_index_clear(state: State<AppState>) -> Result<i64> {
    let conn = db(&state)?;
    let n = queries::subtitle_index_count(&conn)?;
    queries::clear_subtitle_index(&conn)?;
    Ok(n)
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
