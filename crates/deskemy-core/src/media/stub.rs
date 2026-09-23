//! Placeholder prober used until libmpv is wired in at M3. Infers the container
//! from the file extension and assumes playable; leaves duration/tracks/chapters
//! empty. Lets the importer run end-to-end without native media deps.

use super::{MediaMetadata, MediaProber};
use crate::error::Result;
use std::path::Path;

pub struct StubProber;

impl MediaProber for StubProber {
    fn probe(&self, path: &Path) -> Result<MediaMetadata> {
        let container = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        Ok(MediaMetadata {
            duration: None,
            playable: true,
            video_tracks: Vec::new(),
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            chapters: Vec::new(),
            container,
            video_codec: None,
            thumbnail: None,
        })
    }
}
