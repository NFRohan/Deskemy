//! Rows for the course panel — the content tree and the current section's
//! resources — built the way the Tauri player's sidebar builds them.

use crate::tracks::clock;
use crate::{LectureRow, ResourceGroup, ResourceItem, SectionRow};
use deskemy_core::domain::{Attachment, CourseDetail, Section};
use std::collections::HashSet;

/// A Slint list model over `items`.
pub fn model<T: Clone + 'static>(items: Vec<T>) -> slint::ModelRc<T> {
    slint::ModelRc::new(slint::VecModel::from(items))
}

/// The section holding `lecture`.
pub fn current_section<'a>(course: &'a CourseDetail, lecture: Option<&str>) -> Option<&'a Section> {
    let lecture = lecture?;
    course
        .sections
        .iter()
        .find(|s| s.lectures.iter().any(|l| l.id == lecture))
}

/// Every section with its done/total count; lectures only for expanded ones.
pub fn sections(course: &CourseDetail, current: Option<&str>, expanded: &HashSet<String>) -> Vec<SectionRow> {
    course
        .sections
        .iter()
        .map(|s| {
            let done = s.lectures.iter().filter(|l| l.completed).count();
            let lectures = s
                .lectures
                .iter()
                .map(|l| LectureRow {
                    id: l.id.clone().into(),
                    title: l.title.clone().into(),
                    duration: l.duration.map(clock).unwrap_or_default().into(),
                    completed: l.completed,
                    playable: l.playable,
                    current: Some(l.id.as_str()) == current,
                })
                .collect();
            SectionRow {
                id: s.id.clone().into(),
                title: s.title.clone().into(),
                progress: format!("{done}/{}", s.lectures.len()).into(),
                expanded: expanded.contains(&s.id),
                lectures: model(lectures),
            }
        })
        .collect()
}

pub struct Resources {
    /// Title of the current section ("" when nothing is playing).
    pub section: String,
    pub groups: Vec<ResourceGroup>,
    pub count: usize,
}

/// The current section's resources only — grouped by lecture in course order,
/// then the section-level ones — so the panel isn't a course-wide dump.
pub fn resources(course: &CourseDetail, attachments: &[Attachment], current: Option<&str>) -> Resources {
    let Some(section) = current_section(course, current) else {
        return Resources { section: String::new(), groups: Vec::new(), count: 0 };
    };
    let item = |a: &Attachment| ResourceItem {
        name: a.name.clone().into(),
        kind: a.kind.clone().unwrap_or_default().into(),
        path: a.file_path.clone().into(),
    };
    let in_section: Vec<&Attachment> = attachments
        .iter()
        .filter(|a| a.section_id.as_deref() == Some(section.id.as_str()))
        .collect();

    let mut groups: Vec<(String, Vec<ResourceItem>)> = section
        .lectures
        .iter()
        .map(|l| {
            let items: Vec<ResourceItem> = in_section
                .iter()
                .filter(|a| a.lecture_id.as_deref() == Some(l.id.as_str()))
                .map(|a| item(a))
                .collect();
            (l.title.clone(), items)
        })
        .filter(|(_, items)| !items.is_empty())
        .collect();
    let section_level: Vec<ResourceItem> =
        in_section.iter().filter(|a| a.lecture_id.is_none()).map(|a| item(a)).collect();
    if !section_level.is_empty() {
        groups.push(("Section resources".into(), section_level));
    }

    let count = groups.iter().map(|(_, items)| items.len()).sum();
    Resources {
        section: section.title.clone(),
        groups: groups
            .into_iter()
            .map(|(title, items)| ResourceGroup { title: title.into(), items: model(items) })
            .collect(),
        count,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deskemy_core::domain::Lecture;
    use slint::Model;

    fn lecture(id: &str, title: &str, completed: bool) -> Lecture {
        Lecture {
            id: id.into(),
            section_id: String::new(),
            title: title.into(),
            file_path: String::new(),
            position: 0,
            duration: Some(125.0),
            container: None,
            video_codec: None,
            playable: true,
            position_seconds: 0.0,
            completed,
        }
    }

    fn course() -> CourseDetail {
        let section = |id: &str, title: &str, lectures| Section {
            id: id.into(),
            title: title.into(),
            position: 0,
            lectures,
        };
        CourseDetail {
            id: "c".into(),
            title: "Course".into(),
            folder_path: String::new(),
            thumbnail_path: None,
            total_duration: None,
            is_favorite: false,
            scan_status: "Ready".into(),
            last_opened_at: None,
            last_lecture_id: None,
            sections: vec![
                section("s1", "Intro", vec![lecture("a", "Welcome", true), lecture("b", "Setup", false)]),
                section("s2", "IAM", vec![lecture("c", "Users", false), lecture("d", "Policies", false)]),
            ],
        }
    }

    fn attachment(name: &str, section: &str, lecture: Option<&str>) -> Attachment {
        Attachment {
            id: name.into(),
            name: name.into(),
            file_path: format!("/course/{name}"),
            kind: Some("pdf".into()),
            section_id: Some(section.into()),
            lecture_id: lecture.map(Into::into),
            completed: false,
        }
    }

    #[test]
    fn sections_count_progress_and_mark_the_current_lecture() {
        let expanded = HashSet::from(["s1".to_string()]);
        let rows = sections(&course(), Some("b"), &expanded);
        assert_eq!(rows[0].progress.as_str(), "1/2");
        assert!(rows[0].expanded && !rows[1].expanded);
        let lectures: Vec<(String, bool, String)> = rows[0]
            .lectures
            .iter()
            .map(|l| (l.title.to_string(), l.current, l.duration.to_string()))
            .collect();
        assert_eq!(
            lectures,
            vec![("Welcome".into(), false, "2:05".into()), ("Setup".into(), true, "2:05".into())]
        );
    }

    #[test]
    fn current_section_is_the_one_holding_the_lecture() {
        let c = course();
        assert_eq!(current_section(&c, Some("d")).map(|s| s.title.as_str()), Some("IAM"));
        assert!(current_section(&c, Some("missing")).is_none());
        assert!(current_section(&c, None).is_none());
    }

    #[test]
    fn resources_are_the_current_sections_grouped_by_lecture_then_section() {
        let atts = vec![
            attachment("section-notes.pdf", "s2", None),
            attachment("policies.pdf", "s2", Some("d")),
            attachment("users.pdf", "s2", Some("c")),
            attachment("elsewhere.pdf", "s1", Some("a")),
        ];
        let r = resources(&course(), &atts, Some("c"));
        assert_eq!(r.section, "IAM");
        assert_eq!(r.count, 3);
        let groups: Vec<(String, Vec<String>)> = r
            .groups
            .iter()
            .map(|g| (g.title.to_string(), g.items.iter().map(|i| i.name.to_string()).collect()))
            .collect();
        assert_eq!(
            groups,
            vec![
                ("Users".into(), vec!["users.pdf".into()]),
                ("Policies".into(), vec!["policies.pdf".into()]),
                ("Section resources".into(), vec!["section-notes.pdf".into()]),
            ]
        );
    }

    #[test]
    fn nothing_playing_means_no_resources() {
        let r = resources(&course(), &[attachment("x.pdf", "s1", None)], None);
        assert!(r.groups.is_empty() && r.section.is_empty() && r.count == 0);
    }
}
