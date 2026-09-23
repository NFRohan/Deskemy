//! Course-level edits shared by every frontend: relocating a moved course,
//! setting its cover, and removing it from the library.

use crate::db::{queries, Connection};
use crate::error::{DeskemyError, Result};
use std::path::Path;

/// Where course covers are stored, under the data directory.
pub const THUMBNAILS_DIR: &str = "thumbnails";

/// Point a course at the folder it was moved or renamed to. Its lectures and
/// resources are re-pathed in place, so progress, bookmarks and tags survive.
/// Refuses a folder that doesn't hold the course's files (a sampled lecture
/// must exist at the same relative path). Returns the normalized new folder.
pub fn relocate(conn: &Connection, course_id: &str, new_folder: &str) -> Result<String> {
    let new_folder = new_folder.trim_end_matches(['/', '\\']).to_string();
    if !Path::new(&new_folder).is_dir() {
        return Err(DeskemyError::NotFound(format!("folder not found: {new_folder}")));
    }
    let old_folder = queries::course_folder(conn, course_id)?
        .ok_or_else(|| DeskemyError::NotFound("course not found".into()))?;
    if let Some(sample) = queries::first_lecture_path(conn, course_id)? {
        if let Ok(rel) = Path::new(&sample).strip_prefix(&old_folder) {
            if !Path::new(&new_folder).join(rel).exists() {
                return Err(DeskemyError::Other(
                    "That folder doesn't contain this course's files. Pick the folder the \
                     course was moved or renamed to."
                        .into(),
                ));
            }
        }
    }
    queries::relocate_course(conn, course_id, &old_folder, &new_folder)?;
    Ok(new_folder)
}

/// Store image `bytes` as a course's cover (content-addressed under
/// `thumbs_dir`) and point the course at it. Returns the stored path.
pub fn set_cover(
    conn: &Connection,
    thumbs_dir: &Path,
    course_id: &str,
    bytes: &[u8],
    ext_hint: Option<&str>,
) -> Result<String> {
    let path = crate::thumbnails::store(thumbs_dir, bytes, ext_hint)?;
    let path = path.to_string_lossy().into_owned();
    queries::set_thumbnail(conn, course_id, Some(&path))?;
    Ok(path)
}

/// Remove a course from the library, in one transaction. Only the database
/// is touched — the files on disk stay where they are.
pub fn delete(conn: &mut Connection, course_id: &str) -> Result<()> {
    let tx = conn.transaction()?;
    queries::delete_course(&tx, course_id)?;
    tx.commit()?;
    Ok(())
}
