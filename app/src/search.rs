//! Search — titles across the library, and spoken subtitle text — as the
//! Tauri app's routes/search.

use crate::course_panel::model;
use crate::session::Db;
use crate::tracks::clock;
use crate::{AppWindow, Search, SearchGroup, SearchRow, SubtitleRow, Theme};
use deskemy_core::db::queries;
use deskemy_core::domain::SearchHit;
use slint::{ComponentHandle, Model};

/// Results per kind, as the Tauri commands.
const LIMIT: i64 = 50;

/// The groups title hits show in, in this order: what each kind is called.
const GROUPS: [(&str, &str); 4] = [("course", "Courses"), ("section", "Sections"), ("lecture", "Lectures"), ("attachment", "Resources")];

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

/// Title hits by kind, in the groups' order, each best first; kinds with no
/// hits are left out.
pub fn groups(hits: &[SearchHit]) -> Vec<(&'static str, &'static str, Vec<&SearchHit>)> {
    GROUPS
        .iter()
        .map(|&(kind, name)| (kind, name, hits.iter().filter(|h| h.kind == kind).collect::<Vec<_>>()))
        .filter(|(_, _, hits)| !hits.is_empty())
        .collect()
}

/// A group's heading: "LECTURES · 12", or "· 50+" when there may be more.
pub fn heading(name: &str, count: usize) -> String {
    let more = if count as i64 >= LIMIT { "+" } else { "" };
    format!("{} · {count}{more}", name.to_uppercase())
}

/// A title hit's second line: its course (the group says what it is); none
/// for a course.
fn context(hit: &SearchHit) -> String {
    if hit.kind == "course" { String::new() } else { hit.course_title.clone() }
}

/// Show every hit of a group, not just its first few.
pub fn show_all(ui: &AppWindow, kind: &str) {
    let groups = ui.global::<Search>().get_groups();
    if let Some(i) = groups.iter().position(|g| g.kind == kind) {
        if let Some(group) = groups.row_data(i) {
            groups.set_row_data(i, SearchGroup { all: true, ..group });
        }
    }
}

/// Run a search and show its results (an empty query clears them).
pub fn run(ui: &AppWindow, db: &Db, query: &str) {
    let search = ui.global::<Search>();
    let query = query.trim();
    if query.is_empty() {
        search.set_groups(model(Vec::new()));
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

    search.set_groups(model(
        groups(&hits)
            .into_iter()
            .map(|(kind, name, hits)| SearchGroup {
                kind: kind.into(),
                heading: heading(name, hits.len()).into(),
                all: false,
                rows: model(
                    hits.into_iter()
                        .map(|h| SearchRow {
                            kind: h.kind.clone().into(),
                            id: h.entity_id.clone().into(),
                            course: h.course_id.clone().into(),
                            title: h.title.clone().into(),
                            context: context(h).into(),
                        })
                        .collect(),
                ),
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
        hit_titled(kind, "t")
    }

    fn hit_titled(kind: &str, title: &str) -> SearchHit {
        SearchHit {
            kind: kind.into(),
            entity_id: "e".into(),
            course_id: "c".into(),
            course_title: "AWS".into(),
            title: title.into(),
        }
    }

    #[test]
    fn groups_hits_by_kind_in_a_fixed_order_best_first() {
        // As the query returns them: by rank, kinds mixed.
        let hits = [
            hit_titled("lecture", "best lecture"),
            hit_titled("attachment", "notes"),
            hit_titled("section", "section"),
            hit_titled("lecture", "next lecture"),
        ];
        let grouped: Vec<(&str, Vec<&str>)> =
            groups(&hits).into_iter().map(|(kind, _, hits)| (kind, hits.iter().map(|h| h.title.as_str()).collect())).collect();
        assert_eq!(
            grouped,
            vec![
                ("section", vec!["section"]),
                ("lecture", vec!["best lecture", "next lecture"]),
                ("attachment", vec!["notes"]),
            ]
        );
    }

    #[test]
    fn headings_count_and_say_when_there_may_be_more() {
        assert_eq!(heading("Lectures", 12), "LECTURES · 12");
        assert_eq!(heading("Resources", LIMIT as usize), "RESOURCES · 50+");
    }

    #[test]
    fn a_hit_s_second_line_is_its_course() {
        assert_eq!(context(&hit("course")), "");
        assert_eq!(context(&hit("section")), "AWS");
        assert_eq!(context(&hit("attachment")), "AWS");
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
