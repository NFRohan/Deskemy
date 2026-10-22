//! Seeking: commands that land where they're sent, and the seek bar
//! holding a seek's target until it has.

use crate::{AppWindow, Playback};
use deskemy_core::mpv::Mpv;
use slint::ComponentHandle;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

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

/// Seeks sent from the player's controls: the bar goes to the target at once
/// and stays there until the seek lands (`Seeks`).
#[derive(Clone)]
pub struct Seeker {
    mpv: Arc<Mpv>,
    seeks: Arc<Seeks>,
    ui: slint::Weak<AppWindow>,
}

impl Seeker {
    pub fn new(mpv: &Arc<Mpv>, seeks: &Arc<Seeks>, ui: &AppWindow) -> Self {
        Seeker { mpv: mpv.clone(), seeks: seeks.clone(), ui: ui.as_weak() }
    }

    pub fn to(&self, t: f64) {
        self.show(|_| t);
        seek_to(&self.mpv, t);
    }

    pub fn by(&self, delta: f64) {
        self.show(|p| (f64::from(p.get_live_position()) + delta).clamp(0.0, f64::from(p.get_duration())));
        seek_by(&self.mpv, delta);
    }

    fn show(&self, target: impl FnOnce(&Playback) -> f64) {
        self.seeks.sent();
        if let Some(ui) = self.ui.upgrade() {
            let playback = ui.global::<Playback>();
            playback.set_position(target(&playback) as f32);
        }
    }
}

/// How long a seek may take before the bar shows readings again anyway (a
/// slow drive, or a seek mpv dropped).
const LANDING: Duration = Duration::from_secs(2);

/// Seeks on their way. Until mpv lands one, readings still say where
/// playback was, and showing them would flick the seek bar back to the old
/// position before it jumps to the new one. So the bar goes to the target as
/// the seek is sent, and readings taken before it lands aren't shown.
pub struct Seeks {
    state: Mutex<State>,
    landing: Duration,
}

struct State {
    /// Seeks sent so far: a reading taken before the latest isn't shown.
    sent: u64,
    /// Until when the latest may still be on its way.
    until: Option<Instant>,
}

impl Default for Seeks {
    fn default() -> Self {
        Seeks::with_landing(LANDING)
    }
}

impl Seeks {
    fn with_landing(landing: Duration) -> Self {
        Seeks { state: Mutex::new(State { sent: 0, until: None }), landing }
    }

    /// A seek is being sent.
    pub fn sent(&self) {
        let mut s = self.state();
        s.sent += 1;
        s.until = Some(Instant::now() + self.landing);
    }

    /// mpv restarted playback: the seek landed.
    pub fn landed(&self) {
        self.state().until = None;
    }

    /// Tag a reading taken now, for `shows`: None while a seek is on its way.
    pub fn reading(&self) -> Option<u64> {
        let s = self.state();
        match s.until {
            Some(until) if Instant::now() < until => None,
            _ => Some(s.sent),
        }
    }

    /// Whether a reading can be shown: taken with no seek on its way, and
    /// none sent since.
    pub fn shows(&self, reading: Option<u64>) -> bool {
        reading == Some(self.state().sent)
    }

    fn state(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::{seek_by, seek_to, Seeks};
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

    #[test]
    fn readings_wait_for_a_seek_to_land() {
        let seeks = Seeks::default();
        let before = seeks.reading();
        assert!(seeks.shows(before), "no seek: readings show");

        seeks.sent();
        let during = seeks.reading();
        assert!(!seeks.shows(before), "taken before the seek");
        assert!(!seeks.shows(during), "taken while it's on its way");

        seeks.landed();
        assert!(seeks.shows(seeks.reading()), "taken once it landed");
    }

    #[test]
    fn a_reading_taken_before_another_seek_is_dropped() {
        let seeks = Seeks::default();
        let reading = seeks.reading();
        seeks.sent();
        seeks.landed();
        assert!(!seeks.shows(reading));
    }

    #[test]
    fn a_seek_that_never_lands_lets_go() {
        let seeks = Seeks::with_landing(Duration::ZERO);
        seeks.sent();
        assert!(seeks.shows(seeks.reading()));
    }
}
