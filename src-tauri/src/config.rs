//! Typed application config, persisted as `config.json` in the app-data dir.
//! Replaces a loose key-value settings table.

use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// "dark" | "light"
    pub theme: String,
    /// Default playback speed applied when opening a lecture.
    pub default_speed: f64,
    /// Auto-advance to the next lecture when one ends.
    pub autoplay_next: bool,
    /// Daily watch-time goal in minutes (stats page).
    pub daily_goal_minutes: i64,
    /// Auto re-import a course when its folder changes on disk. Off by default:
    /// re-importing probes new files while holding the DB lock, which can briefly
    /// block the UI, and it must not run against a course being watched.
    pub auto_rescan: bool,
    /// Last library root the user registered (convenience).
    pub last_root: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            default_speed: 1.0,
            autoplay_next: true,
            daily_goal_minutes: 30,
            auto_rescan: false,
            last_root: None,
        }
    }
}

impl AppConfig {
    /// Load from disk, falling back to defaults when the file is absent.
    pub fn load(path: &Path) -> Result<Self> {
        match std::fs::read_to_string(path) {
            Ok(contents) => Ok(serde_json::from_str(&contents)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// Persist to disk (creating parent dirs as needed).
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }
}
