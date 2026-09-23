//! Where Deskemy keeps its data. Shared so every frontend opens the same
//! library: the Tauri app resolves the same directory via
//! `app.path().app_data_dir()` (OS data dir + bundle identifier).

use std::path::PathBuf;

/// Bundle identifier from `tauri.conf.json`; names the data directory.
pub const IDENTIFIER: &str = "com.spooksy.deskemy";

pub const DB_FILE: &str = "deskemy.db";
pub const CONFIG_FILE: &str = "config.json";

/// Portable copies keep their data in a sibling `data/` folder, flagged by a
/// `.portable` marker file next to the executable.
pub fn portable_data_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join(".portable").exists().then(|| dir.join("data"))
}

/// The data directory to use: `DESKEMY_DATA_DIR` if set (handy for testing
/// against a scratch library), else the portable dir, else the OS per-user
/// data dir (`%APPDATA%`, `~/.local/share`, `~/Library/Application Support`).
pub fn data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("DESKEMY_DATA_DIR") {
        return Some(PathBuf::from(d));
    }
    portable_data_dir().or_else(|| dirs::data_dir().map(|d| d.join(IDENTIFIER)))
}
