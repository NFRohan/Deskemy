//! Namespaced Tauri commands — the thin IPC surface over db/importer/config.

pub mod player;

use crate::db::queries;
use crate::domain::{Bookmark, BookmarkDetail, CourseDetail, CourseSummary, SearchHit};
use crate::error::{DeskemyError, Result};
use rusqlite::Connection;
use serde::Serialize;
use std::path::Path;
use std::sync::MutexGuard;
use tauri::State;

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

/// Import a single folder as one course.
#[tauri::command]
pub fn library_import_course(state: State<AppState>, path: String) -> Result<String> {
    let mut guard = db(&state)?;
    let conn: &mut Connection = &mut guard;
    state.importer.import_course(conn, None, Path::new(&path))
}

/// Scan a registered root: each immediate subfolder becomes a course.
#[tauri::command]
pub fn library_scan_root(state: State<AppState>, root_id: String) -> Result<ScanResult> {
    let root_path = {
        let conn = db(&state)?;
        queries::list_library_roots(&conn)?
            .into_iter()
            .find(|(id, _)| *id == root_id)
            .map(|(_, p)| p)
            .ok_or_else(|| DeskemyError::NotFound(format!("library root {root_id}")))?
    };

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

// ---------------------------------------------------------------------------
// search_*
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn search_query(state: State<AppState>, query: String) -> Result<Vec<SearchHit>> {
    let conn = db(&state)?;
    queries::search(&conn, &query, 50)
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
