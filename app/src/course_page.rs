//! The course page — header, curriculum and resources — as the Tauri app's
//! `routes/course/[id]/+page.svelte`.

use crate::course_panel::model;
use crate::library::{format_duration, pct};
use crate::session::Db;
use crate::settings::{self, Config};
use crate::tracks::clock;
use crate::{AppWindow, Course, CurriculumLecture, CurriculumSection, Nav, ResourceGroup, ResourceItem};
use deskemy_core::courses;
use deskemy_core::db::queries;
use deskemy_core::domain::{Attachment, CourseDetail, Lecture, Section};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::rc::Rc;

/// The lecture "Resume" / "Start" opens: where the course was left off, else
/// the first unfinished playable one, else the first.
pub fn next_lecture(c: &CourseDetail) -> Option<&Lecture> {
    let all = || c.sections.iter().flat_map(|s| s.lectures.iter());
    c.last_lecture_id
        .as_deref()
        .and_then(|id| all().find(|l| l.id == id))
        .or_else(|| all().find(|l| !l.completed && l.playable))
        .or_else(|| all().next())
}

/// "3/12 · 1h 5m" — done / total, plus the known duration if any.
pub fn section_meta(s: &Section) -> String {
    let done = s.lectures.iter().filter(|l| l.completed).count();
    let known: Vec<f64> = s.lectures.iter().filter_map(|l| l.duration).collect();
    let mut meta = format!("{done}/{}", s.lectures.len());
    if !known.is_empty() {
        meta += &format!(" · {}", format_duration(Some(known.iter().sum())));
    }
    meta
}

/// Resources to show inside the curriculum: per lecture, and per section
/// for the section's resources that belong to no one lecture. Course-wide
/// resources stay in the Resources list.
pub fn inline_resources(attachments: &[Attachment]) -> (HashMap<&str, Vec<&Attachment>>, HashMap<&str, Vec<&Attachment>>) {
    let (mut by_lecture, mut by_section): (HashMap<&str, Vec<&Attachment>>, HashMap<&str, Vec<&Attachment>>) =
        Default::default();
    for a in attachments {
        match (a.lecture_id.as_deref(), a.section_id.as_deref()) {
            (Some(lecture), _) => by_lecture.entry(lecture).or_default().push(a),
            (None, Some(section)) => by_section.entry(section).or_default().push(a),
            (None, None) => {}
        }
    }
    (by_lecture, by_section)
}

/// "2 / 5 resources done", or "" for a course without resources.
pub fn resources_summary(attachments: &[Attachment]) -> String {
    if attachments.is_empty() {
        return String::new();
    }
    let done = attachments.iter().filter(|a| a.completed).count();
    let word = if attachments.len() == 1 { "resource" } else { "resources" };
    format!("{done} / {} {word} done", attachments.len())
}

/// A resource as a curriculum / Resources row.
pub fn resource_item(a: &Attachment) -> ResourceItem {
    ResourceItem {
        id: a.id.clone().into(),
        name: a.name.clone().into(),
        kind: a.kind.clone().unwrap_or_default().into(),
        path: a.file_path.clone().into(),
        done: a.completed,
    }
}

/// Resources grouped by section in curriculum order, then "Course-wide".
pub fn resource_groups<'a>(c: &CourseDetail, attachments: &'a [Attachment]) -> Vec<(String, Vec<&'a Attachment>)> {
    let mut groups: Vec<(String, Vec<&Attachment>)> = c
        .sections
        .iter()
        .map(|s| {
            let items = attachments.iter().filter(|a| a.section_id.as_deref() == Some(s.id.as_str())).collect();
            (s.title.clone(), items)
        })
        .filter(|(_, items): &(String, Vec<&Attachment>)| !items.is_empty())
        .collect();
    let loose: Vec<&Attachment> = attachments.iter().filter(|a| a.section_id.is_none()).collect();
    if !loose.is_empty() {
        groups.push(("Course-wide".into(), loose));
    }
    groups
}

/// The open course's state (UI thread only).
pub struct CoursePage {
    db: Db,
    config: Config,
    /// Where covers are stored (None without a data directory).
    thumbs_dir: Option<PathBuf>,
    id: RefCell<Option<String>>,
    expanded: RefCell<HashSet<String>>,
    image: RefCell<Option<(String, slint::Image)>>,
}

impl CoursePage {
    pub fn new(db: Db, config: Config, thumbs_dir: Option<PathBuf>) -> Rc<Self> {
        Rc::new(CoursePage {
            db,
            config,
            thumbs_dir,
            id: RefCell::new(None),
            expanded: RefCell::new(HashSet::new()),
            image: RefCell::new(None),
        })
    }

    fn conn(&self) -> std::sync::MutexGuard<'_, deskemy_core::db::Connection> {
        self.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn id(&self) -> Option<String> {
        self.id.borrow().clone()
    }

    /// Open a course's page, recording the visit (it counts as recently
    /// opened, as in the Tauri app).
    pub fn open(&self, ui: &AppWindow, course_id: &str) {
        if let Err(e) = queries::touch_opened(&self.conn(), course_id) {
            tracing::warn!(error = %e, "touch opened");
        }
        self.show(ui, course_id);
    }

    /// Show a course, with the section holding its next lecture expanded,
    /// without recording a visit.
    pub fn show(&self, ui: &AppWindow, course_id: &str) {
        *self.id.borrow_mut() = Some(course_id.to_string());
        let detail = queries::get_course_detail(&self.conn(), course_id).ok().flatten();
        let open = detail.as_ref().and_then(|c| {
            let next = next_lecture(c)?.id.clone();
            c.sections.iter().find(|s| s.lectures.iter().any(|l| l.id == next)).map(|s| s.id.clone())
        });
        *self.expanded.borrow_mut() = open.into_iter().collect();

        let title = detail.as_ref().map_or_else(|| "Course".into(), |c| c.title.clone());
        let nav = ui.global::<Nav>();
        nav.set_page("course".into());
        nav.set_crumbs(model(vec!["Library".into(), title.into()]));
        nav.set_crumb_targets(model(vec!["library".into(), "".into()]));
        self.render(ui, detail);
    }

    /// Re-read and redraw the open course (after watching, or an edit).
    pub fn refresh(&self, ui: &AppWindow) {
        let Some(id) = self.id() else { return };
        let detail = queries::get_course_detail(&self.conn(), &id).ok().flatten();
        self.render(ui, detail);
    }

    fn render(&self, ui: &AppWindow, detail: Option<CourseDetail>) {
        let page = ui.global::<Course>();
        let Some(c) = detail else {
            page.set_found(false);
            return;
        };
        let (attachments, tags) = {
            let conn = self.conn();
            (
                queries::list_course_attachments(&conn, &c.id).unwrap_or_default(),
                queries::tags_for_course(&conn, &c.id).unwrap_or_default(),
            )
        };
        let lectures: Vec<&Lecture> = c.sections.iter().flat_map(|s| s.lectures.iter()).collect();
        let done = lectures.iter().filter(|l| l.completed).count() as i64;
        let next = next_lecture(&c).map(|l| l.id.clone());
        let expanded = self.expanded.borrow();

        page.set_found(true);
        page.set_id(c.id.clone().into());
        page.set_title(c.title.clone().into());
        page.set_favorite(c.is_favorite);
        page.set_missing(c.scan_status == "Missing" || c.scan_status == "Error");
        page.set_tags(model(tags.into_iter().map(Into::into).collect()));
        page.set_done(done as i32);
        page.set_total(lectures.len() as i32);
        page.set_percent(pct(done, lectures.len() as i64));
        page.set_duration(c.total_duration.map(|d| format!("{} total", format_duration(Some(d)))).unwrap_or_default().into());
        page.set_next(next.clone().unwrap_or_default().into());
        page.set_next_label(if done > 0 { "Resume Lecture" } else { "Start Course" }.into());
        page.set_resources_summary(resources_summary(&attachments).into());
        let inline = settings::lock(&self.config).resources_inline;
        let (by_lecture, by_section) = if inline { inline_resources(&attachments) } else { (HashMap::new(), HashMap::new()) };
        let items = |list: Option<&Vec<&Attachment>>| {
            model(list.map(|l| l.iter().map(|a| resource_item(a)).collect()).unwrap_or_default())
        };

        let (image, has) = self.thumbnail(c.thumbnail_path.as_deref());
        page.set_thumbnail(image);
        page.set_has_thumbnail(has);

        page.set_sections(model(
            c.sections
                .iter()
                .map(|s| CurriculumSection {
                    id: s.id.clone().into(),
                    title: s.title.clone().into(),
                    meta: section_meta(s).into(),
                    expanded: expanded.contains(&s.id),
                    loose: items(by_section.get(s.id.as_str())),
                    lectures: model(
                        s.lectures
                            .iter()
                            .map(|l| CurriculumLecture {
                                id: l.id.clone().into(),
                                title: l.title.clone().into(),
                                duration: l.duration.map(clock).unwrap_or_default().into(),
                                completed: l.completed,
                                playable: l.playable,
                                next: next.as_deref() == Some(l.id.as_str()),
                                resources: items(by_lecture.get(l.id.as_str())),
                            })
                            .collect(),
                    ),
                })
                .collect(),
        ));
        page.set_resources(model(
            resource_groups(&c, &attachments)
                .into_iter()
                .map(|(title, items)| ResourceGroup {
                    title: title.into(),
                    items: model(
                        items
                            .into_iter()
                            .map(resource_item)
                            .collect(),
                    ),
                })
                .collect(),
        ));
    }

    fn thumbnail(&self, path: Option<&str>) -> (slint::Image, bool) {
        let Some(path) = path else { return (slint::Image::default(), false) };
        let mut cached = self.image.borrow_mut();
        if let Some((p, image)) = cached.as_ref() {
            if p == path {
                return (image.clone(), true);
            }
        }
        let image = slint::Image::load_from_path(std::path::Path::new(path)).unwrap_or_default();
        *cached = Some((path.to_string(), image.clone()));
        (image, true)
    }

    pub fn toggle_section(&self, ui: &AppWindow, id: &str) {
        {
            let mut expanded = self.expanded.borrow_mut();
            if !expanded.remove(id) {
                expanded.insert(id.to_string());
            }
        }
        self.refresh(ui);
    }

    pub fn toggle_favorite(&self, ui: &AppWindow) {
        let Some(id) = self.id() else { return };
        let favorite = !ui.global::<Course>().get_favorite();
        if let Err(e) = queries::set_favorite(&self.conn(), &id, favorite) {
            tracing::warn!(error = %e, "set favorite");
        }
        self.refresh(ui);
    }

    pub fn add_tag(&self, ui: &AppWindow, tag: &str) {
        let Some(id) = self.id() else { return };
        if let Err(e) = queries::add_tag(&self.conn(), &id, tag) {
            tracing::warn!(error = %e, "add tag");
        }
        self.refresh(ui);
    }

    pub fn remove_tag(&self, ui: &AppWindow, tag: &str) {
        let Some(id) = self.id() else { return };
        if let Err(e) = queries::remove_tag(&self.conn(), &id, tag) {
            tracing::warn!(error = %e, "remove tag");
        }
        self.refresh(ui);
    }

    /// Upload a cover from an image file.
    pub fn pick_cover(&self, ui: &AppWindow) {
        let Some(file) = rfd::FileDialog::new()
            .set_title("Choose a course cover")
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
            .set_parent(&ui.window().window_handle())
            .pick_file()
        else {
            return;
        };
        let ext = file.extension().and_then(|e| e.to_str()).map(str::to_owned);
        let stored = std::fs::read(&file)
            .map_err(|e| format!("{}: {e}", file.display()))
            .and_then(|bytes| self.store_cover(&bytes, ext.as_deref()));
        self.cover_done(ui, stored);
    }

    /// Use the image on the clipboard as the cover (Ctrl+V in the editor).
    pub fn paste_cover(&self, ui: &AppWindow) {
        let stored = clipboard_png().and_then(|png| self.store_cover(&png, Some("png")));
        self.cover_done(ui, stored);
    }

    pub fn clear_cover(&self, ui: &AppWindow) {
        let Some(id) = self.id() else { return };
        if let Err(e) = queries::set_thumbnail(&self.conn(), &id, None) {
            ui.global::<Course>().set_cover_error(e.to_string().into());
        }
        self.refresh(ui);
    }

    fn store_cover(&self, bytes: &[u8], ext: Option<&str>) -> Result<String, String> {
        let id = self.id().ok_or("no course is open")?;
        let dir = self.thumbs_dir.as_ref().ok_or("no data directory to store covers in")?;
        courses::set_cover(&self.conn(), dir, &id, bytes, ext).map_err(|e| e.to_string())
    }

    fn cover_done(&self, ui: &AppWindow, stored: Result<String, String>) {
        let page = ui.global::<Course>();
        match stored {
            Ok(_) => page.set_cover_error("".into()),
            Err(e) => {
                tracing::warn!(error = %e, "set cover");
                page.set_cover_error(e.into());
            }
        }
        self.refresh(ui);
    }

    /// Point a Missing course at the folder it moved to.
    pub fn relocate(&self, ui: &AppWindow) {
        let Some(id) = self.id() else { return };
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Where is this course now?")
            .set_parent(&ui.window().window_handle())
            .pick_folder()
        else {
            return;
        };
        let result = courses::relocate(&self.conn(), &id, &folder.to_string_lossy());
        let page = ui.global::<Course>();
        match result {
            Ok(_) => page.set_relocate_error("".into()),
            Err(e) => page.set_relocate_error(e.to_string().into()),
        }
        self.refresh(ui);
    }

    /// Remove the course from the library (its files stay on disk); returns
    /// whether it went, so the caller can leave the page.
    pub fn delete(&self, ui: &AppWindow) -> bool {
        let Some(id) = self.id() else { return false };
        let result = courses::delete(&mut self.conn(), &id);
        match result {
            Ok(()) => {
                *self.id.borrow_mut() = None;
                true
            }
            Err(e) => {
                ui.global::<Course>().set_delete_error(e.to_string().into());
                false
            }
        }
    }

    /// Mark a resource done / not done.
    pub fn set_resource_done(&self, ui: &AppWindow, id: &str, done: bool) {
        if let Err(e) = queries::set_attachment_completed(&self.conn(), id, done) {
            tracing::warn!(error = %e, "mark resource");
        }
        self.refresh(ui);
    }

    /// Mark a lecture done / not done by hand.
    pub fn toggle_complete(&self, ui: &AppWindow, lecture: &str) {
        let done = {
            let conn = self.conn();
            let (_, completed, _) = queries::get_progress(&conn, lecture).unwrap_or((0.0, false, None));
            if let Err(e) = queries::set_completed(&conn, lecture, !completed) {
                tracing::warn!(error = %e, "set completed");
            }
            !completed
        };
        tracing::debug!(lecture, done, "marked");
        self.refresh(ui);
    }
}

/// The clipboard's image, as PNG bytes.
fn clipboard_png() -> Result<Vec<u8>, String> {
    let image = arboard::Clipboard::new()
        .and_then(|mut clipboard| clipboard.get_image())
        .map_err(|_| "There's no image on the clipboard.".to_string())?;
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, image.width as u32, image.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&image.bytes))
        .map_err(|e| e.to_string())?;
    Ok(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lecture(id: &str, completed: bool, playable: bool, duration: Option<f64>) -> Lecture {
        Lecture {
            id: id.into(),
            section_id: String::new(),
            title: id.into(),
            file_path: String::new(),
            position: 0,
            duration,
            container: None,
            video_codec: None,
            playable,
            position_seconds: 0.0,
            completed,
        }
    }

    fn course(last: Option<&str>, sections: Vec<(&str, Vec<Lecture>)>) -> CourseDetail {
        CourseDetail {
            id: "c".into(),
            title: "Course".into(),
            folder_path: String::new(),
            thumbnail_path: None,
            total_duration: None,
            is_favorite: false,
            scan_status: "Ready".into(),
            last_opened_at: None,
            last_lecture_id: last.map(Into::into),
            sections: sections
                .into_iter()
                .map(|(id, lectures)| Section { id: id.into(), title: id.into(), position: 0, lectures })
                .collect(),
        }
    }

    #[test]
    fn next_lecture_prefers_the_resume_point_then_the_first_unfinished() {
        let lectures = || {
            vec![
                ("s1", vec![lecture("a", true, true, None), lecture("broken", false, false, None)]),
                ("s2", vec![lecture("b", false, true, None)]),
            ]
        };
        assert_eq!(next_lecture(&course(Some("a"), lectures())).unwrap().id, "a");
        // Skips done and unplayable lectures.
        assert_eq!(next_lecture(&course(None, lectures())).unwrap().id, "b");
        // A resume point that vanished falls through.
        assert_eq!(next_lecture(&course(Some("gone"), lectures())).unwrap().id, "b");
    }

    #[test]
    fn section_meta_counts_done_and_sums_known_durations() {
        let s = Section {
            id: "s".into(),
            title: "s".into(),
            position: 0,
            lectures: vec![
                lecture("a", true, true, Some(1800.0)),
                lecture("b", false, true, Some(2100.0)),
                lecture("c", false, true, None),
            ],
        };
        assert_eq!(section_meta(&s), "1/3 · 1h 5m");
        let unknown = Section { lectures: vec![lecture("x", false, true, None)], ..s };
        assert_eq!(section_meta(&unknown), "0/1");
    }

    #[test]
    fn inline_resources_split_by_lecture_then_section() {
        let att = |name: &str, section: Option<&str>, lecture: Option<&str>, completed: bool| Attachment {
            id: name.into(),
            name: name.into(),
            file_path: String::new(),
            kind: Some("pdf".into()),
            section_id: section.map(Into::into),
            lecture_id: lecture.map(Into::into),
            completed,
        };
        let atts = vec![
            att("ex1.pdf", Some("s1"), Some("l1"), true),
            att("ex1b.pdf", Some("s1"), Some("l1"), false),
            att("notes.pdf", Some("s1"), None, false),
            att("syllabus.pdf", None, None, false),
        ];
        let (by_lecture, by_section) = inline_resources(&atts);
        let names = |v: Option<&Vec<&Attachment>>| v.map(|v| v.iter().map(|a| a.name.clone()).collect::<Vec<_>>());
        assert_eq!(names(by_lecture.get("l1")), Some(vec!["ex1.pdf".to_string(), "ex1b.pdf".into()]));
        assert_eq!(names(by_section.get("s1")), Some(vec!["notes.pdf".to_string()]));
        // Course-wide ones stay out of the curriculum.
        assert_eq!(by_lecture.values().chain(by_section.values()).map(Vec::len).sum::<usize>(), 3);
        assert_eq!(resources_summary(&atts), "1 / 4 resources done");
        assert_eq!(resources_summary(&atts[..1]), "1 / 1 resource done");
        assert_eq!(resources_summary(&[]), "");
    }

    #[test]
    fn resources_follow_the_curriculum_then_course_wide() {
        let c = course(None, vec![("s1", vec![]), ("s2", vec![])]);
        let att = |name: &str, section: Option<&str>| Attachment {
            id: name.into(),
            name: name.into(),
            file_path: String::new(),
            kind: None,
            section_id: section.map(Into::into),
            lecture_id: None,
            completed: false,
        };
        let atts = vec![att("loose.pdf", None), att("two.pdf", Some("s2")), att("one.pdf", Some("s1"))];
        let groups: Vec<(String, Vec<&str>)> = resource_groups(&c, &atts)
            .into_iter()
            .map(|(t, items)| (t, items.into_iter().map(|a| a.name.as_str()).collect()))
            .collect();
        assert_eq!(
            groups,
            vec![
                ("s1".into(), vec!["one.pdf"]),
                ("s2".into(), vec!["two.pdf"]),
                ("Course-wide".into(), vec!["loose.pdf"]),
            ]
        );
    }
}
