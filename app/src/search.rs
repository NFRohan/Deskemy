//! Search — titles across the library, and spoken subtitle text — as the
//! Tauri app's routes/search.

use crate::course_panel::model;
use crate::session::Db;
use crate::tracks::clock;
use crate::{AppWindow, Search, SearchRow, SubtitleRow, Theme};
use deskemy_core::db::queries;
use deskemy_core::domain::SearchHit;
use slint::ComponentHandle;

/// Results per kind, as the Tauri commands.
const LIMIT: i64 = 50;

/// Around matched words in subtitle snippets: characters subtitles don't use.
const OPEN: &str = "\u{2}";
const CLOSE: &str = "\u{3}";

/// Markdown for a snippet: its text escaped, matched words bold in `color`.
pub fn highlight(snippet: &str, color: &str) -> String {
    let escape = |text: &str| {
        text.chars()
            .map(|c| if c.is_ascii_punctuation() { format!("\\{c}") } else { c.to_string() })
            .collect::<String>()
    };
    let mut out = String::new();
    for (i, part) in snippet.split(OPEN).enumerate() {
        // Every part after the first starts inside a match.
        let (matched, rest) = match (i, part.split_once(CLOSE)) {
            (0, _) | (_, None) => ("", part),
            (_, Some((matched, rest))) => (matched, rest),
        };
        if !matched.is_empty() {
            out += &format!("<font color=\"{color}\">**{}**</font>", escape(matched));
        }
        out += &escape(rest);
    }
    out
}

/// The snippet as plain text (for screen readers).
pub fn plain(snippet: &str) -> String {
    snippet.replace(['\u{2}', '\u{3}'], "")
}

fn hex(color: slint::Color) -> String {
    format!("#{:02x}{:02x}{:02x}", color.red(), color.green(), color.blue())
}

/// A title hit's kind label and second line ("Lecture · Course").
pub fn describe(hit: &SearchHit) -> (&'static str, String) {
    let label = match hit.kind.as_str() {
        "course" => "Course",
        "section" => "Section",
        "attachment" => "Attachment",
        _ => "Lecture",
    };
    let context = if hit.kind == "course" {
        label.to_string()
    } else {
        format!("{label} · {}", hit.course_title)
    };
    (label, context)
}

/// Run a search and show its results (an empty query clears them).
pub fn run(ui: &AppWindow, db: &Db, query: &str) {
    let search = ui.global::<Search>();
    let query = query.trim();
    if query.is_empty() {
        search.set_results(model(Vec::new()));
        search.set_subtitles(model(Vec::new()));
        search.set_searched(false);
        return;
    }
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    let hits = queries::search(&conn, query, LIMIT).unwrap_or_else(|e| {
        tracing::warn!(error = %e, "search");
        Vec::new()
    });
    let spoken = queries::subtitle_search_marked(&conn, query, LIMIT, (OPEN, CLOSE)).unwrap_or_default();
    drop(conn);

    search.set_results(model(
        hits.iter()
            .map(|h| {
                let (_, context) = describe(h);
                SearchRow {
                    kind: h.kind.clone().into(),
                    id: h.entity_id.clone().into(),
                    course: h.course_id.clone().into(),
                    title: h.title.clone().into(),
                    context: context.into(),
                }
            })
            .collect(),
    ));
    let color = hex(ui.global::<Theme>().get_primary());
    search.set_subtitles(model(
        spoken
            .into_iter()
            .map(|s| SubtitleRow {
                styled: slint::StyledText::from_markdown(&highlight(&s.snippet, &color))
                    .unwrap_or_else(|_| slint::StyledText::from_plain_text(&plain(&s.snippet))),
                lecture: s.lecture_id.into(),
                time: clock(s.start_ms as f64 / 1000.0).into(),
                start: (s.start_ms / 1000) as f32,
                snippet: plain(&s.snippet).into(),
                context: format!("{} · {}", s.lecture_title, s.course_title).into(),
            })
            .collect(),
    ));
    search.set_searched(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(kind: &str) -> SearchHit {
        SearchHit {
            kind: kind.into(),
            entity_id: "e".into(),
            course_id: "c".into(),
            course_title: "AWS".into(),
            title: "t".into(),
        }
    }

    #[test]
    fn describes_hits_like_the_tauri_page() {
        assert_eq!(describe(&hit("course")).1, "Course");
        assert_eq!(describe(&hit("section")).1, "Section · AWS");
        assert_eq!(describe(&hit("attachment")).1, "Attachment · AWS");
        assert_eq!(describe(&hit("lecture")).1, "Lecture · AWS");
    }

    #[test]
    fn highlights_matches_and_escapes_the_rest() {
        let snippet = format!("…a [Music] {OPEN}pod{CLOSE} *runs* on {OPEN}k8s{CLOSE}");
        assert_eq!(
            highlight(&snippet, "#abcdef"),
            r##"…a \[Music\] <font color="#abcdef">**pod**</font> \*runs\* on <font color="#abcdef">**k8s**</font>"##
        );
        assert_eq!(plain(&snippet), "…a [Music] pod *runs* on k8s");
        // Unbalanced markers degrade to plain text.
        assert_eq!(highlight(&format!("x {OPEN}y"), "#000"), "x y");
        for s in [snippet.as_str(), "<b>not html</b> & 1 < 2"] {
            assert!(slint::StyledText::from_markdown(&highlight(s, "#abcdef")).is_ok(), "{s}");
        }
    }
}
