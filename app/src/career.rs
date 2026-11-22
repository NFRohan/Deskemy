//! Career tracks — ordered learning paths of courses — as the Tauri app's
//! routes/tracks and routes/tracks/[id].

use crate::course_panel::model;
use crate::library::{format_duration, pct, LibraryPage};
use crate::session::Db;
use crate::{AppWindow, Nav, PickRow, TrackCard, TrackCourseRow, Tracks};
use deskemy_core::db::queries;
use deskemy_core::domain::{CourseSummary, TrackCourse};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

/// Every lecture in the course is done.
pub fn is_done(c: &TrackCourse) -> bool {
    c.lecture_count > 0 && c.completed_lectures >= c.lecture_count
}

/// "done", "started" or "" (not started), for the row's status icon.
pub fn status(c: &TrackCourse) -> &'static str {
    if is_done(c) {
        "done"
    } else if c.completed_lectures > 0 {
        "started"
    } else {
        ""
    }
}

/// The course to work on next: the first one not yet finished.
pub fn next_index(courses: &[TrackCourse]) -> Option<usize> {
    courses.iter().position(|c| !is_done(c))
}

/// The order after moving the course at `i` one step (`dir` -1 = up,
/// 1 = down), or None when it's already at that end.
pub fn moved(ids: &[String], i: usize, dir: i32) -> Option<Vec<String>> {
    let j = i.checked_add_signed(dir as isize)?;
    if i >= ids.len() || j >= ids.len() {
        return None;
    }
    let mut order = ids.to_vec();
    order.swap(i, j);
    Some(order)
}

/// Library courses that could be added: not already in the track, and
/// matching the filter (case-insensitive, anywhere in the title).
pub fn pickable<'a>(library: &'a [CourseSummary], in_track: &HashSet<&str>, query: &str) -> Vec<&'a CourseSummary> {
    let query = query.trim().to_lowercase();
    library
        .iter()
        .filter(|c| !in_track.contains(c.id.as_str()) && c.title.to_lowercase().contains(&query))
        .collect()
}

/// "1 course", "3 courses".
pub fn count(n: i64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// "245 lectures · 21h 30m": how many, and how long when known.
pub fn course_meta(c: &TrackCourse) -> String {
    let mut text = count(c.lecture_count, "lecture");
    if c.total_seconds > 0.0 {
        text += &format!(" · {}", format_duration(Some(c.total_seconds)));
    }
    text
}

/// "~18h 40m left", or "" when nothing's left or the durations are unknown.
pub fn time_left(seconds: f64) -> String {
    if seconds < 1.0 { String::new() } else { format!("~{} left", format_duration(Some(seconds))) }
}

/// Where a track stands: "Not started", "In progress" or "Completed".
pub fn track_state(courses: &[TrackCourse]) -> &'static str {
    if !courses.is_empty() && courses.iter().all(is_done) {
        "Completed"
    } else if courses.iter().any(|c| c.completed_lectures > 0) {
        "In progress"
    } else {
        "Not started"
    }
}

/// A blank description is no description.
fn optional(text: &str) -> Option<&str> {
    Some(text.trim()).filter(|t| !t.is_empty())
}

/// The open track's state (UI thread only). Thumbnails come from the
/// library's cache, and so does the course list the picker offers.
pub struct TracksPage {
    db: Db,
    library: Rc<LibraryPage>,
    /// Where header images are stored (the thumbnail cache), if anywhere.
    thumbs_dir: Option<std::path::PathBuf>,
    id: RefCell<Option<String>>,
    /// The open track's courses, in order (what Move up / down rewrites).
    order: RefCell<Vec<String>>,
}

impl TracksPage {
    pub fn new(db: Db, library: Rc<LibraryPage>, thumbs_dir: Option<std::path::PathBuf>) -> Rc<Self> {
        Rc::new(TracksPage { db, library, thumbs_dir, id: RefCell::new(None), order: RefCell::new(Vec::new()) })
    }

    /// Pick a header image for the open track: copied into the thumbnail
    /// cache, as course covers are.
    pub fn pick_header(&self, ui: &AppWindow) {
        let Some(id) = self.id.borrow().clone() else { return };
        let file = match crate::dialogs::pick("track header", || {
            rfd::FileDialog::new()
                .set_title("Choose a header image")
                .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                .set_parent(&ui.window().window_handle())
                .pick_file()
        }) {
            crate::dialogs::Picked::Chosen(file) => file,
            crate::dialogs::Picked::Cancelled => return,
            crate::dialogs::Picked::Unavailable(why) => {
                ui.global::<Tracks>().set_header_error(why.into());
                return;
            }
        };
        let stored = (|| -> Result<(), String> {
            let dir = self.thumbs_dir.as_ref().ok_or("no data directory to store the image in")?;
            let bytes = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            let ext = file.extension().and_then(|e| e.to_str());
            let path = deskemy_core::thumbnails::store(dir, &bytes, ext).map_err(|e| e.to_string())?;
            queries::set_track_header(&self.conn(), &id, Some(&path.to_string_lossy())).map_err(|e| e.to_string())
        })();
        let page = ui.global::<Tracks>();
        match stored {
            Ok(()) => page.set_header_error("".into()),
            Err(e) => {
                tracing::warn!(error = %e, "track header");
                page.set_header_error(e.into());
            }
        }
        self.refresh(ui);
    }

    /// No header image for the open track (the file goes with the next
    /// thumbnail clean-up).
    pub fn clear_header(&self, ui: &AppWindow) {
        let Some(id) = self.id.borrow().clone() else { return };
        if let Err(e) = queries::set_track_header(&self.conn(), &id, None) {
            tracing::warn!(error = %e, "clear track header");
        }
        ui.global::<Tracks>().set_header_error("".into());
        self.refresh(ui);
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, deskemy_core::db::Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Load the tracks list.
    pub fn show_list(&self, ui: &AppWindow) {
        let tracks = queries::list_tracks(&self.conn()).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "list tracks");
            Vec::new()
        });
        let page = ui.global::<Tracks>();
        page.set_any_header(tracks.iter().any(|t| t.header_path.is_some()));
        page.set_tracks(model(
            tracks
                .into_iter()
                .map(|t| TrackCard {
                    header: self.library.image(t.header_path.as_deref()),
                    has_header: t.header_path.is_some(),
                    id: t.id.into(),
                    name: t.name.into(),
                    description: t.description.unwrap_or_default().into(),
                    meta: format!(
                        "{} · {} / {} lectures",
                        count(t.course_count, "course"),
                        t.completed_lectures,
                        t.total_lectures
                    )
                    .into(),
                    percent: pct(t.completed_lectures, t.total_lectures),
                })
                .collect(),
        ));
    }

    /// Create a track and open it.
    pub fn create(&self, ui: &AppWindow, name: &str, description: &str) {
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let tracks = ui.global::<Tracks>();
        // Its own statement: a guard in a `match` scrutinee lives to the end of
        // the match, and `open` locks the database again (it hung the app).
        let created = queries::create_track(&self.conn(), name, optional(description));
        match created {
            Ok(id) => {
                tracks.set_dialog("".into());
                self.open(ui, &id);
            }
            Err(e) => tracks.set_error(e.to_string().into()),
        }
    }

    pub fn open(&self, ui: &AppWindow, id: &str) {
        crate::history::visit(crate::history::Place::Track(id.to_string()));
        // Used: to the top of the list.
        if let Err(e) = queries::touch_track(&self.conn(), id) {
            tracing::warn!(error = %e, "touch track");
        }
        *self.id.borrow_mut() = Some(id.to_string());
        ui.global::<Tracks>().set_header_error("".into());
        ui.global::<Nav>().set_page("track".into());
        self.refresh(ui);
    }

    /// Re-read and redraw the open track (after an edit, or watching).
    pub fn refresh(&self, ui: &AppWindow) {
        let Some(id) = self.id.borrow().clone() else { return };
        let detail = queries::get_track(&self.conn(), &id).ok().flatten();
        let page = ui.global::<Tracks>();
        let nav = ui.global::<Nav>();
        nav.set_crumb_targets(model(vec!["tracks".into(), "".into()]));
        let Some(t) = detail else {
            page.set_found(false);
            nav.set_crumbs(model(vec!["Career Tracks".into(), "Track".into()]));
            return;
        };
        nav.set_crumbs(model(vec!["Career Tracks".into(), t.name.clone().into()]));

        let done: i64 = t.courses.iter().map(|c| c.completed_lectures).sum();
        let total: i64 = t.courses.iter().map(|c| c.lecture_count).sum();
        let percent = pct(done, total);
        let next = next_index(&t.courses);
        page.set_found(true);
        page.set_id(t.id.clone().into());
        page.set_name(t.name.clone().into());
        page.set_description(t.description.clone().unwrap_or_default().into());
        page.set_header(self.library.image(t.header_path.as_deref()));
        page.set_has_header(t.header_path.is_some());
        page.set_percent(percent);
        page.set_summary(format!("{percent}% · {done} / {total} lectures").into());
        page.set_count_label(count(t.courses.len() as i64, "course").to_uppercase().into());
        let seconds: f64 = t.courses.iter().map(|c| c.total_seconds).sum();
        let left: f64 = t.courses.iter().map(|c| c.remaining_seconds).sum();
        let mut meta = format!("{} · {}", count(t.courses.len() as i64, "course"), count(total, "lecture"));
        if seconds > 0.0 {
            meta += &format!(" · {}", format_duration(Some(seconds)));
        }
        page.set_meta(meta.into());
        page.set_state(track_state(&t.courses).into());
        page.set_lectures_done(done as i32);
        page.set_lectures_total(total as i32);
        page.set_courses_done(t.courses.iter().filter(|c| is_done(c)).count() as i32);
        // "—" when no durations are known; "0m" once nothing's left.
        let left_label = if left >= 1.0 {
            format_duration(Some(left))
        } else if seconds > 0.0 {
            "0m".into()
        } else {
            "—".into()
        };
        page.set_time_left(left_label.into());
        // The course to work on next, and the lecture it resumes at.
        page.set_next_index(next.map_or(-1, |i| i as i32));
        let resume = next.and_then(|i| {
            let c = &t.courses[i];
            let detail = queries::get_course_detail(&self.conn(), &c.id).ok().flatten()?;
            let lecture = crate::course_page::next_lecture(&detail)?;
            // Started: something finished, or left off partway.
            let started = c.completed_lectures > 0 || detail.last_lecture_id.is_some();
            Some((lecture.id.clone(), lecture.title.clone(), started, c.remaining_seconds))
        });
        let (lecture, lecture_title, started, course_left) = resume.unwrap_or_default();
        page.set_next_lecture(lecture.into());
        page.set_next_lecture_title(lecture_title.into());
        page.set_next_label(if started { "Continue" } else { "Start course" }.into());
        page.set_next_left(time_left(course_left).into());
        page.set_courses(model(
            t.courses
                .iter()
                .enumerate()
                .map(|(i, c)| TrackCourseRow {
                    id: c.id.clone().into(),
                    title: c.title.clone().into(),
                    thumbnail: self.library.image(c.thumbnail_path.as_deref()),
                    has_thumbnail: c.thumbnail_path.is_some(),
                    done: c.completed_lectures as i32,
                    total: c.lecture_count as i32,
                    percent: pct(c.completed_lectures, c.lecture_count),
                    status: status(c).into(),
                    next: next == Some(i),
                    number: i as i32 + 1,
                    meta: course_meta(c).into(),
                })
                .collect(),
        ));
        *self.order.borrow_mut() = t.courses.into_iter().map(|c| c.id).collect();
    }

    pub fn save(&self, ui: &AppWindow, name: &str, description: &str) {
        let Some(id) = self.id.borrow().clone() else { return };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        let tracks = ui.global::<Tracks>();
        match queries::update_track(&self.conn(), &id, name, optional(description)) {
            Ok(()) => tracks.set_dialog("".into()),
            Err(e) => tracks.set_error(e.to_string().into()),
        }
        self.refresh(ui);
    }

    /// Delete the open track (only the grouping — courses are untouched) and
    /// go back to the list.
    pub fn delete(&self, ui: &AppWindow) {
        let Some(id) = self.id.borrow_mut().take() else { return };
        if let Err(e) = queries::delete_track(&self.conn(), &id) {
            tracing::warn!(error = %e, "delete track");
        }
        ui.global::<Tracks>().set_dialog("".into());
        let nav = ui.global::<Nav>();
        nav.set_page("tracks".into());
        nav.set_crumbs(model(vec!["Career Tracks".into()]));
        nav.set_crumb_targets(model(vec!["".into()]));
        self.show_list(ui);
    }

    /// Fill the "Add courses" picker for a filter.
    pub fn filter(&self, ui: &AppWindow, query: &str) {
        let courses = self.library.courses();
        let order = self.order.borrow();
        let in_track: HashSet<&str> = order.iter().map(String::as_str).collect();
        let rows = pickable(&courses, &in_track, query)
            .into_iter()
            .map(|c| PickRow {
                id: c.id.clone().into(),
                title: c.title.clone().into(),
                lectures: count(c.lecture_count, "lecture").into(),
                thumbnail: self.library.image(c.thumbnail_path.as_deref()),
                has_thumbnail: c.thumbnail_path.is_some(),
            })
            .collect();
        let tracks = ui.global::<Tracks>();
        tracks.set_pickable(model(rows));
        tracks.set_pick_empty(
            if courses.is_empty() {
                "No courses in your library yet."
            } else {
                "Every matching course is already in this track."
            }
            .into(),
        );
    }

    /// Append a course; it leaves the picker, which stays open for more.
    pub fn add(&self, ui: &AppWindow, course: &str) {
        let Some(id) = self.id.borrow().clone() else { return };
        if let Err(e) = queries::add_course_to_track(&self.conn(), &id, course) {
            tracing::warn!(error = %e, "add course to track");
        }
        self.refresh(ui);
        self.filter(ui, &ui.global::<Tracks>().get_pick_query());
    }

    pub fn remove(&self, ui: &AppWindow, course: &str) {
        let Some(id) = self.id.borrow().clone() else { return };
        if let Err(e) = queries::remove_course_from_track(&self.conn(), &id, course) {
            tracing::warn!(error = %e, "remove course from track");
        }
        self.refresh(ui);
    }

    /// Move the course at `index` up (-1) or down (1) the path.
    pub fn move_course(&self, ui: &AppWindow, index: usize, dir: i32) {
        let Some(id) = self.id.borrow().clone() else { return };
        let Some(order) = moved(&self.order.borrow(), index, dir) else { return };
        if let Err(e) = queries::reorder_track_courses(&self.conn(), &id, &order) {
            tracing::warn!(error = %e, "reorder track");
        }
        self.refresh(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn course(id: &str, done: i64, total: i64) -> TrackCourse {
        TrackCourse {
            id: id.into(),
            title: id.into(),
            thumbnail_path: None,
            lecture_count: total,
            completed_lectures: done,
            total_seconds: 0.0,
            remaining_seconds: 0.0,
        }
    }

    #[test]
    fn a_track_is_not_started_in_progress_or_completed() {
        assert_eq!(track_state(&[]), "Not started");
        assert_eq!(track_state(&[course("a", 0, 5), course("b", 0, 5)]), "Not started");
        assert_eq!(track_state(&[course("a", 5, 5), course("b", 0, 5)]), "In progress");
        assert_eq!(track_state(&[course("a", 5, 5), course("b", 5, 5)]), "Completed");
    }

    #[test]
    fn course_meta_and_time_left_leave_out_unknown_durations() {
        let mut c = course("a", 1, 245);
        assert_eq!(course_meta(&c), "245 lectures");
        c.total_seconds = 77_400.0;
        assert_eq!(course_meta(&c), "245 lectures · 21h 30m");
        assert_eq!(time_left(0.0), "");
        assert_eq!(time_left(67_200.0), "~18h 40m left");
    }

    #[test]
    fn next_is_the_first_unfinished_course() {
        let courses = vec![course("linux", 10, 10), course("docker", 3, 20), course("k8s", 0, 30)];
        assert_eq!(next_index(&courses), Some(1));
        assert_eq!(status(&courses[0]), "done");
        assert_eq!(status(&courses[1]), "started");
        assert_eq!(status(&courses[2]), "");
        // An empty course isn't "done", so it's next.
        assert_eq!(next_index(&[course("linux", 10, 10), course("empty", 0, 0)]), Some(1));
        assert_eq!(next_index(&[course("linux", 10, 10)]), None);
    }

    #[test]
    fn moves_one_step_and_stops_at_the_ends() {
        let ids: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        assert_eq!(moved(&ids, 1, -1).unwrap(), ["b", "a", "c"]);
        assert_eq!(moved(&ids, 1, 1).unwrap(), ["a", "c", "b"]);
        assert_eq!(moved(&ids, 0, -1), None);
        assert_eq!(moved(&ids, 2, 1), None);
        assert_eq!(moved(&ids, 5, -1), None);
    }

    #[test]
    fn picker_hides_members_and_filters_by_title() {
        let summary = |id: &str, title: &str| CourseSummary {
            id: id.into(),
            title: title.into(),
            folder_path: String::new(),
            thumbnail_path: None,
            lecture_count: 1,
            total_duration: None,
            is_favorite: false,
            scan_status: "Ready".into(),
            last_opened_at: None,
            completed_count: 0,
            resume_thumbnail_path: None,
            last_lecture_id: None,
            last_lecture_title: None,
            tags: Vec::new(),
        };
        let library = vec![summary("1", "Linux Basics"), summary("2", "Docker Deep Dive"), summary("3", "Docker Swarm")];
        let members: HashSet<&str> = ["2"].into_iter().collect();
        let ids = |q: &str| pickable(&library, &members, q).into_iter().map(|c| c.id.as_str()).collect::<Vec<_>>();
        assert_eq!(ids(""), ["1", "3"]);
        assert_eq!(ids("  docker "), ["3"]);
        assert!(ids("kubernetes").is_empty());
    }

    #[test]
    fn counts_read_naturally() {
        assert_eq!(count(1, "course"), "1 course");
        assert_eq!(count(0, "course"), "0 courses");
        assert_eq!(count(12, "lecture"), "12 lectures");
    }
}
