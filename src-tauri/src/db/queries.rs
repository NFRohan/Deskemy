//! All SQL statements. Every function takes `&Connection` (a `Transaction`
//! derefs to it, so these compose inside import transactions).

use crate::db::{new_id, now};
use crate::domain::{CourseDetail, CourseSummary, Lecture, Section};
use crate::error::Result;
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Library roots
// ---------------------------------------------------------------------------

/// Insert a root if new; return its id (existing or newly created).
pub fn add_library_root(conn: &Connection, path: &str) -> Result<String> {
    if let Some(id) = conn
        .query_row(
            "SELECT id FROM library_roots WHERE path = ?1",
            params![path],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let id = new_id();
    conn.execute(
        "INSERT INTO library_roots (id, path, added_at) VALUES (?1, ?2, ?3)",
        params![id, path, now()],
    )?;
    Ok(id)
}

pub fn list_library_roots(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT id, path FROM library_roots ORDER BY added_at")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn remove_library_root(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM library_roots WHERE id = ?1", params![id])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Courses
// ---------------------------------------------------------------------------

pub fn find_course_by_path(conn: &Connection, folder_path: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT id FROM courses WHERE folder_path = ?1",
            params![folder_path],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

pub fn delete_course(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM search_index WHERE course_id = ?1", params![id])?;
    conn.execute("DELETE FROM courses WHERE id = ?1", params![id])?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_course(
    conn: &Connection,
    id: &str,
    root_id: Option<&str>,
    title: &str,
    folder_path: &str,
    scan_status: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO courses (id, root_id, title, folder_path, scan_status, imported_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, root_id, title, folder_path, scan_status, now()],
    )?;
    Ok(())
}

pub fn update_course_stats(
    conn: &Connection,
    id: &str,
    lecture_count: i64,
    total_duration: Option<f64>,
    thumbnail_path: Option<&str>,
    scan_status: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE courses
         SET lecture_count = ?2, total_duration = ?3, thumbnail_path = ?4,
             scan_status = ?5, last_scanned_at = ?6
         WHERE id = ?1",
        params![id, lecture_count, total_duration, thumbnail_path, scan_status, now()],
    )?;
    Ok(())
}

pub fn set_scan_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE courses SET scan_status = ?2 WHERE id = ?1",
        params![id, status],
    )?;
    Ok(())
}

pub fn set_favorite(conn: &Connection, id: &str, favorite: bool) -> Result<()> {
    conn.execute(
        "UPDATE courses SET is_favorite = ?2 WHERE id = ?1",
        params![id, favorite as i64],
    )?;
    Ok(())
}

pub fn touch_opened(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE courses SET last_opened_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Sections / lectures / associated rows
// ---------------------------------------------------------------------------

pub fn insert_section(
    conn: &Connection,
    id: &str,
    course_id: &str,
    title: &str,
    position: i64,
    folder_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO sections (id, course_id, title, position, folder_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, course_id, title, position, folder_path],
    )?;
    Ok(())
}

/// Fields for inserting one lecture row.
pub struct LectureInsert<'a> {
    pub id: &'a str,
    pub course_id: &'a str,
    pub section_id: &'a str,
    pub title: &'a str,
    pub file_path: &'a str,
    pub position: i64,
    pub duration: Option<f64>,
    pub container: Option<&'a str>,
    pub video_codec: Option<&'a str>,
    pub playable: bool,
    pub file_size: Option<i64>,
    pub mtime: Option<i64>,
    pub content_hash: Option<&'a str>,
}

pub fn insert_lecture(conn: &Connection, l: &LectureInsert) -> Result<()> {
    conn.execute(
        "INSERT INTO lectures
         (id, course_id, section_id, title, file_path, position, duration,
          container, video_codec, playable, file_size, mtime, content_hash)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        params![
            l.id, l.course_id, l.section_id, l.title, l.file_path, l.position,
            l.duration, l.container, l.video_codec, l.playable as i64,
            l.file_size, l.mtime, l.content_hash
        ],
    )?;
    Ok(())
}

pub fn insert_subtitle(
    conn: &Connection,
    id: &str,
    lecture_id: &str,
    lang: Option<&str>,
    label: Option<&str>,
    file_path: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO subtitles (id, lecture_id, lang, label, file_path)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, lecture_id, lang, label, file_path],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn insert_attachment(
    conn: &Connection,
    id: &str,
    course_id: &str,
    section_id: Option<&str>,
    lecture_id: Option<&str>,
    name: &str,
    file_path: &str,
    kind: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO attachments (id, course_id, section_id, lecture_id, name, file_path, kind)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![id, course_id, section_id, lecture_id, name, file_path, kind],
    )?;
    Ok(())
}

pub fn insert_chapter(
    conn: &Connection,
    id: &str,
    lecture_id: &str,
    idx: i64,
    title: Option<&str>,
    start_time: f64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO chapters (id, lecture_id, idx, title, start_time)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, lecture_id, idx, title, start_time],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Full-text search index
// ---------------------------------------------------------------------------

pub fn fts_insert(
    conn: &Connection,
    kind: &str,
    entity_id: &str,
    course_id: &str,
    title: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO search_index (kind, entity_id, course_id, title)
         VALUES (?1, ?2, ?3, ?4)",
        params![kind, entity_id, course_id, title],
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Reads for the UI
// ---------------------------------------------------------------------------

pub fn list_course_summaries(conn: &Connection) -> Result<Vec<CourseSummary>> {
    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.folder_path, c.thumbnail_path, c.lecture_count,
                c.total_duration, c.is_favorite, c.scan_status, c.last_opened_at,
                (SELECT COUNT(*) FROM lectures l
                   JOIN progress p ON p.lecture_id = l.id
                  WHERE l.course_id = c.id AND p.completed = 1) AS completed_count
           FROM courses c
          ORDER BY COALESCE(c.last_opened_at, 0) DESC, c.imported_at DESC",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(CourseSummary {
                id: r.get(0)?,
                title: r.get(1)?,
                folder_path: r.get(2)?,
                thumbnail_path: r.get(3)?,
                lecture_count: r.get(4)?,
                total_duration: r.get(5)?,
                is_favorite: r.get::<_, i64>(6)? != 0,
                scan_status: r.get(7)?,
                last_opened_at: r.get(8)?,
                completed_count: r.get(9)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Playback: lecture lookup, playlist, progress
// ---------------------------------------------------------------------------

/// (file_path, course_id, duration) for a lecture.
pub fn get_lecture_playback(
    conn: &Connection,
    lecture_id: &str,
) -> Result<Option<(String, String, Option<f64>)>> {
    Ok(conn
        .query_row(
            "SELECT file_path, course_id, duration FROM lectures WHERE id = ?1",
            params![lecture_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?)
}

/// Ordered (id, file_path) of playable lectures in a course — the playlist.
pub fn list_course_playlist(conn: &Connection, course_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT id, file_path FROM lectures
          WHERE course_id = ?1 AND playable = 1
          ORDER BY position",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// (lecture_title, course_id, course_title, section_title) for the watch header.
pub fn get_lecture_view(
    conn: &Connection,
    lecture_id: &str,
) -> Result<Option<(String, String, String, String)>> {
    Ok(conn
        .query_row(
            "SELECT l.title, l.course_id, c.title, s.title
               FROM lectures l
               JOIN courses c ON c.id = l.course_id
               JOIN sections s ON s.id = l.section_id
              WHERE l.id = ?1",
            params![lecture_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?)
}

pub fn get_progress(conn: &Connection, lecture_id: &str) -> Result<(f64, bool)> {
    Ok(conn
        .query_row(
            "SELECT position_seconds, completed FROM progress WHERE lecture_id = ?1",
            params![lecture_id],
            |r| Ok((r.get::<_, f64>(0)?, r.get::<_, i64>(1)? != 0)),
        )
        .optional()?
        .unwrap_or((0.0, false)))
}

pub fn save_progress(
    conn: &Connection,
    lecture_id: &str,
    position_seconds: f64,
    completed: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO progress (lecture_id, position_seconds, completed, last_watched_at)
         VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(lecture_id) DO UPDATE SET
           position_seconds = excluded.position_seconds,
           completed = MAX(progress.completed, excluded.completed),
           last_watched_at = excluded.last_watched_at",
        params![lecture_id, position_seconds, completed as i64, now()],
    )?;
    Ok(())
}

pub fn set_last_lecture(conn: &Connection, course_id: &str, lecture_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE courses SET last_lecture_id = ?2, last_opened_at = ?3 WHERE id = ?1",
        params![course_id, lecture_id, now()],
    )?;
    Ok(())
}

pub fn get_course_detail(conn: &Connection, id: &str) -> Result<Option<CourseDetail>> {
    let base = conn
        .query_row(
            "SELECT id, title, folder_path, thumbnail_path, total_duration,
                    is_favorite, scan_status, last_opened_at, last_lecture_id
               FROM courses WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, Option<String>>(3)?,
                    r.get::<_, Option<f64>>(4)?,
                    r.get::<_, i64>(5)? != 0,
                    r.get::<_, String>(6)?,
                    r.get::<_, Option<i64>>(7)?,
                    r.get::<_, Option<String>>(8)?,
                ))
            },
        )
        .optional()?;

    let Some((
        cid,
        title,
        folder_path,
        thumbnail_path,
        total_duration,
        is_favorite,
        scan_status,
        last_opened_at,
        last_lecture_id,
    )) = base
    else {
        return Ok(None);
    };

    // All lectures for the course, grouped by section in Rust.
    let mut lstmt = conn.prepare(
        "SELECT l.id, l.section_id, l.title, l.file_path, l.position, l.duration,
                l.container, l.video_codec, l.playable,
                COALESCE(p.position_seconds, 0), COALESCE(p.completed, 0)
           FROM lectures l
           LEFT JOIN progress p ON p.lecture_id = l.id
          WHERE l.course_id = ?1
          ORDER BY l.position",
    )?;
    let lectures = lstmt
        .query_map(params![cid], |r| {
            Ok(Lecture {
                id: r.get(0)?,
                section_id: r.get(1)?,
                title: r.get(2)?,
                file_path: r.get(3)?,
                position: r.get(4)?,
                duration: r.get(5)?,
                container: r.get(6)?,
                video_codec: r.get(7)?,
                playable: r.get::<_, i64>(8)? != 0,
                position_seconds: r.get(9)?,
                completed: r.get::<_, i64>(10)? != 0,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    let mut by_section: HashMap<String, Vec<Lecture>> = HashMap::new();
    for lec in lectures {
        by_section.entry(lec.section_id.clone()).or_default().push(lec);
    }

    let mut sstmt = conn.prepare(
        "SELECT id, title, position FROM sections WHERE course_id = ?1 ORDER BY position",
    )?;
    let sections = sstmt
        .query_map(params![cid], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?
        .into_iter()
        .map(|(sid, stitle, pos)| {
            let lectures = by_section.remove(&sid).unwrap_or_default();
            Section {
                id: sid,
                title: stitle,
                position: pos,
                lectures,
            }
        })
        .collect();

    Ok(Some(CourseDetail {
        id: cid,
        title,
        folder_path,
        thumbnail_path,
        total_duration,
        is_favorite,
        scan_status,
        last_opened_at,
        last_lecture_id,
        sections,
    }))
}
