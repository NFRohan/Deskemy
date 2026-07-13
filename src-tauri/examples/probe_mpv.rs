//! Validate the libmpv prober against a single media file.
//!
//!   cargo run --example probe_mpv -- "D:\\path\\to\\lecture.mp4"

use deskemy_lib::media::mpv_prober::MpvProber;
use deskemy_lib::media::MediaProber;
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("usage: probe_mpv <file>");

    println!("libmpv available: {}", MpvProber::available());
    let meta = MpvProber.probe(Path::new(&path)).expect("probe failed");

    println!("playable:   {}", meta.playable);
    println!("container:  {}", meta.container);
    println!("video codec: {:?}", meta.video_codec);
    println!(
        "duration:   {}",
        meta.duration
            .map(|d| format!("{:.1}s", d.as_secs_f64()))
            .unwrap_or_else(|| "unknown".into())
    );
    println!(
        "tracks:     {} video, {} audio, {} subtitle",
        meta.video_tracks.len(),
        meta.audio_tracks.len(),
        meta.subtitle_tracks.len()
    );
    for a in &meta.audio_tracks {
        println!("   audio #{} {} lang={:?} ch={:?}", a.id, a.codec, a.lang, a.channels);
    }
    for v in &meta.video_tracks {
        println!("   video #{} {} {:?}x{:?}", v.id, v.codec, v.width, v.height);
    }
    println!("chapters:   {}", meta.chapters.len());
    for c in meta.chapters.iter().take(5) {
        println!("   [{}] {:?} @ {:.1}s", c.index, c.title, c.start.as_secs_f64());
    }
}
