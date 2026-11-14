//! The A–B loop: a stretch of a lecture played over and over, to go through
//! a fast part again (slower, with the speed control, if wanted). mpv does
//! the looping (`ab-loop-a` / `ab-loop-b`); one action steps through it: the
//! start, then the end (looping from there), then off. A lecture is always
//! loaded without one.

use deskemy_core::mpv::Mpv;

/// The shortest loop: an end this close to the start isn't taken.
const SHORTEST: f64 = 0.5;
/// The end is kept this far before the file's own, so playback loops instead
/// of reaching the end (and the next lecture).
const BEFORE_END: f64 = 0.25;

/// The loop's start and end, after a press at `pos`. An end before the start
/// swaps them.
pub fn step(loop_: (Option<f64>, Option<f64>), pos: f64, duration: f64) -> (Option<f64>, Option<f64>) {
    let end = |t: f64| if duration > 0.0 { t.min(duration - BEFORE_END) } else { t };
    match loop_ {
        (Some(_), Some(_)) => (None, None),
        (Some(a), None) if (pos - a).abs() < SHORTEST => (Some(a), None),
        (Some(a), None) => (Some(a.min(pos)), Some(end(a.max(pos)))),
        (None, _) => (Some(pos), None),
    }
}

/// The loop as mpv has it: (start, end), each None when not set.
pub fn read(mpv: &Mpv) -> (Option<f64>, Option<f64>) {
    (mpv.get_f64("ab-loop-a"), mpv.get_f64("ab-loop-b"))
}

fn set(mpv: &Mpv, (a, b): (Option<f64>, Option<f64>)) {
    let value = |t: Option<f64>| t.map_or_else(|| "no".to_string(), |t| format!("{t:.3}"));
    for (name, t) in [("ab-loop-a", a), ("ab-loop-b", b)] {
        if let Err(e) = mpv.set_property(name, &value(t)) {
            tracing::warn!(error = %e, name, "set loop");
        }
    }
}

/// One press of A (or the loop button), where playback is now.
pub fn press(mpv: &Mpv) {
    let Some(pos) = mpv.get_f64("time-pos") else { return };
    let next = step(read(mpv), pos, mpv.get_f64("duration").unwrap_or(0.0));
    tracing::info!(start = ?next.0, end = ?next.1, "loop");
    set(mpv, next);
}

/// No loop (mpv keeps one from file to file).
pub fn clear(mpv: &Mpv) {
    set(mpv, (None, None));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{headless_mpv, make_video};
    use std::time::{Duration, Instant};

    #[test]
    fn a_press_sets_the_start_then_the_end_then_clears() {
        let start = step((None, None), 12.0, 100.0);
        assert_eq!(start, (Some(12.0), None));
        assert_eq!(step(start, 30.0, 100.0), (Some(12.0), Some(30.0)));
        assert_eq!(step((Some(12.0), Some(30.0)), 50.0, 100.0), (None, None));
    }

    #[test]
    fn an_end_before_the_start_swaps_them() {
        assert_eq!(step((Some(30.0), None), 12.0, 100.0), (Some(12.0), Some(30.0)));
    }

    #[test]
    fn an_end_too_close_to_the_start_isnt_taken() {
        assert_eq!(step((Some(12.0), None), 12.3, 100.0), (Some(12.0), None));
        assert_eq!(step((Some(12.0), None), 11.8, 100.0), (Some(12.0), None));
    }

    #[test]
    fn the_end_stays_short_of_the_files() {
        assert_eq!(step((Some(90.0), None), 100.0, 100.0), (Some(90.0), Some(99.75)));
        // Duration not known yet: as pressed.
        assert_eq!(step((Some(90.0), None), 100.0, 0.0), (Some(90.0), Some(100.0)));
    }

    /// With a loop from 2 s to 4 s, playback keeps coming back to 2 s and
    /// never passes 4 s or reaches the end; cleared, it plays on.
    #[test]
    fn mpv_loops_the_stretch() {
        let Some(mpv) = headless_mpv() else { return };
        let dir = tempfile::tempdir().unwrap();
        let video = dir.path().join("lecture.mp4");
        if !make_video(&video, 10, 1) {
            return;
        }
        mpv.command(&["loadfile", &video.to_string_lossy()]).unwrap();
        let ready = Instant::now() + Duration::from_secs(10);
        while mpv.get_f64("duration").is_none() {
            assert!(Instant::now() < ready, "the video never loaded");
            mpv.wait_event(0.05);
        }
        set(&mpv, (Some(2.0), Some(4.0)));
        assert_eq!(read(&mpv), (Some(2.0), Some(4.0)));
        mpv.set_property("speed", "4").unwrap();
        mpv.set_property("pause", "no").unwrap();

        // Watch 3 s (12 s of video at 4×, past the end were it not looping).
        let (mut furthest, mut came_back) = (0.0_f64, false);
        let until = Instant::now() + Duration::from_secs(3);
        while Instant::now() < until {
            mpv.wait_event(0.02);
            let Some(at) = mpv.get_f64("time-pos") else { continue };
            assert!(at < 4.3, "played past the loop's end: {at}");
            came_back |= furthest > 3.5 && at < 2.5;
            furthest = furthest.max(at);
        }
        assert!(came_back, "never looped back (furthest {furthest})");
        assert_ne!(mpv.get_property_string("eof-reached").as_deref(), Some("yes"));

        clear(&mpv);
        assert_eq!(read(&mpv), (None, None));
    }
}
