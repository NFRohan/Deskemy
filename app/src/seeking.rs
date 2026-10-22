//! Seeking: commands that land where they're sent.

use deskemy_core::mpv::Mpv;

/// Seek to `t` seconds. Absolute seeks are exact by mpv's default.
pub fn seek_to(mpv: &Mpv, t: f64) {
    seek(mpv, &["seek", &format!("{t:.3}"), "absolute"]);
}

/// Skip `delta` seconds. mpv lands a relative seek on a keyframe unless it's
/// told to be exact, and keyframes can be seconds apart: a +10 s skip could
/// land short or barely move.
pub fn seek_by(mpv: &Mpv, delta: f64) {
    seek(mpv, &["seek", &format!("{delta:.3}"), "relative+exact"]);
}

fn seek(mpv: &Mpv, args: &[&str]) {
    if let Err(e) = mpv.command(args) {
        tracing::warn!(error = %e, ?args, "mpv seek");
    }
}

#[cfg(test)]
mod tests {
    use super::{seek_by, seek_to};
    use crate::testing::{headless_mpv, make_video};
    use deskemy_core::mpv::{Mpv, MPV_EVENT_PLAYBACK_RESTART};
    use std::time::{Duration, Instant};

    /// Wait for mpv to finish a seek (or a load) and resume output.
    fn restarted(mpv: &Mpv) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            assert!(Instant::now() < deadline, "mpv never finished the seek");
            let event = mpv.wait_event(0.1);
            if !event.is_null() && unsafe { (*event).event_id } == MPV_EVENT_PLAYBACK_RESTART {
                return;
            }
        }
    }

    /// Every seek lands where it was sent, playing or paused, forward or
    /// back, in a video whose keyframes are 10 s apart.
    #[test]
    fn seeks_land_on_their_target() {
        let Some(mpv) = headless_mpv() else { return };
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("lecture.mp4");
        if !make_video(&video, 30, 10) {
            return;
        }
        mpv.command(&["loadfile", &video.to_string_lossy()]).unwrap();
        restarted(&mpv);

        type Seek = fn(&Mpv, f64);
        let cases: [(&str, Seek, f64, f64); 4] = [
            ("skip forward", seek_by, 5.0, 17.0),
            ("skip back", seek_by, -5.0, 7.0),
            ("seek forward", seek_to, 23.0, 23.0),
            ("seek back", seek_to, 4.0, 4.0),
        ];
        for paused in [true, false] {
            for (what, seek, arg, target) in cases {
                mpv.set_property("pause", "yes").unwrap();
                seek_to(&mpv, 12.0);
                restarted(&mpv);
                mpv.set_property("pause", if paused { "yes" } else { "no" }).unwrap();

                seek(&mpv, arg);
                restarted(&mpv);
                let at = mpv.get_f64("time-pos").unwrap();
                assert!((at - target).abs() < 0.2, "{what} (paused: {paused}) landed at {at}, not {target}");
            }
        }
    }
}
