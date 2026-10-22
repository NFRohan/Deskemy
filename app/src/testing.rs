//! Helpers for tests that drive a real mpv over real (tiny) videos.

use deskemy_core::mpv::Mpv;
use std::path::Path;
use std::sync::Arc;

/// A real mpv core with no video/audio output, or None without libmpv.
/// With DESKEMY_REQUIRE_LIBMPV set (CI, release QA) a missing or broken
/// libmpv fails the test instead of skipping it.
pub fn headless_mpv() -> Option<Arc<Mpv>> {
    let required = std::env::var_os("DESKEMY_REQUIRE_LIBMPV").is_some();
    let skip = |why: &str| {
        assert!(!required, "{why} (DESKEMY_REQUIRE_LIBMPV is set)");
        eprintln!("{why} — skipping");
    };
    if !deskemy_core::mpv::is_available() {
        skip("libmpv not found");
        return None;
    }
    let mpv = match Mpv::new() {
        Ok(mpv) => mpv,
        Err(e) => {
            skip(&format!("libmpv didn't start: {e}"));
            return None;
        }
    };
    for (name, value) in [("vo", "null"), ("ao", "null"), ("idle", "yes"), ("config", "no")] {
        if let Err(e) = mpv.set_option(name, value) {
            skip(&format!("mpv option {name}: {e}"));
            return None;
        }
    }
    if let Err(e) = mpv.initialize() {
        skip(&format!("mpv didn't initialize: {e}"));
        return None;
    }
    Some(Arc::new(mpv))
}

/// A black 64×36 video at `path`, `seconds` long at 25 fps, with a keyframe
/// every `keyframe_every` seconds. False (and a note) without ffmpeg.
pub fn make_video(path: &Path, seconds: u32, keyframe_every: u32) -> bool {
    let made = std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-f", "lavfi", "-i"])
        .arg(format!("color=c=black:s=64x36:r=25:d={seconds}"))
        .args(["-c:v", "mpeg4", "-g"])
        .arg((keyframe_every * 25).to_string())
        .arg("-y")
        .arg(path)
        .status();
    let made = made.is_ok_and(|s| s.success());
    if !made {
        eprintln!("ffmpeg not available — skipping");
    }
    made
}
