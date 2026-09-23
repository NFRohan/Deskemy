//! Playback rules shared by every frontend's player: where a lecture resumes
//! and when it counts as watched.

/// Fraction of a video that must be watched for it to auto-mark as complete
/// (the ✓ badge, section counts, stats) when the user stops *before* the end.
/// Reaching actual EOF always marks done regardless. Kept a touch under 1.0 so
/// trailing outros / "see you next lecture" tails don't block completion, but
/// high enough that a long video isn't marked done with many minutes still left.
pub const COMPLETE_FRACTION: f64 = 0.95;

/// Whether stopping at `position` of `duration` counts as having watched it.
pub fn watched_enough(position: f64, duration: f64) -> bool {
    duration > 0.0 && position / duration >= COMPLETE_FRACTION
}

/// Where playback should start when (re)loading a lecture.
///
/// Resume the saved position — even for videos marked complete (watched >=90%) —
/// EXCEPT when the saved spot is effectively at the very end, where resuming
/// would just replay the final seconds; then start over from 0. This keeps the
/// `completed` flag (used for badges/stats) independent of the resume point, so a
/// video you stopped at, say, 92% resumes at 92% instead of restarting. Falls
/// back to the `completed` flag when the duration is unknown (unprobed).
///
/// `END_EPSILON_SECS` is the "close enough to the end to start over" window.
pub fn resume_start(resume: bool, saved_pos: f64, completed: bool, duration: Option<f64>) -> f64 {
    const END_EPSILON_SECS: f64 = 5.0;
    if !resume {
        return 0.0;
    }
    let at_end = match duration {
        Some(d) if d > 0.0 => saved_pos >= d - END_EPSILON_SECS,
        _ => completed,
    };
    if at_end {
        0.0
    } else {
        saved_pos
    }
}

#[cfg(test)]
mod tests {
    use super::{resume_start, watched_enough};

    #[test]
    fn resumes_partway_through() {
        // 50% of a 600s video → resume there.
        assert_eq!(resume_start(true, 300.0, false, Some(600.0)), 300.0);
    }

    #[test]
    fn resumes_completed_video_stopped_before_the_end() {
        // The bug: watched to 92% (marked complete) but not finished → must
        // resume at 552s, not restart at 0.
        assert_eq!(resume_start(true, 552.0, true, Some(600.0)), 552.0);
    }

    #[test]
    fn restarts_when_effectively_at_the_end() {
        // Watched to the final seconds → replaying the end is pointless; start over.
        assert_eq!(resume_start(true, 599.0, true, Some(600.0)), 0.0);
        assert_eq!(resume_start(true, 600.0, true, Some(600.0)), 0.0);
    }

    #[test]
    fn never_resumes_when_resume_is_off() {
        // Explicit "play from start" (e.g. autoplay advance) always starts at 0.
        assert_eq!(resume_start(false, 300.0, false, Some(600.0)), 0.0);
    }

    #[test]
    fn falls_back_to_completed_flag_when_duration_unknown() {
        // Unprobed lecture (no duration): keep the old behavior — completed
        // restarts, in-progress resumes.
        assert_eq!(resume_start(true, 300.0, true, None), 0.0);
        assert_eq!(resume_start(true, 300.0, false, None), 300.0);
    }

    #[test]
    fn short_video_end_window() {
        // On a very short clip, the last 5s counts as "at the end".
        assert_eq!(resume_start(true, 7.0, true, Some(8.0)), 0.0);
    }

    #[test]
    fn completion_threshold() {
        assert!(!watched_enough(569.0, 600.0));
        assert!(watched_enough(570.0, 600.0));
        // Unknown duration never auto-completes (EOF still does).
        assert!(!watched_enough(570.0, 0.0));
    }
}
