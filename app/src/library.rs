//! The library list (placeholder until the real home page is ported).

use crate::session::Db;
use crate::{AppWindow, CourseRow};
use deskemy_core::db::queries;

/// (Re)load the course list into the window.
pub fn show(ui: &AppWindow, db: &Db) {
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    match queries::list_course_summaries(&conn) {
        Ok(courses) => {
            ui.set_status(format!("{} courses", courses.len()).into());
            let rows: Vec<CourseRow> = courses
                .into_iter()
                .map(|c| CourseRow {
                    id: c.id.into(),
                    resume: c.last_lecture_id.unwrap_or_default().into(),
                    title: c.title.into(),
                    lectures: c.lecture_count as i32,
                    completed: c.completed_count as i32,
                })
                .collect();
            ui.set_courses(slint::ModelRc::new(slint::VecModel::from(rows)));
        }
        Err(e) => {
            tracing::error!(error = %e, "list courses");
            ui.set_status(format!("Could not read the library: {e}").into());
        }
    }
}

/// Lectures to try when opening a course, best first: where it was left off
/// (that lecture may have vanished in a rescan), then the first lecture.
pub fn lectures_to_open(db: &Db, row: &CourseRow) -> Vec<String> {
    let mut ids = Vec::new();
    if !row.resume.is_empty() {
        ids.push(row.resume.to_string());
    }
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((first, _)) = queries::list_course_playlist(&conn, &row.id)
        .ok()
        .and_then(|items| items.into_iter().next())
    {
        if !ids.contains(&first) {
            ids.push(first);
        }
    }
    ids
}
