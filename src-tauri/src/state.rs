//! Shared application state managed by Tauri.

use crate::config::AppConfig;
use crate::importer::Importer;
use crate::media::mpv_prober::MpvProber;
use crate::media::stub::StubProber;
use crate::media::MediaProber;
use crate::player::MpvPlayer;
use rusqlite::Connection;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub config: Mutex<AppConfig>,
    pub config_path: PathBuf,
    pub importer: Importer,
    /// Embedded mpv player, created lazily on first playback.
    pub player: Mutex<Option<MpvPlayer>>,
}

impl AppState {
    pub fn new(db: Connection, config: AppConfig, config_path: PathBuf) -> Self {
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
            importer: Importer::new(prober),
            player: Mutex::new(None),
        }
    }
}
