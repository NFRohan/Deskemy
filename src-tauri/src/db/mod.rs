//! Persistence layer. Owns the schema, migrations, and every SQL statement.
//! Maps rows into `crate::domain` types so no rusqlite detail leaks upward.

pub mod queries;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current schema version. Bump + add a migration arm when the schema changes.
const SCHEMA_VERSION: i64 = 6;

const SCHEMA_V1: &str = r#"
CREATE TABLE library_roots (
    id       TEXT PRIMARY KEY,
    path     TEXT NOT NULL UNIQUE,
    added_at INTEGER NOT NULL
);

CREATE TABLE courses (
    id             TEXT PRIMARY KEY,
    root_id        TEXT REFERENCES library_roots(id) ON DELETE SET NULL,
    title          TEXT NOT NULL,
    folder_path    TEXT NOT NULL UNIQUE,
    thumbnail_path TEXT,
    total_duration REAL,
    lecture_count  INTEGER NOT NULL DEFAULT 0,
    is_favorite    INTEGER NOT NULL DEFAULT 0,
    scan_status    TEXT NOT NULL DEFAULT 'Ready',
    last_opened_at INTEGER,
    last_lecture_id TEXT,
    imported_at    INTEGER NOT NULL,
    last_scanned_at INTEGER
);

CREATE TABLE sections (
    id          TEXT PRIMARY KEY,
    course_id   TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    title       TEXT NOT NULL,
    position    INTEGER NOT NULL,
    folder_path TEXT
);
CREATE INDEX idx_sections_course ON sections(course_id);

CREATE TABLE lectures (
    id           TEXT PRIMARY KEY,
    course_id    TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    section_id   TEXT NOT NULL REFERENCES sections(id) ON DELETE CASCADE,
    title        TEXT NOT NULL,
    file_path    TEXT NOT NULL,
    position     INTEGER NOT NULL,
    duration     REAL,
    container    TEXT,
    video_codec  TEXT,
    playable     INTEGER NOT NULL DEFAULT 1,
    file_size    INTEGER,
    mtime        INTEGER,
    content_hash TEXT
);
CREATE INDEX idx_lectures_course ON lectures(course_id);
CREATE INDEX idx_lectures_section ON lectures(section_id);
CREATE INDEX idx_lectures_hash ON lectures(content_hash);

CREATE TABLE chapters (
    id         TEXT PRIMARY KEY,
    lecture_id TEXT NOT NULL REFERENCES lectures(id) ON DELETE CASCADE,
    idx        INTEGER NOT NULL,
    title      TEXT,
    start_time REAL NOT NULL
);
CREATE INDEX idx_chapters_lecture ON chapters(lecture_id);

CREATE TABLE subtitles (
    id         TEXT PRIMARY KEY,
    lecture_id TEXT NOT NULL REFERENCES lectures(id) ON DELETE CASCADE,
    lang       TEXT,
    label      TEXT,
    file_path  TEXT NOT NULL
);
CREATE INDEX idx_subtitles_lecture ON subtitles(lecture_id);

CREATE TABLE attachments (
    id         TEXT PRIMARY KEY,
    course_id  TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    section_id TEXT REFERENCES sections(id) ON DELETE CASCADE,
    lecture_id TEXT REFERENCES lectures(id) ON DELETE CASCADE,
    name       TEXT NOT NULL,
    file_path  TEXT NOT NULL,
    kind       TEXT
);
CREATE INDEX idx_attachments_course ON attachments(course_id);
CREATE INDEX idx_attachments_lecture ON attachments(lecture_id);

CREATE TABLE progress (
    lecture_id       TEXT PRIMARY KEY REFERENCES lectures(id) ON DELETE CASCADE,
    position_seconds REAL NOT NULL DEFAULT 0,
    completed        INTEGER NOT NULL DEFAULT 0,
    last_watched_at  INTEGER
);

CREATE TABLE bookmarks (
    id               TEXT PRIMARY KEY,
    lecture_id       TEXT NOT NULL REFERENCES lectures(id) ON DELETE CASCADE,
    course_id        TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
    position_seconds REAL NOT NULL,
    label            TEXT,
    created_at       INTEGER NOT NULL
);
CREATE INDEX idx_bookmarks_lecture ON bookmarks(lecture_id);

CREATE VIRTUAL TABLE search_index USING fts5(
    kind,
    entity_id UNINDEXED,
    course_id UNINDEXED,
    title,
    tokenize = 'unicode61'
);
"#;

/// Unix seconds now.
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// A fresh UUIDv7 as a string (time-ordered, index-friendly).
pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// Open a connection at `path`, apply pragmas, and run migrations.
pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let conn = Connection::open(path)?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

/// Open an in-memory database (used by tests).
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    configure(&conn)?;
    migrate(&conn)?;
    Ok(conn)
}

fn configure(conn: &Connection) -> Result<()> {
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    Ok(())
}

fn migrate(conn: &Connection) -> Result<()> {
    let version: i64 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    // Run every step + the version bump in one transaction so an interrupted
    // upgrade rolls back cleanly instead of leaving a half-migrated (and
    // possibly unopenable) database. DDL and user_version are transactional.
    let tx = conn.unchecked_transaction()?;
    if version < 1 {
        tx.execute_batch(SCHEMA_V1)?;
    }
    if version < 2 {
        // A frame grabbed from the player at the resume point, shown on the
        // library's Continue Watching entry.
        tx.execute_batch("ALTER TABLE courses ADD COLUMN resume_thumbnail_path TEXT;")?;
    }
    if version < 3 {
        tx.execute_batch(
            "CREATE TABLE course_tags (
                 course_id TEXT NOT NULL REFERENCES courses(id) ON DELETE CASCADE,
                 tag       TEXT NOT NULL,
                 PRIMARY KEY (course_id, tag)
             );
             CREATE INDEX idx_course_tags_tag ON course_tags(tag);",
        )?;
    }
    if version < 4 {
        // Full-text index over sidecar subtitle text, one row per cue.
        tx.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS subtitle_index USING fts5(
                 lecture_id UNINDEXED,
                 course_id  UNINDEXED,
                 start_ms   UNINDEXED,
                 text,
                 tokenize = 'unicode61'
             );",
        )?;
    }
    if version < 5 {
        // Per-day watch telemetry for the stats page (heatmap, streaks, etc.).
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS daily_activity (
                 day                TEXT PRIMARY KEY,
                 watch_seconds      REAL NOT NULL DEFAULT 0,
                 lectures_completed INTEGER NOT NULL DEFAULT 0
             );",
        )?;
    }
    if version < 6 {
        // Per-course playback preferences (speed / subtitle / audio).
        tx.execute_batch(
            "CREATE TABLE IF NOT EXISTS course_prefs (
                 course_id    TEXT PRIMARY KEY REFERENCES courses(id) ON DELETE CASCADE,
                 speed        REAL,
                 subtitle_id  INTEGER,
                 subtitles_on INTEGER NOT NULL DEFAULT 0,
                 audio_id     INTEGER
             );",
        )?;
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    tx.commit()?;
    Ok(())
}
