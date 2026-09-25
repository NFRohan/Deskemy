//! Search — titles across the library, and spoken subtitle text — as the
//! Tauri app's routes/search.

use crate::course_panel::model;
use crate::session::Db;
use crate::tracks::clock;
use crate::{AppWindow, Search, SearchRow, SubtitleRow};
use deskemy_core::db::queries;
use deskemy_core::domain::SearchHit;
use slint::ComponentHandle;

/// Results per kind, as the Tauri commands.
const LIMIT: i64 = 50;

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
    let spoken = queries::subtitle_search(&conn, query, LIMIT).unwrap_or_default();
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
    search.set_subtitles(model(
        spoken
            .into_iter()
            .map(|s| SubtitleRow {
                lecture: s.lecture_id.into(),
                time: clock(s.start_ms as f64 / 1000.0).into(),
                start: (s.start_ms / 1000) as f32,
                snippet: s.snippet.into(),
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
}
