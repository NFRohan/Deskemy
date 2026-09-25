//! The simple list pages — Favorites, History and Bookmarks — as the Tauri
//! app's routes/favorites, routes/history and routes/bookmarks.

use crate::course_panel::model;
use crate::library::pct;
use crate::session::Db;
use crate::tracks::clock;
use crate::{AppWindow, BookmarkEntry, BookmarkGroup, HistoryGroup, HistoryRow, Lists};
use chrono::{DateTime, Local, NaiveDate};
use deskemy_core::db::queries;
use deskemy_core::domain::{BookmarkDetail, HistoryEntry};
use slint::ComponentHandle;

/// How much history the page shows, as the Tauri app.
const HISTORY_LIMIT: i64 = 500;

fn local(unix: i64) -> DateTime<Local> {
    DateTime::from_timestamp(unix, 0).unwrap_or_default().with_timezone(&Local)
}

/// "Today", "Yesterday", else e.g. "Tue, Sep 23, 2026" — as `formatDayGroup`.
pub fn day_group(day: NaiveDate, today: NaiveDate) -> String {
    match (today - day).num_days() {
        ..=0 => "Today".into(),
        1 => "Yesterday".into(),
        _ => day.format("%a, %b %-d, %Y").to_string(),
    }
}

/// A history entry's progress: 100 when done, else how far in (None when
/// the duration is unknown) — as the Tauri page.
pub fn progress(h: &HistoryEntry) -> Option<i32> {
    if h.completed {
        return Some(100);
    }
    h.duration
        .filter(|d| *d > 0.0)
        .map(|d| pct(h.position_seconds.round() as i64, d.round() as i64).min(100))
}

/// Group history by local day, keeping the newest-first order.
pub fn history_groups(items: &[HistoryEntry], today: NaiveDate) -> Vec<(String, Vec<&HistoryEntry>)> {
    let mut groups: Vec<(String, Vec<&HistoryEntry>)> = Vec::new();
    for h in items {
        let label = day_group(local(h.last_watched_at).date_naive(), today);
        match groups.last_mut() {
            Some((last, entries)) if *last == label => entries.push(h),
            _ => groups.push((label, vec![h])),
        }
    }
    groups
}

/// Group bookmarks by course, keeping the order they came in.
pub fn bookmark_groups(items: &[BookmarkDetail]) -> Vec<(&str, &str, Vec<&BookmarkDetail>)> {
    let mut groups: Vec<(&str, &str, Vec<&BookmarkDetail>)> = Vec::new();
    for b in items {
        match groups.iter_mut().find(|(id, _, _)| *id == b.course_id) {
            Some((_, _, entries)) => entries.push(b),
            None => groups.push((&b.course_id, &b.course_title, vec![b])),
        }
    }
    groups
}

/// Load a list page's data when it's shown.
pub fn show(ui: &AppWindow, db: &Db, page: &str) {
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    let lists = ui.global::<Lists>();
    match page {
        "history" => {
            let items = queries::list_history(&conn, HISTORY_LIMIT).unwrap_or_else(|e| {
                tracing::error!(error = %e, "history");
                Vec::new()
            });
            let today = Local::now().date_naive();
            let groups = history_groups(&items, today)
                .into_iter()
                .map(|(label, entries)| HistoryGroup {
                    label: label.into(),
                    items: model(
                        entries
                            .into_iter()
                            .map(|h| HistoryRow {
                                lecture: h.lecture_id.clone().into(),
                                time: local(h.last_watched_at).format("%H:%M").to_string().into(),
                                title: h.lecture_title.clone().into(),
                                context: format!("{} · {}", h.course_title, h.section_title).into(),
                                completed: h.completed,
                                percent: progress(h).unwrap_or(-1),
                                // Watch again from the start once it's done.
                                start: if h.completed { 0.0 } else { h.position_seconds.floor() as f32 },
                            })
                            .collect(),
                    ),
                })
                .collect();
            lists.set_history(model(groups));
        }
        "bookmarks" => {
            let items = queries::list_all_bookmarks(&conn).unwrap_or_else(|e| {
                tracing::error!(error = %e, "bookmarks");
                Vec::new()
            });
            let groups = bookmark_groups(&items)
                .into_iter()
                .map(|(course, title, entries)| BookmarkGroup {
                    course: course.into(),
                    title: title.into(),
                    items: model(
                        entries
                            .into_iter()
                            .map(|b| BookmarkEntry {
                                id: b.id.clone().into(),
                                lecture: b.lecture_id.clone().into(),
                                time: clock(b.position_seconds).into(),
                                title: b.label.clone().unwrap_or_else(|| b.lecture_title.clone()).into(),
                                context: format!("{} · {}", b.section_title, b.lecture_title).into(),
                                position: b.position_seconds as f32,
                            })
                            .collect(),
                    ),
                })
                .collect();
            lists.set_bookmarks(model(groups));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(id: &str, at: i64, completed: bool, position: f64, duration: Option<f64>) -> HistoryEntry {
        HistoryEntry {
            lecture_id: id.into(),
            lecture_title: id.into(),
            section_title: "S".into(),
            course_id: "c".into(),
            course_title: "C".into(),
            position_seconds: position,
            duration,
            completed,
            last_watched_at: at,
        }
    }

    #[test]
    fn day_groups_read_like_the_tauri_app() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 25).unwrap();
        assert_eq!(day_group(today, today), "Today");
        assert_eq!(day_group(today.pred_opt().unwrap(), today), "Yesterday");
        assert_eq!(day_group(NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(), today), "Tue, Sep 1, 2026");
    }

    #[test]
    fn progress_is_done_or_how_far_in() {
        assert_eq!(progress(&entry("a", 0, true, 5.0, Some(100.0))), Some(100));
        assert_eq!(progress(&entry("a", 0, false, 25.0, Some(100.0))), Some(25));
        assert_eq!(progress(&entry("a", 0, false, 25.0, None)), None);
    }

    #[test]
    fn history_groups_by_day_in_order() {
        let now = Local::now();
        let today = now.date_naive();
        let t = now.timestamp();
        let day = 24 * 3600;
        let items = vec![entry("a", t, false, 0.0, None), entry("b", t, false, 0.0, None), entry("c", t - 3 * day, false, 0.0, None)];
        let groups: Vec<(String, Vec<&str>)> = history_groups(&items, today)
            .into_iter()
            .map(|(l, e)| (l, e.into_iter().map(|h| h.lecture_id.as_str()).collect()))
            .collect();
        assert_eq!(groups[0], ("Today".to_string(), vec!["a", "b"]));
        assert_eq!(groups[1].1, vec!["c"]);
    }

    #[test]
    fn bookmarks_group_by_course_in_order() {
        let mark = |id: &str, course: &str| BookmarkDetail {
            id: id.into(),
            lecture_id: "l".into(),
            lecture_title: "L".into(),
            section_title: "S".into(),
            course_id: course.into(),
            course_title: course.to_uppercase(),
            position_seconds: 1.0,
            label: None,
            created_at: 0,
        };
        let items = vec![mark("1", "a"), mark("2", "b"), mark("3", "a")];
        let groups: Vec<(&str, Vec<&str>)> = bookmark_groups(&items)
            .into_iter()
            .map(|(_, title, e)| (title, e.into_iter().map(|b| b.id.as_str()).collect()))
            .collect();
        assert_eq!(groups, vec![("A", vec!["1", "3"]), ("B", vec!["2"])]);
    }
}
