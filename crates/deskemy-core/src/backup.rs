//! Data export / import: a single `.zip` snapshot of the app data (database,
//! config, and thumbnail cache) that can be carried to another Deskemy install —
//! chiefly a portable copy moving to a new release, where each zip ships an empty
//! `data/` folder.
//!
//! This is NOT sync. On import the restored database re-attaches to your video
//! files by path (and by `content_hash` if they moved). It cannot map progress
//! onto a *different* download of a course — different files have different
//! hashes and paths, so there is nothing to match against.
//!
//! Because the live SQLite file is locked while the app runs, import doesn't swap
//! in place: it stages the archive into `data/.pending-import/`, and the swap
//! happens on the next startup (`apply_pending_import`) before the db is opened.

use crate::error::{DeskemyError, Result};
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::Path;

const MANIFEST_NAME: &str = "deskemy-backup.json";
const PENDING_DIR: &str = ".pending-import";
const DB_NAME: &str = "deskemy.db";
const CONFIG_NAME: &str = "config.json";
const THUMBS_DIR: &str = "thumbnails";

#[derive(Serialize, Deserialize)]
struct Manifest {
    app_version: String,
    /// SQLite `user_version` at export time; import refuses a value newer than
    /// what this build understands.
    schema_version: i64,
    /// Unix epoch seconds.
    created_at: i64,
}

/// Write a `.zip` snapshot to `dest`. `db_snapshot` must be a consistent copy of
/// the database (the caller produces one via `VACUUM INTO`, which drops the WAL).
pub fn write_archive(
    db_snapshot: &Path,
    config_path: &Path,
    thumbs_dir: &Path,
    dest: &Path,
    app_version: &str,
    schema_version: i64,
    created_at: i64,
) -> Result<()> {
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    let manifest = Manifest {
        app_version: app_version.to_string(),
        schema_version,
        created_at,
    };
    zip.start_file(MANIFEST_NAME, opts)?;
    std::io::Write::write_all(&mut zip, serde_json::to_string_pretty(&manifest)?.as_bytes())?;

    add_file(&mut zip, db_snapshot, DB_NAME, opts)?;
    if config_path.exists() {
        add_file(&mut zip, config_path, CONFIG_NAME, opts)?;
    }
    if thumbs_dir.is_dir() {
        for entry in std::fs::read_dir(thumbs_dir)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let name = format!("{THUMBS_DIR}/{}", entry.file_name().to_string_lossy());
                add_file(&mut zip, &entry.path(), &name, opts)?;
            }
        }
    }
    zip.finish()?;
    Ok(())
}

fn add_file(
    zip: &mut zip::ZipWriter<std::fs::File>,
    path: &Path,
    name: &str,
    opts: zip::write::SimpleFileOptions,
) -> Result<()> {
    zip.start_file(name, opts)?;
    let mut f = std::fs::File::open(path)?;
    std::io::copy(&mut f, zip)?;
    Ok(())
}

/// Validate `src` and extract it into `data_dir/.pending-import/`. The real swap
/// happens on next startup. `current_schema` is this build's `SCHEMA_VERSION`.
pub fn stage_import(data_dir: &Path, src: &Path, current_schema: i64) -> Result<()> {
    let file = std::fs::File::open(src)?;
    let mut archive = zip::ZipArchive::new(file)?;

    let manifest: Manifest = {
        let mut mf = archive
            .by_name(MANIFEST_NAME)
            .map_err(|_| DeskemyError::Other("not a Deskemy backup (no manifest)".into()))?;
        let mut s = String::new();
        mf.read_to_string(&mut s)?;
        serde_json::from_str(&s)?
    };
    if manifest.schema_version > current_schema {
        return Err(DeskemyError::Other(format!(
            "this backup is from a newer version of Deskemy (data format v{}, this build supports v{}). Update Deskemy and try again.",
            manifest.schema_version, current_schema
        )));
    }

    let pending = data_dir.join(PENDING_DIR);
    let _ = std::fs::remove_dir_all(&pending);
    std::fs::create_dir_all(&pending)?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name().to_string();
        if name == MANIFEST_NAME {
            continue;
        }
        // Only accept the files we write; reject anything with a traversal or
        // absolute component so a crafted archive can't escape the pending dir.
        let is_ours = name == DB_NAME
            || name == CONFIG_NAME
            || name.strip_prefix(&format!("{THUMBS_DIR}/")).is_some_and(|r| !r.is_empty());
        if !is_ours || name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
            continue;
        }
        if entry.is_dir() {
            continue;
        }
        let out = pending.join(&name);
        if let Some(p) = out.parent() {
            std::fs::create_dir_all(p)?;
        }
        let mut w = std::fs::File::create(&out)?;
        std::io::copy(&mut entry, &mut w)?;
    }

    if !pending.join(DB_NAME).exists() {
        let _ = std::fs::remove_dir_all(&pending);
        return Err(DeskemyError::Other(
            "backup archive is missing its database".into(),
        ));
    }
    Ok(())
}

/// On startup, before the database is opened: if an import was staged, swap it
/// into place. The old process is gone, so the db file is no longer locked.
/// Returns `true` if an import was applied.
pub fn apply_pending_import(data_dir: &Path) -> Result<bool> {
    let pending = data_dir.join(PENDING_DIR);
    let new_db = pending.join(DB_NAME);
    if !new_db.exists() {
        return Ok(false);
    }

    // Replace the db, and delete the old WAL/SHM so no stale journal is applied
    // over the freshly-imported database.
    let db = data_dir.join(DB_NAME);
    let _ = std::fs::remove_file(data_dir.join(format!("{DB_NAME}-wal")));
    let _ = std::fs::remove_file(data_dir.join(format!("{DB_NAME}-shm")));
    move_replace(&new_db, &db)?;

    let new_cfg = pending.join(CONFIG_NAME);
    if new_cfg.exists() {
        let _ = move_replace(&new_cfg, &data_dir.join(CONFIG_NAME));
    }

    let new_thumbs = pending.join(THUMBS_DIR);
    if new_thumbs.is_dir() {
        let dst = data_dir.join(THUMBS_DIR);
        let _ = std::fs::remove_dir_all(&dst);
        std::fs::create_dir_all(&dst)?;
        for entry in std::fs::read_dir(&new_thumbs)? {
            let entry = entry?;
            let _ = move_replace(&entry.path(), &dst.join(entry.file_name()));
        }
    }

    let _ = std::fs::remove_dir_all(&pending);
    tracing::info!("applied staged data import");
    Ok(true)
}

/// Rename `from` over `to`, falling back to copy+delete across volumes.
fn move_replace(from: &Path, to: &Path) -> Result<()> {
    let _ = std::fs::remove_file(to);
    match std::fs::rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) => {
            std::fs::copy(from, to)?;
            let _ = std::fs::remove_file(from);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_stage_apply_roundtrip() {
        let base = std::env::temp_dir().join(format!("deskemy-bak-test-{}", std::process::id()));
        let src = base.join("src");
        let dst = base.join("dst");
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(src.join(THUMBS_DIR)).unwrap();
        std::fs::create_dir_all(&dst).unwrap();

        let db_snap = src.join("db.snap");
        std::fs::write(&db_snap, b"SQLITE-DATA").unwrap();
        std::fs::write(src.join(CONFIG_NAME), b"{\"theme\":\"dark\"}").unwrap();
        std::fs::write(src.join(THUMBS_DIR).join("a.png"), b"PNG").unwrap();
        // A stale WAL beside the destination db must be dropped on apply.
        std::fs::write(dst.join(format!("{DB_NAME}-wal")), b"stale").unwrap();

        let archive = base.join("backup.zip");
        write_archive(&db_snap, &src.join(CONFIG_NAME), &src.join(THUMBS_DIR), &archive, "1.1.0", 7, 123).unwrap();
        assert!(archive.exists());

        stage_import(&dst, &archive, 7).unwrap();
        assert!(dst.join(PENDING_DIR).join(DB_NAME).exists());

        assert!(apply_pending_import(&dst).unwrap());
        assert_eq!(std::fs::read(dst.join(DB_NAME)).unwrap(), b"SQLITE-DATA");
        assert_eq!(std::fs::read(dst.join(CONFIG_NAME)).unwrap(), b"{\"theme\":\"dark\"}");
        assert_eq!(std::fs::read(dst.join(THUMBS_DIR).join("a.png")).unwrap(), b"PNG");
        assert!(!dst.join(PENDING_DIR).exists());
        assert!(!dst.join(format!("{DB_NAME}-wal")).exists()); // stale WAL gone

        // A backup from a newer schema is refused, and no import is left staged.
        let newer = base.join("newer.zip");
        write_archive(&db_snap, &src.join(CONFIG_NAME), &src.join(THUMBS_DIR), &newer, "9.9.9", 999, 123).unwrap();
        assert!(stage_import(&dst, &newer, 7).is_err());
        assert!(!apply_pending_import(&dst).unwrap());

        let _ = std::fs::remove_dir_all(&base);
    }

    // The export snapshots the live db with `VACUUM INTO` (the command's exact
    // call). That copy MUST keep its data and `user_version`, or an imported db
    // would look unmigrated and re-run schema creation on populated tables.
    #[test]
    fn vacuum_into_snapshot_preserves_data_and_user_version() {
        use rusqlite::Connection;
        let base = std::env::temp_dir().join(format!("deskemy-vac-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        let snap = base.join("snap.db");
        {
            let conn = Connection::open(base.join("live.db")).unwrap();
            conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x); INSERT INTO t VALUES (42);")
                .unwrap();
            conn.pragma_update(None, "user_version", 7i64).unwrap();
            conn.execute("VACUUM INTO ?1", [snap.to_string_lossy().as_ref()])
                .unwrap();
        }
        let s = Connection::open(&snap).unwrap();
        assert_eq!(s.query_row("SELECT x FROM t", [], |r| r.get::<_, i64>(0)).unwrap(), 42);
        assert_eq!(
            s.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0)).unwrap(),
            7
        );
        let _ = std::fs::remove_dir_all(&base);
    }
}
