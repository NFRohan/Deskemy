//! Shared application state managed by Tauri.

use crate::config::AppConfig;
use crate::importer::{ImportPlan, ImportSnapshot, Importer};
use crate::media::mpv_prober::MpvProber;
use crate::media::stub::StubProber;
use crate::media::MediaProber;
use crate::player::MpvPlayer;
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    /// App data directory (holds the db, config, and thumbnails cache).
    pub data_dir: PathBuf,
    pub importer: Importer,
    /// Probed-but-unpersisted import plans keyed by folder path, staged by
    /// `library_preview_import` so confirming the import doesn't re-probe.
    pub pending_imports: Mutex<HashMap<String, (ImportSnapshot, ImportPlan)>>,
    /// Embedded mpv player, created lazily on first playback.
    pub player: Mutex<Option<MpvPlayer>>,
}

impl AppState {
    pub fn new(db: Connection, config: AppConfig, data_dir: PathBuf, config_path: PathBuf) -> Self {
        // Prefer the real libmpv prober; fall back to the stub if the DLL is
        // unavailable so the app still runs (import just lacks durations).
        let prober: Box<dyn MediaProber> = if MpvProber::available() {
            tracing::info!("using libmpv prober");
            Box::new(MpvProber)
        } else {
            tracing::warn!("libmpv unavailable — using stub prober (no durations)");
            Box::new(StubProber)
        };

        Self {
            db: Mutex::new(db),
            config: Mutex::new(config),
            config_path,
            data_dir,
            importer: Importer::new(prober),
            pending_imports: Mutex::new(HashMap::new()),
            player: Mutex::new(None),
        }
    }

    /// Directory where course thumbnails are cached.
    pub fn thumbnails_dir(&self) -> PathBuf {
        self.data_dir.join("thumbnails")
    }
}
