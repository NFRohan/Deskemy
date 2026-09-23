//! libmpv-backed prober. Loads each file headlessly (no video/audio output),
//! waits for it to demux, then reads metadata via mpv property paths.

use super::{AudioTrack, Chapter, MediaMetadata, MediaProber, SubtitleTrack, VideoTrack};
use crate::error::Result;
use crate::mpv::{Mpv, MPV_EVENT_END_FILE, MPV_EVENT_FILE_LOADED, MPV_EVENT_SHUTDOWN};
use std::path::Path;
use std::time::{Duration, Instant};

pub struct MpvProber;

impl MpvProber {
    pub fn available() -> bool {
        crate::mpv::is_available()
    }
}

impl MediaProber for MpvProber {
    fn probe(&self, path: &Path) -> Result<MediaMetadata> {
        let mpv = Mpv::new()?;
        // Headless: no windows, no output, don't actually play.
        mpv.set_option("vo", "null")?;
        mpv.set_option("ao", "null")?;
        mpv.set_option("pause", "yes")?;
        mpv.set_option("idle", "yes")?;
        mpv.set_option("terminal", "no")?;
        mpv.set_option("load-scripts", "no")?;
        mpv.set_option("ytdl", "no")?;
        mpv.initialize()?;

        mpv.command(&["loadfile", &path.to_string_lossy()])?;

        if !wait_loaded(&mpv, Duration::from_secs(15)) {
            return Ok(unplayable(path));
        }

        let duration = mpv.get_f64("duration").map(Duration::from_secs_f64);
        let container = mpv
            .get_property_string("file-format")
            .unwrap_or_else(|| extension(path));
        let video_codec = mpv.get_property_string("video-codec");
        let chapters = read_chapters(&mpv);
        let (video_tracks, audio_tracks, subtitle_tracks) = read_tracks(&mpv);

        Ok(MediaMetadata {
            duration,
            playable: true,
            video_tracks,
            audio_tracks,
            subtitle_tracks,
            chapters,
            container,
            video_codec,
            thumbnail: None,
        })
    }
}

fn wait_loaded(mpv: &Mpv, timeout: Duration) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        let ev = mpv.wait_event(0.5);
        if ev.is_null() {
            continue;
        }
        match unsafe { (*ev).event_id } {
            MPV_EVENT_FILE_LOADED => return true,
            MPV_EVENT_END_FILE | MPV_EVENT_SHUTDOWN => return false,
            _ => {}
        }
    }
    false
}

fn read_chapters(mpv: &Mpv) -> Vec<Chapter> {
    let n = mpv.get_i64("chapters").unwrap_or(0).max(0);
    (0..n)
        .map(|i| {
            let title = mpv.get_property_string(&format!("chapter-list/{i}/title"));
            let start = mpv
                .get_f64(&format!("chapter-list/{i}/time"))
                .unwrap_or(0.0)
                .max(0.0);
            Chapter {
                index: i as usize,
                title,
                start: Duration::from_secs_f64(start),
            }
        })
        .collect()
}

fn read_tracks(mpv: &Mpv) -> (Vec<VideoTrack>, Vec<AudioTrack>, Vec<SubtitleTrack>) {
    let n = mpv.get_i64("track-list/count").unwrap_or(0).max(0);
    let mut videos = Vec::new();
    let mut audios = Vec::new();
    let mut subs = Vec::new();

    for i in 0..n {
        let kind = mpv
            .get_property_string(&format!("track-list/{i}/type"))
            .unwrap_or_default();
        let id = mpv.get_i64(&format!("track-list/{i}/id")).unwrap_or(0);
        let lang = mpv.get_property_string(&format!("track-list/{i}/lang"));
        let title = mpv.get_property_string(&format!("track-list/{i}/title"));
        let codec = mpv
            .get_property_string(&format!("track-list/{i}/codec"))
            .unwrap_or_default();

        match kind.as_str() {
            "video" => videos.push(VideoTrack {
                id,
                codec,
                width: mpv
                    .get_i64(&format!("track-list/{i}/demux-w"))
                    .map(|w| w as u32),
                height: mpv
                    .get_i64(&format!("track-list/{i}/demux-h"))
                    .map(|h| h as u32),
            }),
            "audio" => audios.push(AudioTrack {
                id,
                codec,
                lang,
                channels: mpv
                    .get_i64(&format!("track-list/{i}/demux-channel-count"))
                    .map(|c| c as u32),
            }),
            "sub" => subs.push(SubtitleTrack { id, lang, title }),
            _ => {}
        }
    }
    (videos, audios, subs)
}

fn unplayable(path: &Path) -> MediaMetadata {
    MediaMetadata {
        duration: None,
        playable: false,
        video_tracks: Vec::new(),
        audio_tracks: Vec::new(),
        subtitle_tracks: Vec::new(),
        chapters: Vec::new(),
        container: extension(path),
        video_codec: None,
        thumbnail: None,
    }
}

fn extension(path: &Path) -> String {
    path.extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}
