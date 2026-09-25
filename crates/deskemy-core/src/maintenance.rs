//! Library maintenance and storage upkeep — the Settings page's actions,
//! shared by both frontends.

use crate::db::queries;
use crate::domain::StorageStats;
use crate::error::Result;
use crate::paths::DB_FILE;
use rusqlite::Connection;
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize)]
pub struct ReconcileReport {
    pub courses_checked: i64,
    pub courses_missing: i64,
    pub files_missing: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct GcReport {
    pub removed: i64,
    pub freed_bytes: i64,
}

fn lock(db: &Mutex<Connection>) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
}

/// Check every lecture's file on disk and flag courses with missing files as
/// Missing (or clear the flag when the files are back). The disk checks run
/// without the database lock.
pub fn reconcile(db: &Mutex<Connection>) -> Result<ReconcileReport> {
    let entries = queries::all_lecture_files(&lock(db))?;
    let mut total: HashMap<String, i64> = HashMap::new();
    let mut missing: HashMap<String, i64> = HashMap::new();
    let mut files_missing = 0i64;
    for (course, path) in &entries {
        *total.entry(course.clone()).or_default() += 1;
        if !Path::new(path).exists() {
            *missing.entry(course.clone()).or_default() += 1;
            files_missing += 1;
        }
    }

    let conn = lock(db);
    let mut courses_missing = 0i64;
    for course in total.keys() {
        let is_missing = missing.get(course).copied().unwrap_or(0) > 0;
        queries::set_missing(&conn, course, is_missing)?;
        if is_missing {
            courses_missing += 1;
        }
    }
    Ok(ReconcileReport { courses_checked: total.len() as i64, courses_missing, files_missing })
}

/// Rebuild the search index from the base tables; returns its row count.
pub fn reindex_search(conn: &Connection) -> Result<i64> {
    queries::rebuild_search_index(conn)?;
    queries::search_index_count(conn)
}

/// (Re)build the subtitle text index by parsing every sidecar subtitle file;
/// returns the number of cues indexed. Files are read without the lock.
pub fn reindex_subtitles(db: &Mutex<Connection>) -> Result<i64> {
    let files = queries::all_subtitle_files(&lock(db))?;
    let parsed: Vec<(String, String, Vec<(i64, String)>)> = files
        .into_iter()
        .filter_map(|(lecture, course, path)| {
            // Missing or unreadable subtitle files are skipped.
            let bytes = std::fs::read(&path).ok()?;
            Some((lecture, course, crate::subtitles::parse(&String::from_utf8_lossy(&bytes))))
        })
        .collect();

    let mut conn = lock(db);
    let tx = conn.transaction()?;
    queries::clear_subtitle_index(&tx)?;
    let mut total = 0i64;
    for (lecture, course, cues) in parsed {
        for (start_ms, text) in cues {
            queries::insert_subtitle_cue(&tx, &lecture, &course, start_ms, &text)?;
            total += 1;
        }
    }
    tx.commit()?;
    Ok(total)
}

/// Delete thumbnail-cache files no longer referenced by any course.
pub fn gc_thumbnails(conn: &Connection, dir: &Path) -> Result<GcReport> {
    let referenced: HashSet<String> = queries::all_thumbnail_paths(conn)?
        .iter()
        .filter_map(|p| Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();

    let mut report = GcReport { removed: 0, freed_bytes: 0 };
    if !dir.exists() {
        return Ok(report);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else { continue };
        if !path.is_file() || referenced.contains(&name) {
            continue;
        }
        let size = entry.metadata().map(|m| m.len() as i64).unwrap_or(0);
        if std::fs::remove_file(&path).is_ok() {
            report.removed += 1;
            report.freed_bytes += size;
        }
    }
    Ok(report)
}

/// Size of the SQLite files (main + WAL + shared memory).
pub fn db_bytes(data_dir: &Path) -> u64 {
    [DB_FILE.to_string(), format!("{DB_FILE}-wal"), format!("{DB_FILE}-shm")]
        .iter()
        .filter_map(|f| std::fs::metadata(data_dir.join(f)).ok())
        .map(|m| m.len())
        .sum()
}

/// Total size of the (flat, content-addressed) files directly under `dir`.
pub fn dir_bytes(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .map(|m| m.len())
        .sum()
}

/// On-disk footprint of the local stores.
pub fn storage(conn: &Connection, data_dir: &Path, thumbnails: &Path) -> Result<StorageStats> {
    Ok(StorageStats {
        db_bytes: db_bytes(data_dir),
        thumbnail_bytes: dir_bytes(thumbnails),
        subtitle_cues: queries::subtitle_index_count(conn)?,
    })
}

/// VACUUM to reclaim pages freed by removed courses or cleared indexes
/// (SQLite never shrinks the file on its own). Returns the new size.
pub fn compact(conn: &Connection, data_dir: &Path) -> Result<u64> {
    conn.execute_batch("VACUUM;")?;
    Ok(db_bytes(data_dir))
}

/// Drop the subtitle full-text index — the largest reclaimable chunk.
/// Returns the cues removed; the file only shrinks after a compact.
pub fn clear_subtitles(conn: &Connection) -> Result<i64> {
    let n = queries::subtitle_index_count(conn)?;
    queries::clear_subtitle_index(conn)?;
    Ok(n)
}

/// Write a backup archive of the library to `dest`: a consistent snapshot of
/// the database (taken under a brief lock), config and thumbnails.
pub fn export_backup(
    db: &Mutex<Connection>,
    data_dir: &Path,
    config: &Path,
    thumbnails: &Path,
    dest: &Path,
    app_version: &str,
) -> Result<()> {
    let tmp = data_dir.join(".export.tmp.db");
    let _ = std::fs::remove_file(&tmp);
    let schema: i64 = {
        let conn = lock(db);
        let version = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
        conn.execute("VACUUM INTO ?1", [tmp.to_string_lossy().as_ref()])?;
        version
    };
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let result = crate::backup::write_archive(&tmp, config, thumbnails, dest, app_version, schema, created_at);
    let _ = std::fs::remove_file(&tmp);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumbnail_gc_keeps_referenced_files() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("thumbnails");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("kept.png"), b"12345").unwrap();
        std::fs::write(dir.join("stale.png"), b"123").unwrap();

        let conn = crate::db::open_in_memory().unwrap();
        conn.execute(
            "INSERT INTO courses (id, title, folder_path, thumbnail_path, imported_at)
             VALUES ('c', 'C', '/c', ?1, 0)",
            [dir.join("kept.png").to_string_lossy().as_ref()],
        )
        .unwrap();

        let report = gc_thumbnails(&conn, &dir).unwrap();
        assert_eq!((report.removed, report.freed_bytes), (1, 3));
        assert!(dir.join("kept.png").exists());
        assert!(!dir.join("stale.png").exists());
        assert_eq!(dir_bytes(&dir), 5);
        // A missing cache is simply clean.
        assert_eq!(gc_thumbnails(&conn, &tmp.path().join("none")).unwrap().removed, 0);
    }

    #[test]
    fn exported_backup_restores_into_another_data_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let (from, to) = (tmp.path().join("from"), tmp.path().join("to"));
        std::fs::create_dir_all(from.join("thumbnails")).unwrap();
        std::fs::create_dir_all(&to).unwrap();
        std::fs::write(from.join("thumbnails").join("cover.png"), b"png").unwrap();
        std::fs::write(from.join("config.json"), r#"{"daily_goal_minutes": 90}"#).unwrap();

        let conn = crate::db::open(&from.join(DB_FILE)).unwrap();
        conn.execute(
            "INSERT INTO courses (id, title, folder_path, imported_at) VALUES ('c', 'Kept', '/c', 0)",
            [],
        )
        .unwrap();
        let db = Mutex::new(conn);
        let zip = tmp.path().join("backup.zip");
        export_backup(&db, &from, &from.join("config.json"), &from.join("thumbnails"), &zip, "1.2.2").unwrap();
        assert!(!from.join(".export.tmp.db").exists(), "the snapshot is cleaned up");

        crate::backup::stage_import(&to, &zip, crate::db::SCHEMA_VERSION).unwrap();
        assert!(crate::backup::apply_pending_import(&to).unwrap());
        let restored = crate::db::open(&to.join(DB_FILE)).unwrap();
        let title: String = restored.query_row("SELECT title FROM courses WHERE id = 'c'", [], |r| r.get(0)).unwrap();
        assert_eq!(title, "Kept");
        assert!(to.join("thumbnails").join("cover.png").exists());
        let config = crate::config::AppConfig::load(&to.join("config.json")).unwrap();
        assert_eq!(config.daily_goal_minutes, 90);
    }
}
