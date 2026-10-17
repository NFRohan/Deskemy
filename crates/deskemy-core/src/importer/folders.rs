//! What a folder given to Add Folder (or dropped on the window) holds: one
//! course, or a folder of courses. A course has videos in its own folder, or
//! in sections that hold them directly (`Course/01 Intro/001 Welcome.mp4`). A
//! folder of courses has none of its own, and at least one subfolder with
//! sections of its own (`Courses/Rust/01 Intro/…`); its courses are the
//! subfolders with videos, sectioned or not. A course whose videos all sit a
//! level deeper (`Course/Part 1/Section 1/…`) reads as a folder of courses
//! too, so the caller offers importing it as one; a folder of courses that
//! all keep their videos loose reads as one course of sections.

use super::structure::sort_key;
use crate::scanner::{is_video, SKIPPED_DIRS};
use std::path::{Path, PathBuf};

#[derive(Debug, PartialEq)]
pub enum Found {
    Course,
    /// The subfolders with videos, in name order.
    Courses(Vec<PathBuf>),
    /// No videos at all.
    Nothing,
}

pub fn courses_in(folder: &Path) -> Found {
    if has_video(folder, 1, 1) {
        return Found::Course;
    }
    let Ok(entries) = std::fs::read_dir(folder) else { return Found::Nothing };
    let mut with_videos: Vec<PathBuf> = entries
        .flatten()
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !skipped(&e.file_name().to_string_lossy()))
        .map(|e| e.path())
        .filter(|dir| has_video(dir, 1, usize::MAX))
        .collect();
    if with_videos.is_empty() {
        return Found::Nothing;
    }
    // Sections hold their videos; a course has them in folders of its own.
    if !with_videos.iter().any(|dir| has_video(dir, 2, usize::MAX)) {
        return Found::Course;
    }
    with_videos.sort_by_cached_key(|p| sort_key(&p.file_name().unwrap_or_default().to_string_lossy()));
    Found::Courses(with_videos)
}

/// A video between `from` and `to` levels below `dir` (1: in it). Stops at
/// the first.
fn has_video(dir: &Path, from: usize, to: usize) -> bool {
    walkdir::WalkDir::new(dir)
        .min_depth(from)
        .max_depth(to)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && skipped(&e.file_name().to_string_lossy())))
        .flatten()
        .any(|e| e.file_type().is_file() && is_video(e.path()))
}

fn skipped(name: &str) -> bool {
    SKIPPED_DIRS.contains(&name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn touch(root: &Path, rel: &str) {
        let path = root.join(rel);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"x").unwrap();
    }

    #[test]
    fn videos_in_the_folder_or_its_sections_make_a_course() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "loose/001 Welcome.mp4");
        touch(tmp.path(), "sections/01 Intro/001 Welcome.mkv");
        assert_eq!(courses_in(&tmp.path().join("loose")), Found::Course);
        assert_eq!(courses_in(&tmp.path().join("sections")), Found::Course);
    }

    #[test]
    fn a_folder_of_courses_lists_the_ones_with_videos_in_order() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        touch(root, "10 Go/01 Intro/001 Hello.mp4");
        // Loose videos of its own, and sections.
        touch(root, "2 Rust/000 Promo.mp4");
        touch(root, "2 Rust/01 Intro/001 Hello.mp4");
        touch(root, "Python/Part 1/01 Basics/001 Hello.mp4");
        // A course without sections, among ones with.
        touch(root, "Shorts/001 Tip.mp4");
        touch(root, "Notes/readme.pdf");
        touch(root, "Stuff/node_modules/x/01/clip.mp4");
        let names = match courses_in(root) {
            Found::Courses(dirs) => dirs.iter().map(|d| d.file_name().unwrap().to_string_lossy().to_string()).collect::<Vec<_>>(),
            other => panic!("{other:?}"),
        };
        assert_eq!(names, ["2 Rust", "10 Go", "Python", "Shorts"]);
    }

    #[test]
    fn subfolders_holding_their_videos_are_sections() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "Course/01 Intro/001 Welcome.mp4");
        touch(tmp.path(), "Course/02 Basics/002 Types.mp4");
        touch(tmp.path(), "Course/02 Basics/code/main.rs");
        assert_eq!(courses_in(&tmp.path().join("Course")), Found::Course);
    }

    #[test]
    fn no_videos_is_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "Docs/a/b/notes.pdf");
        // TypeScript, not a video.
        touch(tmp.path(), "Code/src/index.ts");
        assert_eq!(courses_in(tmp.path()), Found::Nothing);
        assert_eq!(courses_in(&tmp.path().join("missing")), Found::Nothing);
    }
}
