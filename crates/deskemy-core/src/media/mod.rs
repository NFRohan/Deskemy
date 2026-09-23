//! Media probing abstraction. Callers depend only on `MediaProber` + the
//! concrete `MediaMetadata` return shape — never on a specific backend.
//!
//! M1 ships `StubProber` (no native deps). M3 adds `MpvProber` (libmpv) behind
//! the same trait, and an `FfprobeProber` could follow, all without touching
//! the importer.

pub mod mpv_prober;
pub mod stub;

use crate::error::Result;
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Debug, Clone, Serialize)]
pub struct VideoTrack {
    pub id: i64,
    pub codec: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AudioTrack {
    pub id: i64,
    pub codec: String,
    pub lang: Option<String>,
    pub channels: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SubtitleTrack {
    pub id: i64,
    pub lang: Option<String>,
    pub title: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Chapter {
    pub index: usize,
    pub title: Option<String>,
    pub start: Duration,
}

/// The single object every `MediaProber` implementation returns.
#[derive(Debug, Clone, Serialize)]
pub struct MediaMetadata {
    /// `None` when the backend can't determine it (e.g. the stub prober).
    pub duration: Option<Duration>,
    /// `false` when the file exists but can't be opened/decoded (corrupted).
    pub playable: bool,
    pub video_tracks: Vec<VideoTrack>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,
    pub chapters: Vec<Chapter>,
    pub container: String,
    pub video_codec: Option<String>,
    pub thumbnail: Option<PathBuf>,
}

pub trait MediaProber: Send + Sync {
    fn probe(&self, path: &Path) -> Result<MediaMetadata>;
}
