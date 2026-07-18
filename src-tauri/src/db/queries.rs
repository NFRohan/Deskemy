//! All SQL statements. Every function takes `&Connection` (a `Transaction`
//! derefs to it, so these compose inside import transactions).

use crate::db::{new_id, now};
use crate::domain::{
    Attachment, Bookmark, BookmarkDetail, CourseDetail, CourseSummary, DayActivity, HistoryEntry,
    Lecture, LibraryStats, Section, SearchHit, SubtitleHit, TrackCourse, TrackDetail, TrackSummary,
};
use crate::error::{DeskemyError, Result};
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

/// (id, folder_path, root_id) for every course — used to set up filesystem
/// watches and to map a changed path back to its course.
pub fn all_course_folders(conn: &Connection) -> Result<Vec<(String, String, Option<String>)>> {
    let mut stmt = conn.prepare("SELECT id, folder_path, root_id FROM courses")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
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

/// A course's current folder path, if it exists.
pub fn course_folder(conn: &Connection, course_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT folder_path FROM courses WHERE id = ?1",
            params![course_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// One lecture's file path for a course (used to sanity-check a relocate target).
pub fn first_lecture_path(conn: &Connection, course_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT file_path FROM lectures WHERE course_id = ?1 LIMIT 1",
            params![course_id],
            |r| r.get::<_, String>(0),
        )
        .optional()?)
}

/// Repoint a course and its lecture/attachment file paths from `old_folder` to
/// `new_folder` (a moved or renamed course folder). Rewrites only the path
/// prefix and keeps every id, so progress, bookmarks, tags, and track membership
/// are preserved untouched.
pub fn relocate_course(
    conn: &Connection,
    course_id: &str,
    old_folder: &str,
    new_folder: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE lectures SET file_path = ?2 || substr(file_path, length(?3) + 1)
           WHERE course_id = ?1",
        params![course_id, new_folder, old_folder],
    )?;
    conn.execute(
        "UPDATE attachments SET file_path = ?2 || substr(file_path, length(?3) + 1)
           WHERE course_id = ?1",
        params![course_id, new_folder, old_folder],
    )?;
    conn.execute(
        "UPDATE courses SET folder_path = ?2, scan_status = 'Ready' WHERE id = ?1",
        params![course_id, new_folder],
    )?;
    Ok(())
}

pub fn delete_course(conn: &Connection, id: &str) -> Result<()> {
    // The FTS tables have no FK, so clear their rows explicitly.
    conn.execute("DELETE FROM search_index WHERE course_id = ?1", params![id])?;
    conn.execute("DELETE FROM subtitle_index WHERE course_id = ?1", params![id])?;
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

/// Set (or clear, with `None`) a course's thumbnail path.
pub fn set_thumbnail(conn: &Connection, id: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE courses SET thumbnail_path = ?2 WHERE id = ?1",
        params![id, path],
    )?;
    Ok(())
}

/// Set (or clear) a course's Continue-Watching resume frame.
pub fn set_resume_thumbnail(conn: &Connection, id: &str, path: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE courses SET resume_thumbnail_path = ?2 WHERE id = ?1",
        params![id, path],
    )?;
    Ok(())
}

pub fn tags_for_course(conn: &Connection, course_id: &str) -> Result<Vec<String>> {
    let mut stmt =
        conn.prepare("SELECT tag FROM course_tags WHERE course_id = ?1 ORDER BY tag")?;
    let rows = stmt
        .query_map(params![course_id], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Add a tag to a course (no-op if already present); returns the course's tags.
pub fn add_tag(conn: &Connection, course_id: &str, tag: &str) -> Result<Vec<String>> {
    let tag = tag.trim();
    if !tag.is_empty() {
        conn.execute(
            "INSERT OR IGNORE INTO course_tags (course_id, tag) VALUES (?1, ?2)",
            params![course_id, tag],
        )?;
    }
    tags_for_course(conn, course_id)
}

pub fn remove_tag(conn: &Connection, course_id: &str, tag: &str) -> Result<Vec<String>> {
    conn.execute(
        "DELETE FROM course_tags WHERE course_id = ?1 AND tag = ?2",
        params![course_id, tag],
    )?;
    tags_for_course(conn, course_id)
}

pub fn touch_opened(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE courses SET last_opened_at = ?2 WHERE id = ?1",
        params![id, now()],
    )?;
    Ok(())
}

/// (course_id, file_path) for every lecture — used to check for missing files.
pub fn all_lecture_files(conn: &Connection) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare("SELECT course_id, file_path FROM lectures")?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Toggle a course between Ready and Missing during reconciliation, without
/// disturbing Importing/Scanning/Error states.
pub fn set_missing(conn: &Connection, id: &str, missing: bool) -> Result<()> {
    if missing {
        conn.execute(
            "UPDATE courses SET scan_status='Missing'
              WHERE id=?1 AND scan_status IN ('Ready','Missing')",
            params![id],
        )?;
    } else {
        conn.execute(
            "UPDATE courses SET scan_status='Ready' WHERE id=?1 AND scan_status='Missing'",
            params![id],
        )?;
    }
    Ok(())
}

/// Every thumbnail path referenced by a course (course + resume frames).
pub fn all_thumbnail_paths(conn: &Connection) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT thumbnail_path FROM courses WHERE thumbnail_path IS NOT NULL
         UNION
         SELECT resume_thumbnail_path FROM courses WHERE resume_thumbnail_path IS NOT NULL",
    )?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
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
/// Per-lecture media metadata for a course: (file_path, size, mtime, duration,
/// container, video_codec, playable). Used to reuse metadata for unchanged
/// files on rescan instead of re-probing them.
#[allow(clippy::type_complexity)]
#[allow(clippy::type_complexity)]
pub fn course_lecture_media(
    conn: &Connection,
    course_id: &str,
) -> Result<
    Vec<(
        String,
        Option<i64>,
        Option<i64>,
        Option<f64>,
        Option<String>,
        Option<String>,
        bool,
        Option<String>,
    )>,
> {
    let mut stmt = conn.prepare(
        "SELECT file_path, file_size, mtime, duration, container, video_codec, playable, content_hash
           FROM lectures WHERE course_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get::<_, i64>(6)? != 0,
                r.get(7)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// (file_path, chapter idx, title, start_time) for a course's chapters.
pub fn course_chapters(
    conn: &Connection,
    course_id: &str,
) -> Result<Vec<(String, i64, Option<String>, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT l.file_path, ch.idx, ch.title, ch.start_time
           FROM chapters ch JOIN lectures l ON l.id = ch.lecture_id
          WHERE l.course_id = ?1
          ORDER BY ch.idx",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// --- Preserve user data across a re-import (rescan) ---

/// (is_favorite, last_opened_at, last_lecture_id, thumbnail_path,
/// resume_thumbnail_path) for a course.
#[allow(clippy::type_complexity)]
pub fn course_preserve(
    conn: &Connection,
    id: &str,
) -> Result<(bool, Option<i64>, Option<String>, Option<String>, Option<String>)> {
    Ok(conn.query_row(
        "SELECT is_favorite, last_opened_at, last_lecture_id, thumbnail_path, resume_thumbnail_path
           FROM courses WHERE id = ?1",
        params![id],
        |r| {
            Ok((
                r.get::<_, i64>(0)? != 0,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
            ))
        },
    )?)
}

pub fn lecture_file_path(conn: &Connection, lecture_id: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT file_path FROM lectures WHERE id = ?1",
            params![lecture_id],
            |r| r.get(0),
        )
        .optional()?)
}

/// (file_path, position_seconds, completed, last_watched_at) per progress row.
/// (file_path, content_hash, position_seconds, completed, last_watched_at) per
/// progress row — content_hash lets a renamed file keep its progress.
pub fn progress_with_files(
    conn: &Connection,
    course_id: &str,
) -> Result<Vec<(String, Option<String>, f64, bool, Option<i64>)>> {
    let mut stmt = conn.prepare(
        "SELECT l.file_path, l.content_hash, p.position_seconds, p.completed, p.last_watched_at
           FROM progress p JOIN lectures l ON l.id = p.lecture_id
          WHERE l.course_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get::<_, i64>(3)? != 0, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// (file_path, content_hash, position_seconds, label, created_at) per bookmark.
pub fn bookmarks_with_files(
    conn: &Connection,
    course_id: &str,
) -> Result<Vec<(String, Option<String>, f64, Option<String>, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT l.file_path, l.content_hash, b.position_seconds, b.label, b.created_at
           FROM bookmarks b JOIN lectures l ON l.id = b.lecture_id
          WHERE b.course_id = ?1",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Reapply preserved course-row fields after a re-import (thumbnail kept only
/// when the caller has one, so folder-detected thumbnails aren't clobbered).
pub fn restore_course_fields(
    conn: &Connection,
    id: &str,
    is_favorite: bool,
    last_opened_at: Option<i64>,
    thumbnail_path: Option<&str>,
    resume_thumbnail_path: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE courses SET is_favorite = ?2, last_opened_at = ?3,
             thumbnail_path = COALESCE(?4, thumbnail_path),
             resume_thumbnail_path = ?5
           WHERE id = ?1",
        params![id, is_favorite as i64, last_opened_at, thumbnail_path, resume_thumbnail_path],
    )?;
    Ok(())
}

pub fn set_last_lecture_id(conn: &Connection, course_id: &str, lecture_id: &str) -> Result<()> {
    conn.execute(
        "UPDATE courses SET last_lecture_id = ?2 WHERE id = ?1",
        params![course_id, lecture_id],
    )?;
    Ok(())
}

pub fn restore_progress(
    conn: &Connection,
    lecture_id: &str,
    position_seconds: f64,
    completed: bool,
    last_watched_at: Option<i64>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO progress (lecture_id, position_seconds, completed, last_watched_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![lecture_id, position_seconds, completed as i64, last_watched_at],
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn restore_bookmark(
    conn: &Connection,
    id: &str,
    lecture_id: &str,
    course_id: &str,
    position_seconds: f64,
    label: Option<&str>,
    created_at: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO bookmarks (id, lecture_id, course_id, position_seconds, label, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, lecture_id, course_id, position_seconds, label, created_at],
    )?;
    Ok(())
}

/// A course's resources (pdfs, archives, code, …), ordered by section then name.
pub fn list_course_attachments(conn: &Connection, course_id: &str) -> Result<Vec<Attachment>> {
    let mut stmt = conn.prepare(
        "SELECT a.id, a.name, a.file_path, a.kind, a.section_id, a.lecture_id
           FROM attachments a
           LEFT JOIN sections s ON s.id = a.section_id
          WHERE a.course_id = ?1
          ORDER BY COALESCE(s.position, -1), a.name",
    )?;
    let rows = stmt
        .query_map(params![course_id], |r| {
            Ok(Attachment {
                id: r.get(0)?,
                name: r.get(1)?,
                file_path: r.get(2)?,
                kind: r.get(3)?,
                section_id: r.get(4)?,
                lecture_id: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

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

/// Build a safe FTS5 MATCH expression from free-form user input: each token
/// becomes a quoted prefix term restricted to `column`. `None` when there are
/// no usable tokens (so we skip running MATCH on an empty/invalid expression).
fn fts_match_expr(input: &str, column: &str) -> Option<String> {
    let terms: Vec<String> = input
        .split_whitespace()
        .filter(|t| t.chars().any(char::is_alphanumeric))
        .map(|t| format!("\"{}\"*", t.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| format!("{column} : ({})", terms.join(" ")))
}

/// Full-text search across indexed courses/sections/lectures/attachments,
/// ranked by relevance.
pub fn search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SearchHit>> {
    let Some(expr) = fts_match_expr(query, "title") else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT s.kind, s.entity_id, s.course_id, c.title, s.title
           FROM search_index s
           JOIN courses c ON c.id = s.course_id
          WHERE search_index MATCH ?1
          ORDER BY rank
          LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![expr, limit], |r| {
            Ok(SearchHit {
                kind: r.get(0)?,
                entity_id: r.get(1)?,
                course_id: r.get(2)?,
                course_title: r.get(3)?,
                title: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// --- Daily activity telemetry ---

/// Add watched seconds to today's activity bucket.
pub fn add_watch_seconds(conn: &Connection, secs: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO daily_activity (day, watch_seconds) VALUES (date('now','localtime'), ?1)
         ON CONFLICT(day) DO UPDATE SET watch_seconds = watch_seconds + ?1",
        params![secs],
    )?;
    Ok(())
}

/// Record one lecture completion in today's activity bucket.
pub fn add_completion(conn: &Connection) -> Result<()> {
    conn.execute(
        "INSERT INTO daily_activity (day, lectures_completed) VALUES (date('now','localtime'), 1)
         ON CONFLICT(day) DO UPDATE SET lectures_completed = lectures_completed + 1",
        [],
    )?;
    Ok(())
}

/// All daily activity, oldest first: (julian_day_int, day 'YYYY-MM-DD',
/// watch_seconds, lectures_completed). The julian int makes streak/window math
/// straightforward.
pub fn daily_activity(conn: &Connection) -> Result<Vec<(i64, String, f64, i64)>> {
    let mut stmt = conn.prepare(
        "SELECT CAST(julianday(day) AS INTEGER), day, watch_seconds, lectures_completed
           FROM daily_activity ORDER BY 1",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Aggregate library + watch statistics. `daily_goal_minutes` is filled by the
/// caller from config.
pub fn stats(conn: &Connection) -> Result<LibraryStats> {
    use std::collections::HashMap;
    let one = |sql: &str| -> Result<i64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };
    let one_f = |sql: &str| -> Result<f64> { Ok(conn.query_row(sql, [], |r| r.get(0))?) };

    let courses_total = one("SELECT COUNT(*) FROM courses")?;
    let lectures_total = one("SELECT COUNT(*) FROM lectures")?;
    let lectures_completed = one("SELECT COUNT(*) FROM progress WHERE completed=1")?;
    let bookmarks_total = one("SELECT COUNT(*) FROM bookmarks")?;
    let library_seconds = one_f("SELECT COALESCE(SUM(duration),0) FROM lectures")?;
    let watched_seconds = one_f(
        "SELECT COALESCE(SUM(CASE WHEN p.completed=1 THEN COALESCE(l.duration,0)
                                  ELSE p.position_seconds END),0)
           FROM progress p JOIN lectures l ON l.id=p.lecture_id",
    )?;
    let courses_completed = one(
        "SELECT COUNT(*) FROM courses c
          WHERE c.lecture_count>0 AND c.lecture_count=(
            SELECT COUNT(*) FROM lectures l JOIN progress p ON p.lecture_id=l.id
             WHERE l.course_id=c.id AND p.completed=1)",
    )?;
    let started = one(
        "SELECT COUNT(DISTINCT l.course_id) FROM lectures l JOIN progress p ON p.lecture_id=l.id
          WHERE p.completed=1 OR p.position_seconds>0",
    )?;
    let courses_in_progress = (started - courses_completed).max(0);

    // --- Daily activity telemetry ---
    let today = one("SELECT CAST(julianday(date('now','localtime')) AS INTEGER)")?;
    let month_start =
        one("SELECT CAST(julianday(date('now','localtime','start of month')) AS INTEGER)")?;
    let rows = daily_activity(conn)?; // (jd, day, secs, completed)
    let by_jd: HashMap<i64, (f64, i64)> =
        rows.iter().map(|(jd, _, s, c)| (*jd, (*s, *c))).collect();

    let watch_seconds_today = by_jd.get(&today).map(|x| x.0).unwrap_or(0.0);
    let week: Vec<i64> = (today - 6..=today).collect();
    let watch_seconds_week = week.iter().filter_map(|jd| by_jd.get(jd)).map(|x| x.0).sum();
    let lectures_last_7 = week.iter().filter_map(|jd| by_jd.get(jd)).map(|x| x.1).sum();
    let active_days_month = by_jd
        .iter()
        .filter(|(jd, (s, _))| **jd >= month_start && *s > 0.0)
        .count() as i64;
    let best_day_seconds = by_jd.values().map(|x| x.0).fold(0.0, f64::max);

    // Streak: a day counts with >= 15 minutes watched.
    const THRESHOLD: f64 = 15.0 * 60.0;
    let qualifies = |jd: i64| by_jd.get(&jd).map(|x| x.0 >= THRESHOLD).unwrap_or(false);
    let mut current_streak = 0i64;
    let anchor = if qualifies(today) {
        Some(today)
    } else if qualifies(today - 1) {
        Some(today - 1)
    } else {
        None
    };
    if let Some(mut jd) = anchor {
        while qualifies(jd) {
            current_streak += 1;
            jd -= 1;
        }
    }
    let mut qual_days: Vec<i64> = by_jd
        .iter()
        .filter(|(_, (s, _))| *s >= THRESHOLD)
        .map(|(jd, _)| *jd)
        .collect();
    qual_days.sort_unstable();
    let (mut best_streak, mut run, mut prev) = (0i64, 0i64, i64::MIN);
    for jd in qual_days {
        run = if jd == prev + 1 { run + 1 } else { 1 };
        best_streak = best_streak.max(run);
        prev = jd;
    }

    // Most-focused course: the one worked on most in the last 7 days (most
    // lectures touched, tie: most recent). Falls back to the highest-completion
    // in-progress course when there's been no activity this week.
    let (mut focus_course_id, mut focus_course_title, mut focus_course_pct) = (None, None, 0i64);
    {
        let recent: Option<(String, String, i64, i64)> = conn
            .query_row(
                "SELECT l.course_id, c.title, c.lecture_count,
                        (SELECT COUNT(*) FROM lectures l2 JOIN progress p2 ON p2.lecture_id=l2.id
                          WHERE l2.course_id=l.course_id AND p2.completed=1)
                   FROM progress p
                   JOIN lectures l ON l.id = p.lecture_id
                   JOIN courses  c ON c.id = l.course_id
                  WHERE p.last_watched_at >= strftime('%s','now','-7 days')
                  GROUP BY l.course_id
                  ORDER BY COUNT(*) DESC, MAX(p.last_watched_at) DESC
                  LIMIT 1",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;

        let chosen = match recent {
            Some(x) => Some(x),
            None => {
                let mut stmt = conn.prepare(
                    "SELECT c.id, c.title, c.lecture_count, COALESCE(c.last_opened_at,0),
                            (SELECT COUNT(*) FROM lectures l JOIN progress p ON p.lecture_id=l.id
                              WHERE l.course_id=c.id AND p.completed=1) AS done
                       FROM courses c WHERE c.lecture_count > 0",
                )?;
                let cands = stmt
                    .query_map([], |r| {
                        Ok((
                            r.get::<_, String>(0)?,
                            r.get::<_, String>(1)?,
                            r.get::<_, i64>(2)?,
                            r.get::<_, i64>(3)?,
                            r.get::<_, i64>(4)?,
                        ))
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                let mut best: Option<(f64, i64, (String, String, i64, i64))> = None;
                for (id, title, count, opened, done) in cands {
                    if done > 0 && done < count {
                        let frac = done as f64 / count as f64;
                        let better = best
                            .as_ref()
                            .map(|(bf, bo, _)| frac > *bf || (frac == *bf && opened > *bo))
                            .unwrap_or(true);
                        if better {
                            best = Some((frac, opened, (id, title, count, done)));
                        }
                    }
                }
                best.map(|(_, _, t)| t)
            }
        };

        if let Some((id, title, count, done)) = chosen {
            focus_course_id = Some(id);
            focus_course_title = Some(title);
            focus_course_pct = if count > 0 {
                ((done as f64 / count as f64) * 100.0).round() as i64
            } else {
                0
            };
        }
    }

    let activity = rows
        .into_iter()
        .map(|(_, day, watch_seconds, lectures_completed)| DayActivity {
            day,
            watch_seconds,
            lectures_completed,
        })
        .collect();

    Ok(LibraryStats {
        courses_total,
        courses_completed,
        courses_in_progress,
        lectures_total,
        lectures_completed,
        library_seconds,
        watched_seconds,
        bookmarks_total,
        watch_seconds_today,
        watch_seconds_week,
        active_days_month,
        current_streak,
        best_streak,
        lectures_last_7,
        best_day_seconds,
        daily_goal_minutes: 30, // overwritten by the command from config
        focus_course_id,
        focus_course_title,
        focus_course_pct,
        activity,
    })
}

pub fn search_index_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))?)
}

// --- Subtitle full-text index ---

/// (lecture_id, course_id, file_path) for every sidecar subtitle.
pub fn all_subtitle_files(conn: &Connection) -> Result<Vec<(String, String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT s.lecture_id, l.course_id, s.file_path
           FROM subtitles s JOIN lectures l ON l.id = s.lecture_id",
    )?;
    let rows = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn clear_subtitle_index(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM subtitle_index", [])?;
    Ok(())
}

pub fn insert_subtitle_cue(
    conn: &Connection,
    lecture_id: &str,
    course_id: &str,
    start_ms: i64,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO subtitle_index (lecture_id, course_id, start_ms, text)
         VALUES (?1, ?2, ?3, ?4)",
        params![lecture_id, course_id, start_ms, text],
    )?;
    Ok(())
}

pub fn subtitle_index_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM subtitle_index", [], |r| r.get(0))?)
}

/// Full-text search over subtitle cues; returns snippet + jump timestamp.
pub fn subtitle_search(conn: &Connection, query: &str, limit: i64) -> Result<Vec<SubtitleHit>> {
    let Some(expr) = fts_match_expr(query, "text") else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT si.lecture_id, si.course_id, c.title, l.title, si.start_ms,
                snippet(subtitle_index, 3, '[', ']', '…', 10)
           FROM subtitle_index si
           JOIN lectures l ON l.id = si.lecture_id
           JOIN courses  c ON c.id = si.course_id
          WHERE subtitle_index MATCH ?1
          ORDER BY rank
          LIMIT ?2",
    )?;
    let rows = stmt
        .query_map(params![expr, limit], |r| {
            Ok(SubtitleHit {
                lecture_id: r.get(0)?,
                course_id: r.get(1)?,
                course_title: r.get(2)?,
                lecture_title: r.get(3)?,
                start_ms: r.get(4)?,
                snippet: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn course_count(conn: &Connection) -> Result<i64> {
    Ok(conn.query_row("SELECT COUNT(*) FROM courses", [], |r| r.get(0))?)
}

/// Rebuild the whole search index from the base tables (used as a startup
/// safety net for libraries imported before a given entity was indexed).
pub fn rebuild_search_index(conn: &Connection) -> Result<()> {
    conn.execute("DELETE FROM search_index", [])?;
    conn.execute(
        "INSERT INTO search_index (kind, entity_id, course_id, title)
         SELECT 'course', id, id, title FROM courses",
        [],
    )?;
    conn.execute(
        "INSERT INTO search_index (kind, entity_id, course_id, title)
         SELECT 'section', id, course_id, title FROM sections",
        [],
    )?;
    conn.execute(
        "INSERT INTO search_index (kind, entity_id, course_id, title)
         SELECT 'lecture', id, course_id, title FROM lectures",
        [],
    )?;
    conn.execute(
        "INSERT INTO search_index (kind, entity_id, course_id, title)
         SELECT 'attachment', id, course_id, name FROM attachments",
        [],
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
                  WHERE l.course_id = c.id AND p.completed = 1) AS completed_count,
                c.resume_thumbnail_path,
                c.last_lecture_id,
                (SELECT l2.title FROM lectures l2 WHERE l2.id = c.last_lecture_id) AS last_lecture_title
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
                resume_thumbnail_path: r.get(10)?,
                last_lecture_id: r.get(11)?,
                last_lecture_title: r.get(12)?,
                tags: Vec::new(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    // Attach tags in one pass.
    let mut tstmt = conn.prepare("SELECT course_id, tag FROM course_tags ORDER BY tag")?;
    let mut by_course: HashMap<String, Vec<String>> = HashMap::new();
    let pairs = tstmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (cid, tag) in pairs {
        by_course.entry(cid).or_default().push(tag);
    }
    let mut rows = rows;
    for c in &mut rows {
        if let Some(tags) = by_course.remove(&c.id) {
            c.tags = tags;
        }
    }
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
/// `lectures.position` is per-section, so order by section then lecture.
pub fn list_course_playlist(conn: &Connection, course_id: &str) -> Result<Vec<(String, String)>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.file_path
           FROM lectures l JOIN sections s ON s.id = l.section_id
          WHERE l.course_id = ?1 AND l.playable = 1
          ORDER BY s.position, l.position",
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

// --- Per-course playback preferences ---

/// (speed, subtitle_id, subtitles_on, audio_id) for a course, if recorded.
#[allow(clippy::type_complexity)]
pub fn get_course_prefs(
    conn: &Connection,
    course_id: &str,
) -> Result<Option<(Option<f64>, Option<i64>, bool, Option<i64>)>> {
    Ok(conn
        .query_row(
            "SELECT speed, subtitle_id, subtitles_on, audio_id FROM course_prefs WHERE course_id = ?1",
            params![course_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get::<_, i64>(2)? != 0, r.get(3)?)),
        )
        .optional()?)
}

pub fn set_pref_speed(conn: &Connection, course_id: &str, speed: f64) -> Result<()> {
    conn.execute(
        "INSERT INTO course_prefs (course_id, speed) VALUES (?1, ?2)
         ON CONFLICT(course_id) DO UPDATE SET speed = ?2",
        params![course_id, speed],
    )?;
    Ok(())
}

/// Remember the subtitle selection: `Some(id)` records the track and marks subs
/// on; `None` just marks subs off (keeps the last selected track).
pub fn set_pref_subtitle(conn: &Connection, course_id: &str, sid: Option<i64>) -> Result<()> {
    match sid {
        Some(id) => conn.execute(
            "INSERT INTO course_prefs (course_id, subtitle_id, subtitles_on) VALUES (?1, ?2, 1)
             ON CONFLICT(course_id) DO UPDATE SET subtitle_id = ?2, subtitles_on = 1",
            params![course_id, id],
        )?,
        None => conn.execute(
            "INSERT INTO course_prefs (course_id, subtitles_on) VALUES (?1, 0)
             ON CONFLICT(course_id) DO UPDATE SET subtitles_on = 0",
            params![course_id],
        )?,
    };
    Ok(())
}

pub fn set_pref_audio(conn: &Connection, course_id: &str, aid: Option<i64>) -> Result<()> {
    conn.execute(
        "INSERT INTO course_prefs (course_id, audio_id) VALUES (?1, ?2)
         ON CONFLICT(course_id) DO UPDATE SET audio_id = ?2",
        params![course_id, aid],
    )?;
    Ok(())
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

/// Manually set a lecture's completed flag (allows un-marking, unlike save_progress).
pub fn set_completed(conn: &Connection, lecture_id: &str, completed: bool) -> Result<()> {
    conn.execute(
        "INSERT INTO progress (lecture_id, position_seconds, completed, last_watched_at)
         VALUES (?1, 0, ?2, ?3)
         ON CONFLICT(lecture_id) DO UPDATE SET
           completed = excluded.completed,
           last_watched_at = excluded.last_watched_at",
        params![lecture_id, completed as i64, now()],
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

// ---------------------------------------------------------------------------
// Bookmarks
// ---------------------------------------------------------------------------

/// Insert a bookmark at `position_seconds` in a lecture. `course_id` is derived
/// from the lecture row so the caller only needs the lecture id.
pub fn add_bookmark(
    conn: &Connection,
    lecture_id: &str,
    position_seconds: f64,
    label: Option<&str>,
) -> Result<Bookmark> {
    let id = new_id();
    let created_at = now();
    let n = conn.execute(
        "INSERT INTO bookmarks (id, lecture_id, course_id, position_seconds, label, created_at)
         SELECT ?1, ?2, l.course_id, ?3, ?4, ?5 FROM lectures l WHERE l.id = ?2",
        params![id, lecture_id, position_seconds, label, created_at],
    )?;
    if n == 0 {
        return Err(DeskemyError::NotFound(format!("lecture {lecture_id}")));
    }
    Ok(Bookmark {
        id,
        lecture_id: lecture_id.to_string(),
        position_seconds,
        label: label.map(str::to_string),
        created_at,
    })
}

pub fn list_bookmarks(conn: &Connection, lecture_id: &str) -> Result<Vec<Bookmark>> {
    let mut stmt = conn.prepare(
        "SELECT id, lecture_id, position_seconds, label, created_at
           FROM bookmarks WHERE lecture_id = ?1
          ORDER BY position_seconds",
    )?;
    let rows = stmt
        .query_map(params![lecture_id], |r| {
            Ok(Bookmark {
                id: r.get(0)?,
                lecture_id: r.get(1)?,
                position_seconds: r.get(2)?,
                label: r.get(3)?,
                created_at: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn delete_bookmark(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM bookmarks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Every bookmark with lecture/course context, grouped-friendly (ordered by
/// course, then lecture order, then time) for the global bookmarks page.
pub fn list_all_bookmarks(conn: &Connection) -> Result<Vec<BookmarkDetail>> {
    let mut stmt = conn.prepare(
        "SELECT b.id, b.lecture_id, l.title, s.title, b.course_id, c.title,
                b.position_seconds, b.label, b.created_at
           FROM bookmarks b
           JOIN lectures l ON l.id = b.lecture_id
           JOIN sections s ON s.id = l.section_id
           JOIN courses  c ON c.id = b.course_id
          ORDER BY c.title, s.position, l.position, b.position_seconds",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(BookmarkDetail {
                id: r.get(0)?,
                lecture_id: r.get(1)?,
                lecture_title: r.get(2)?,
                section_title: r.get(3)?,
                course_id: r.get(4)?,
                course_title: r.get(5)?,
                position_seconds: r.get(6)?,
                label: r.get(7)?,
                created_at: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// Recently-watched lectures (one row per lecture that has a progress entry),
/// newest first, for the playback-history page. Capped to keep the payload
/// bounded; the frontend groups by day.
pub fn list_history(conn: &Connection, limit: i64) -> Result<Vec<HistoryEntry>> {
    let mut stmt = conn.prepare(
        "SELECT l.id, l.title, s.title, c.id, c.title,
                p.position_seconds, l.duration, p.completed, p.last_watched_at
           FROM progress p
           JOIN lectures l ON l.id = p.lecture_id
           JOIN sections s ON s.id = l.section_id
           JOIN courses  c ON c.id = l.course_id
          WHERE p.last_watched_at IS NOT NULL
          ORDER BY p.last_watched_at DESC
          LIMIT ?1",
    )?;
    let rows = stmt
        .query_map(params![limit], |r| {
            Ok(HistoryEntry {
                lecture_id: r.get(0)?,
                lecture_title: r.get(1)?,
                section_title: r.get(2)?,
                course_id: r.get(3)?,
                course_title: r.get(4)?,
                position_seconds: r.get(5)?,
                duration: r.get(6)?,
                completed: r.get::<_, i64>(7)? != 0,
                last_watched_at: r.get(8)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

// ---------------------------------------------------------------------------
// Career tracks — ordered groupings of courses (organizational layer only).
// ---------------------------------------------------------------------------

/// All tracks with aggregate completion (over every lecture in their courses).
pub fn list_tracks(conn: &Connection) -> Result<Vec<TrackSummary>> {
    let mut stmt = conn.prepare(
        "SELECT t.id, t.name, t.description,
                COUNT(tc.course_id) AS course_count,
                COALESCE(SUM(c.lecture_count), 0) AS total_lectures,
                COALESCE(SUM(
                    (SELECT COUNT(*) FROM lectures l JOIN progress p ON p.lecture_id = l.id
                      WHERE l.course_id = c.id AND p.completed = 1)
                ), 0) AS completed_lectures
           FROM tracks t
           LEFT JOIN track_courses tc ON tc.track_id = t.id
           LEFT JOIN courses c ON c.id = tc.course_id
          GROUP BY t.id
          ORDER BY t.position, t.created_at",
    )?;
    let rows = stmt
        .query_map([], |r| {
            Ok(TrackSummary {
                id: r.get(0)?,
                name: r.get(1)?,
                description: r.get(2)?,
                course_count: r.get(3)?,
                total_lectures: r.get(4)?,
                completed_lectures: r.get(5)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// A track plus its ordered courses (each with completion), or None if missing.
pub fn get_track(conn: &Connection, id: &str) -> Result<Option<TrackDetail>> {
    let base = conn
        .query_row(
            "SELECT id, name, description FROM tracks WHERE id = ?1",
            params![id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?;
    let Some((tid, name, description)) = base else {
        return Ok(None);
    };

    let mut stmt = conn.prepare(
        "SELECT c.id, c.title, c.thumbnail_path, c.lecture_count,
                (SELECT COUNT(*) FROM lectures l JOIN progress p ON p.lecture_id = l.id
                  WHERE l.course_id = c.id AND p.completed = 1) AS completed_lectures
           FROM track_courses tc
           JOIN courses c ON c.id = tc.course_id
          WHERE tc.track_id = ?1
          ORDER BY tc.position",
    )?;
    let courses = stmt
        .query_map(params![tid], |r| {
            Ok(TrackCourse {
                id: r.get(0)?,
                title: r.get(1)?,
                thumbnail_path: r.get(2)?,
                lecture_count: r.get(3)?,
                completed_lectures: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    Ok(Some(TrackDetail {
        id: tid,
        name,
        description,
        courses,
    }))
}

/// Create a track (appended to the end); returns its id.
pub fn create_track(conn: &Connection, name: &str, description: Option<&str>) -> Result<String> {
    let id = new_id();
    let position: i64 =
        conn.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM tracks", [], |r| r.get(0))?;
    conn.execute(
        "INSERT INTO tracks (id, name, description, position, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5)",
        params![id, name, description, position, now()],
    )?;
    Ok(id)
}

/// Rename / re-describe a track.
pub fn update_track(conn: &Connection, id: &str, name: &str, description: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE tracks SET name = ?2, description = ?3 WHERE id = ?1",
        params![id, name, description],
    )?;
    Ok(())
}

/// Delete a track (its course memberships cascade; the courses are untouched).
pub fn delete_track(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM tracks WHERE id = ?1", params![id])?;
    Ok(())
}

/// Append a course to a track (no-op if already a member).
pub fn add_course_to_track(conn: &Connection, track_id: &str, course_id: &str) -> Result<()> {
    let position: i64 = conn.query_row(
        "SELECT COALESCE(MAX(position), -1) + 1 FROM track_courses WHERE track_id = ?1",
        params![track_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO track_courses (track_id, course_id, position)
         VALUES (?1, ?2, ?3)",
        params![track_id, course_id, position],
    )?;
    Ok(())
}

/// Remove a course from a track.
pub fn remove_course_from_track(conn: &Connection, track_id: &str, course_id: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM track_courses WHERE track_id = ?1 AND course_id = ?2",
        params![track_id, course_id],
    )?;
    Ok(())
}

/// Rewrite the course order within a track from the given full ordering. Ids not
/// currently in the track are ignored; missing ones keep their prior position.
pub fn reorder_track_courses(conn: &Connection, track_id: &str, course_ids: &[String]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    for (i, course_id) in course_ids.iter().enumerate() {
        tx.execute(
            "UPDATE track_courses SET position = ?3 WHERE track_id = ?1 AND course_id = ?2",
            params![track_id, course_id, i as i64],
        )?;
    }
    tx.commit()?;
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
