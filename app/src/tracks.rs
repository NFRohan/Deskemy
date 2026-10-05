//! Audio / subtitle tracks, chapters and speeds, read from mpv and turned into
//! the overlay's menus. Labels follow the Tauri player's `trackLabel`.

use crate::MenuItem;
use deskemy_core::mpv::Mpv;

/// The speeds on offer, as in the Tauri player.
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Track {
    pub id: i64,
    pub lang: Option<String>,
    pub title: Option<String>,
    /// For external tracks (e.g. a sidecar .srt): the file's base name.
    pub filename: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Chapter {
    pub title: Option<String>,
    pub time: f64,
}

/// The chapters to show for the playing file. While busy (a cache refill, a
/// seek) mpv can leave the chapter count unanswered; read as "none", the
/// menus rebuilt then hid the Chapters button until the next rebuild (GH #6).
/// So for the same file an unanswered read (`answered` false) keeps the last
/// list; an answer — even 0 — is trusted, and a new file starts over.
pub fn steady_chapters(
    read: Vec<Chapter>,
    answered: bool,
    path: Option<&str>,
    known: &mut Option<(String, Vec<Chapter>)>,
) -> Vec<Chapter> {
    let Some(path) = path else { return read };
    match known {
        Some((p, list)) if p == path && !answered && !list.is_empty() => {
            tracing::debug!(path, kept = list.len(), "chapter count unanswered for the same file; keeping the list");
            list.clone()
        }
        _ => {
            *known = Some((path.to_string(), read.clone()));
            read
        }
    }
}

/// Everything the menus show, read from mpv in one go.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Tracks {
    pub audio: Vec<Track>,
    pub subtitles: Vec<Track>,
    pub chapters: Vec<Chapter>,
    /// mpv answered how many chapters there are (it doesn't while busy).
    pub chapters_known: bool,
    /// Active subtitle / audio track ids (None = off) and chapter (-1 = none).
    pub sid: Option<i64>,
    pub aid: Option<i64>,
    pub chapter: i64,
    pub speed: f64,
}

/// Cheap fingerprint polled every tick; the full lists are only re-read when
/// it changes (a file loaded, a sidecar subtitle attached, a pick was made).
pub fn signature(mpv: &Mpv) -> [Option<String>; 6] {
    ["track-list/count", "chapters", "sid", "aid", "chapter", "speed"]
        .map(|name| mpv.get_property_string(name))
}

pub fn read(mpv: &Mpv) -> Tracks {
    let mut tracks = Tracks {
        sid: mpv.get_i64("sid"),
        aid: mpv.get_i64("aid"),
        chapter: mpv.get_i64("chapter").unwrap_or(-1),
        speed: mpv.get_f64("speed").unwrap_or(1.0),
        ..Tracks::default()
    };
    let prop = |i: i64, key: &str| mpv.get_property_string(&format!("track-list/{i}/{key}"));
    for i in 0..mpv.get_i64("track-list/count").unwrap_or(0).max(0) {
        let track = Track {
            id: prop(i, "id").and_then(|v| v.parse().ok()).unwrap_or(0),
            lang: prop(i, "lang"),
            title: prop(i, "title"),
            filename: prop(i, "external-filename").and_then(|p| {
                std::path::Path::new(&p)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
            }),
        };
        match prop(i, "type").as_deref() {
            Some("audio") => tracks.audio.push(track),
            Some("sub") => tracks.subtitles.push(track),
            _ => {}
        }
    }
    let count = mpv.get_i64("chapters");
    tracks.chapters_known = count.is_some();
    tracks.chapters = (0..count.unwrap_or(0).max(0))
        .map(|i| Chapter {
            title: mpv.get_property_string(&format!("chapter-list/{i}/title")),
            time: mpv.get_f64(&format!("chapter-list/{i}/time")).unwrap_or(0.0),
        })
        .collect();
    tracks
}

/// A track's menu label. External subtitles show their file name so
/// different languages are distinguishable.
pub fn label(track: &Track) -> String {
    if let Some(file) = &track.filename {
        return match &track.lang {
            Some(lang) => format!("{lang} · {file}"),
            None => file.clone(),
        };
    }
    let parts: Vec<&str> = [&track.lang, &track.title].into_iter().flatten().map(String::as_str).collect();
    if parts.is_empty() {
        format!("Track {}", track.id)
    } else {
        parts.join(" · ")
    }
}

/// The preset `dir` steps from `current` (snapping an off-list speed to the
/// nearest preset first), clamped at both ends — as the Tauri player's `< / >`.
pub fn stepped_speed(current: f64, dir: i32) -> f64 {
    let nearest = SPEEDS
        .iter()
        .enumerate()
        .min_by(|a, b| (a.1 - current).abs().total_cmp(&(b.1 - current).abs()))
        .map_or(2, |(i, _)| i);
    SPEEDS[(nearest as i32 + dir).clamp(0, SPEEDS.len() as i32 - 1) as usize]
}

pub fn speed_label(speed: f64) -> String {
    format!("{speed}×")
}

fn item(id: i64, label: String, detail: String, selected: bool) -> MenuItem {
    MenuItem {
        id: id as i32,
        label: label.into(),
        detail: detail.into(),
        selected,
    }
}

/// "Subtitles off" first (id -1), then each subtitle track.
pub fn subtitle_menu(t: &Tracks) -> Vec<MenuItem> {
    std::iter::once(item(-1, "Subtitles off".into(), String::new(), t.sid.is_none()))
        .chain(t.subtitles.iter().map(|s| item(s.id, label(s), String::new(), t.sid == Some(s.id))))
        .collect()
}

pub fn audio_menu(t: &Tracks) -> Vec<MenuItem> {
    t.audio
        .iter()
        .map(|a| item(a.id, label(a), String::new(), t.aid == Some(a.id)))
        .collect()
}

pub fn chapter_menu(t: &Tracks) -> Vec<MenuItem> {
    t.chapters
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let title = c.title.clone().unwrap_or_else(|| format!("Chapter {}", i + 1));
            item(i as i64, title, clock(c.time), t.chapter == i as i64)
        })
        .collect()
}

/// Speed menu; ids index into [`SPEEDS`].
pub fn speed_menu(t: &Tracks) -> Vec<MenuItem> {
    SPEEDS
        .iter()
        .enumerate()
        .map(|(i, &s)| item(i as i64, speed_label(s), String::new(), (t.speed - s).abs() < 0.001))
        .collect()
}

/// `m:ss` / `h:mm:ss`, like the overlay's clock.
pub fn clock(seconds: f64) -> String {
    let s = seconds.max(0.0) as u64;
    match s / 3600 {
        0 => format!("{}:{:02}", s / 60, s % 60),
        h => format!("{h}:{:02}:{:02}", (s / 60) % 60, s % 60),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: i64, lang: Option<&str>, title: Option<&str>, filename: Option<&str>) -> Track {
        Track {
            id,
            lang: lang.map(Into::into),
            title: title.map(Into::into),
            filename: filename.map(Into::into),
        }
    }

    #[test]
    fn labels_match_the_tauri_player() {
        assert_eq!(label(&track(2, Some("en"), None, Some("01 Intro.en.srt"))), "en · 01 Intro.en.srt");
        assert_eq!(label(&track(2, None, None, Some("01 Intro.srt"))), "01 Intro.srt");
        assert_eq!(label(&track(1, Some("eng"), Some("Director"), None)), "eng · Director");
        assert_eq!(label(&track(3, None, None, None)), "Track 3");
    }

    #[test]
    fn subtitle_menu_leads_with_off_and_marks_the_active_track() {
        let t = Tracks {
            subtitles: vec![track(1, Some("en"), None, None), track(2, Some("fr"), None, None)],
            sid: Some(2),
            ..Tracks::default()
        };
        let menu = subtitle_menu(&t);
        let picked: Vec<(i32, bool)> = menu.iter().map(|m| (m.id, m.selected)).collect();
        assert_eq!(picked, vec![(-1, false), (1, false), (2, true)]);

        let off = subtitle_menu(&Tracks { sid: None, ..t });
        assert!(off[0].selected);
    }

    #[test]
    fn chapters_fall_back_to_numbered_titles_with_times() {
        let t = Tracks {
            chapters: vec![
                Chapter { title: Some("Intro".into()), time: 0.0 },
                Chapter { title: None, time: 3725.0 },
            ],
            chapter: 1,
            ..Tracks::default()
        };
        let menu = chapter_menu(&t);
        assert_eq!((menu[0].label.as_str(), menu[0].detail.as_str()), ("Intro", "0:00"));
        assert_eq!((menu[1].label.as_str(), menu[1].detail.as_str()), ("Chapter 2", "1:02:05"));
        assert!(menu[1].selected && !menu[0].selected);
    }

    #[test]
    fn speed_steps_through_the_presets_and_stops_at_the_ends() {
        assert_eq!(stepped_speed(1.0, 1), 1.25);
        assert_eq!(stepped_speed(1.0, -1), 0.75);
        assert_eq!(stepped_speed(2.0, 1), 2.0);
        assert_eq!(stepped_speed(0.5, -1), 0.5);
        // An off-list speed snaps to the nearest preset before stepping.
        assert_eq!(stepped_speed(1.1, 1), 1.25);
    }

    #[test]
    fn speed_menu_marks_the_current_speed() {
        let t = Tracks { speed: 1.25, ..Tracks::default() };
        let menu = speed_menu(&t);
        assert_eq!(menu.len(), SPEEDS.len());
        assert_eq!(menu.iter().filter(|m| m.selected).count(), 1);
        assert_eq!(menu[3].label.as_str(), "1.25×");
        assert!(menu[3].selected);
    }
    #[test]
    fn an_empty_chapter_read_keeps_the_files_chapters() {
        let ch = |t: f64| Chapter { title: Some(format!("at {t}")), time: t };
        let mut known = None;
        let list = vec![ch(0.0), ch(60.0)];
        assert_eq!(steady_chapters(list.clone(), true, Some("a.mp4"), &mut known), list);
        // mpv busy: count unanswered, same file -> the list stays.
        assert_eq!(steady_chapters(vec![], false, Some("a.mp4"), &mut known), list);
        // An answer of 0 for the same file is trusted.
        assert_eq!(steady_chapters(vec![], true, Some("a.mp4"), &mut known), vec![]);
        // A new file without chapters really has none.
        assert_eq!(steady_chapters(vec![], true, Some("b.mp4"), &mut known), vec![]);
        // Back to a.mp4 later: it's read afresh.
        assert_eq!(steady_chapters(vec![ch(5.0)], true, Some("a.mp4"), &mut known), vec![ch(5.0)]);
        // No path (nothing loaded): passed through.
        assert_eq!(steady_chapters(vec![], false, None, &mut known), vec![]);
    }
}
