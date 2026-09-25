//! The Library home — Continue Watching, filters, sorting and the course grid
//! — following the Tauri app's `routes/+page.svelte` and `CourseCard.svelte`.

use crate::session::Db;
use crate::{AppWindow, CourseCard, Hero, Library, SelectOption};
use deskemy_core::db::queries;
use deskemy_core::domain::CourseSummary;
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;

/// "Recently watched" = opened within the last 14 days.
const RECENT_WINDOW_S: i64 = 14 * 24 * 3600;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Sort {
    Recent,
    Alpha,
    Progress,
    Duration,
}

impl Sort {
    pub fn parse(s: &str) -> Self {
        match s {
            "alpha" => Sort::Alpha,
            "progress" => Sort::Progress,
            "duration" => Sort::Duration,
            _ => Sort::Recent,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Status {
    All,
    Recent,
    Progress,
    Finished,
    New,
    Favorites,
}

impl Status {
    pub fn parse(s: &str) -> Self {
        match s {
            "recent" => Status::Recent,
            "progress" => Status::Progress,
            "finished" => Status::Finished,
            "new" => Status::New,
            "favorites" => Status::Favorites,
            _ => Status::All,
        }
    }
}

pub struct Filters {
    pub query: String,
    pub sort: Sort,
    pub status: Status,
    pub tag: Option<String>,
    /// Course ids in the selected career track, if one is selected.
    pub track: Option<HashSet<String>>,
}

/// Rounded percentage, as the Tauri app's `pct`.
pub fn pct(done: i64, total: i64) -> i32 {
    if total <= 0 {
        0
    } else {
        ((done as f64 / total as f64) * 100.0).round() as i32
    }
}

/// "3h 12m" / "45m" / "30s" / "—", as the Tauri app's `formatDuration`.
pub fn format_duration(seconds: Option<f64>) -> String {
    let Some(s) = seconds.filter(|s| *s > 0.0) else { return "—".into() };
    let s = s.floor() as i64;
    let (h, m) = (s / 3600, (s % 3600) / 60);
    if h > 0 {
        format!("{h}h {m}m")
    } else if m > 0 {
        format!("{m}m")
    } else {
        format!("{s}s")
    }
}

fn finished(c: &CourseSummary) -> bool {
    c.lecture_count > 0 && c.completed_count >= c.lecture_count
}

fn started(c: &CourseSummary) -> bool {
    c.completed_count > 0 || c.last_opened_at.is_some()
}

fn matches_status(c: &CourseSummary, status: Status, now: i64) -> bool {
    match status {
        Status::All => true,
        Status::Favorites => c.is_favorite,
        Status::Recent => c.last_opened_at.is_some_and(|t| now - t <= RECENT_WINDOW_S),
        Status::Finished => finished(c),
        Status::Progress => !finished(c) && started(c),
        Status::New => !finished(c) && !started(c),
    }
}

/// The most recently opened course that isn't finished — Continue Watching.
pub fn hero(courses: &[CourseSummary]) -> Option<&CourseSummary> {
    courses
        .iter()
        .filter(|c| c.last_opened_at.is_some() && c.completed_count < c.lecture_count)
        .max_by_key(|c| c.last_opened_at)
}

pub fn all_tags(courses: &[CourseSummary]) -> Vec<String> {
    let mut tags: Vec<String> = courses
        .iter()
        .flat_map(|c| c.tags.iter().cloned())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    tags.sort();
    tags
}

/// The grid's courses: filtered, then explicitly sorted.
pub fn filter<'a>(courses: &'a [CourseSummary], f: &Filters, now: i64) -> Vec<&'a CourseSummary> {
    let query = f.query.to_lowercase();
    let mut list: Vec<&CourseSummary> = courses
        .iter()
        .filter(|c| c.title.to_lowercase().contains(&query))
        .filter(|c| f.tag.as_ref().is_none_or(|t| c.tags.contains(t)))
        .filter(|c| f.track.as_ref().is_none_or(|ids| ids.contains(&c.id)))
        .filter(|c| matches_status(c, f.status, now))
        .collect();
    let title = |c: &CourseSummary| c.title.to_lowercase();
    match f.sort {
        // Last opened first; never-opened courses last, alphabetically.
        Sort::Recent => list.sort_by(|a, b| {
            b.last_opened_at.unwrap_or(0).cmp(&a.last_opened_at.unwrap_or(0)).then_with(|| title(a).cmp(&title(b)))
        }),
        Sort::Alpha => list.sort_by_key(|c| title(c)),
        Sort::Progress => list.sort_by_key(|c| -pct(c.completed_count, c.lecture_count)),
        Sort::Duration => list.sort_by(|a, b| {
            b.total_duration.unwrap_or(0.0).total_cmp(&a.total_duration.unwrap_or(0.0))
        }),
    }
    list
}

/// Holds the course list and loaded thumbnails; turns filter changes into
/// the page's cards. UI thread only.
pub struct LibraryPage {
    db: Db,
    courses: RefCell<Vec<CourseSummary>>,
    /// Career tracks as (id, name), for the filter's label.
    tracks: RefCell<Vec<(String, String)>>,
    images: RefCell<HashMap<String, slint::Image>>,
}

impl LibraryPage {
    pub fn new(db: Db) -> Rc<Self> {
        Rc::new(LibraryPage {
            db,
            courses: RefCell::new(Vec::new()),
            tracks: RefCell::new(Vec::new()),
            images: RefCell::new(HashMap::new()),
        })
    }

    /// Re-read courses and tracks from the database, then re-apply filters.
    pub fn reload(&self, ui: &AppWindow) {
        let (courses, tracks) = {
            let conn = self.db.lock().unwrap_or_else(|e| e.into_inner());
            let courses = queries::list_course_summaries(&conn).unwrap_or_else(|e| {
                tracing::error!(error = %e, "list courses");
                Vec::new()
            });
            (courses, queries::list_tracks(&conn).unwrap_or_default())
        };
        let lib = ui.global::<Library>();
        lib.set_tags(model(all_tags(&courses).into_iter().map(Into::into).collect()));
        let tracks: Vec<(String, String)> = tracks.into_iter().map(|t| (t.id, t.name)).collect();
        let options = std::iter::once((String::new(), "All tracks".to_string()))
            .chain(tracks.iter().cloned())
            .map(|(value, label)| SelectOption { value: value.into(), label: label.into() })
            .collect();
        lib.set_track_options(model(options));
        *self.tracks.borrow_mut() = tracks;
        lib.set_total(courses.len() as i32);
        *self.courses.borrow_mut() = courses;
        self.apply(ui);
    }

    /// Recompute the hero and the grid from the current filters.
    pub fn apply(&self, ui: &AppWindow) {
        let lib = ui.global::<Library>();
        let track = lib.get_track();
        let track_name = self
            .tracks
            .borrow()
            .iter()
            .find(|(id, _)| id.as_str() == track.as_str())
            .map(|(_, name)| name.clone());
        lib.set_track_label(track_name.unwrap_or_else(|| "All tracks".into()).into());
        let filters = Filters {
            query: lib.get_query().to_string(),
            sort: Sort::parse(&lib.get_sort()),
            status: Status::parse(&lib.get_status()),
            tag: Some(lib.get_tag().to_string()).filter(|t| !t.is_empty()),
            track: (!track.is_empty()).then(|| self.track_courses(&track)),
        };
        let now = chrono::Utc::now().timestamp();
        let courses = self.courses.borrow();

        lib.set_hero(match hero(&courses) {
            Some(c) => {
                let image = c.resume_thumbnail_path.as_deref().or(c.thumbnail_path.as_deref());
                Hero {
                    visible: true,
                    id: c.id.clone().into(),
                    resume: c.last_lecture_id.clone().unwrap_or_default().into(),
                    course: c.title.clone().into(),
                    lecture: c.last_lecture_title.clone().unwrap_or_else(|| c.title.clone()).into(),
                    resuming: c.last_lecture_title.is_some(),
                    percent: pct(c.completed_count, c.lecture_count),
                    completed: c.completed_count as i32,
                    lectures: c.lecture_count as i32,
                    image: self.image(image),
                    has_image: image.is_some(),
                }
            }
            None => Hero::default(),
        });

        let cards: Vec<CourseCard> =
            filter(&courses, &filters, now).into_iter().map(|c| self.card(c)).collect();
        lib.set_cards(model(cards));
    }

    /// Cards for the Favorites page (same list, starred only, as loaded).
    pub fn favorites(&self) -> Vec<CourseCard> {
        let courses = self.courses.borrow();
        courses.iter().filter(|c| c.is_favorite).map(|c| self.card(c)).collect()
    }

    fn card(&self, c: &CourseSummary) -> CourseCard {
        CourseCard {
            id: c.id.clone().into(),
            resume: c.last_lecture_id.clone().unwrap_or_default().into(),
            title: c.title.clone().into(),
            lectures: c.lecture_count as i32,
            percent: pct(c.completed_count, c.lecture_count),
            started: started(c),
            finished: finished(c),
            status: c.scan_status.clone().into(),
            duration: c.total_duration.map(|d| format_duration(Some(d))).unwrap_or_default().into(),
            thumbnail: self.image(c.thumbnail_path.as_deref()),
            has_thumbnail: c.thumbnail_path.is_some(),
        }
    }

    fn track_courses(&self, track: &str) -> HashSet<String> {
        let conn = self.db.lock().unwrap_or_else(|e| e.into_inner());
        queries::get_track(&conn, track)
            .ok()
            .flatten()
            .map(|t| t.courses.into_iter().map(|c| c.id).collect())
            .unwrap_or_default()
    }

    /// The library as last loaded.
    pub fn courses(&self) -> std::cell::Ref<'_, Vec<CourseSummary>> {
        self.courses.borrow()
    }

    /// A thumbnail, decoded once and kept.
    pub fn image(&self, path: Option<&str>) -> slint::Image {
        let Some(path) = path else { return slint::Image::default() };
        self.images
            .borrow_mut()
            .entry(path.to_string())
            .or_insert_with(|| slint::Image::load_from_path(std::path::Path::new(path)).unwrap_or_default())
            .clone()
    }
}

fn model<T: Clone + 'static>(items: Vec<T>) -> slint::ModelRc<T> {
    crate::course_panel::model(items)
}

/// Lectures to try when opening a course, best first: where it was left off
/// (that lecture may have vanished in a rescan), then the first lecture.
pub fn lectures_to_open(db: &Db, course_id: &str, resume: &str) -> Vec<String> {
    let mut ids = Vec::new();
    if !resume.is_empty() {
        ids.push(resume.to_string());
    }
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((first, _)) = queries::list_course_playlist(&conn, course_id)
        .ok()
        .and_then(|items| items.into_iter().next())
    {
        if !ids.contains(&first) {
            ids.push(first);
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_800_000_000;
    const DAY: i64 = 24 * 3600;

    fn course(id: &str, done: i64, total: i64, opened: Option<i64>) -> CourseSummary {
        CourseSummary {
            id: id.into(),
            title: id.into(),
            folder_path: String::new(),
            thumbnail_path: None,
            lecture_count: total,
            total_duration: Some(total as f64 * 600.0),
            is_favorite: false,
            scan_status: "Ready".into(),
            last_opened_at: opened,
            completed_count: done,
            resume_thumbnail_path: None,
            last_lecture_id: None,
            last_lecture_title: None,
            tags: Vec::new(),
        }
    }

    fn filters(status: Status, sort: Sort) -> Filters {
        Filters { query: String::new(), sort, status, tag: None, track: None }
    }

    fn ids(list: Vec<&CourseSummary>) -> Vec<&str> {
        list.into_iter().map(|c| c.id.as_str()).collect()
    }

    #[test]
    fn formats_like_the_tauri_app() {
        assert_eq!(format_duration(Some(3.0 * 3600.0 + 12.0 * 60.0)), "3h 12m");
        assert_eq!(format_duration(Some(45.0 * 60.0 + 30.0)), "45m");
        assert_eq!(format_duration(Some(30.0)), "30s");
        assert_eq!(format_duration(None), "—");
        assert_eq!(pct(1, 3), 33);
        assert_eq!(pct(2, 3), 67);
        assert_eq!(pct(5, 0), 0);
    }

    #[test]
    fn statuses_split_the_library() {
        let courses = vec![
            course("done", 4, 4, Some(NOW - 30 * DAY)),
            course("going", 1, 4, Some(NOW - DAY)),
            course("opened-only", 0, 4, Some(NOW - 20 * DAY)),
            course("fresh", 0, 4, None),
        ];
        let by = |s| ids(filter(&courses, &filters(s, Sort::Alpha), NOW));
        assert_eq!(by(Status::Finished), vec!["done"]);
        assert_eq!(by(Status::Progress), vec!["going", "opened-only"]);
        assert_eq!(by(Status::New), vec!["fresh"]);
        assert_eq!(by(Status::Recent), vec!["going"], "within 14 days");
        assert_eq!(by(Status::All).len(), 4);
    }

    #[test]
    fn sorts_recent_first_with_unopened_last() {
        let courses = vec![
            course("b-never", 0, 2, None),
            course("old", 0, 2, Some(NOW - 9 * DAY)),
            course("a-never", 0, 2, None),
            course("new", 0, 2, Some(NOW - DAY)),
        ];
        let list = ids(filter(&courses, &filters(Status::All, Sort::Recent), NOW));
        assert_eq!(list, vec!["new", "old", "a-never", "b-never"]);
    }

    #[test]
    fn sorts_by_progress_and_duration() {
        let courses = vec![course("half", 2, 4, None), course("most", 3, 4, None), course("long", 0, 9, None)];
        assert_eq!(ids(filter(&courses, &filters(Status::All, Sort::Progress), NOW)), vec!["most", "half", "long"]);
        assert_eq!(ids(filter(&courses, &filters(Status::All, Sort::Duration), NOW))[0], "long");
    }

    #[test]
    fn query_tag_and_track_narrow_the_grid() {
        let mut aws = course("AWS Solutions Architect", 0, 2, None);
        aws.tags = vec!["cloud".into()];
        let courses = vec![aws, course("Linux Bootcamp", 0, 2, None)];
        let mut f = filters(Status::All, Sort::Alpha);
        f.query = "aws".into();
        assert_eq!(filter(&courses, &f, NOW).len(), 1);
        f.query.clear();
        f.tag = Some("cloud".into());
        assert_eq!(ids(filter(&courses, &f, NOW)), vec!["AWS Solutions Architect"]);
        f.tag = None;
        f.track = Some(HashSet::from(["Linux Bootcamp".to_string()]));
        assert_eq!(ids(filter(&courses, &f, NOW)), vec!["Linux Bootcamp"]);
    }

    #[test]
    fn hero_is_the_latest_unfinished_course() {
        let courses = vec![
            course("finished-latest", 4, 4, Some(NOW)),
            course("older", 1, 4, Some(NOW - 5 * DAY)),
            course("newer", 1, 4, Some(NOW - DAY)),
            course("never", 0, 4, None),
        ];
        assert_eq!(hero(&courses).map(|c| c.id.as_str()), Some("newer"));
        assert!(hero(&[course("never", 0, 4, None)]).is_none());
    }
}
