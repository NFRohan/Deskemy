//! Persistence layer. Owns the schema, migrations, and every SQL statement.
//! Maps rows into `crate::domain` types so no rusqlite detail leaks upward.

pub mod queries;

use crate::error::Result;
use rusqlite::Connection;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Current schema version. Bump + add a migration arm when the schema changes.
const SCHEMA_VERSION: i64 = 1;

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
    if version < 1 {
        conn.execute_batch(SCHEMA_V1)?;
    }
    // future: if version < 2 { ... }
    if version != SCHEMA_VERSION {
        conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    }
    Ok(())
}
