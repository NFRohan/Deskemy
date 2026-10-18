//! What a folder given to Add Folder (or dropped on the window) holds: one
//! course, or a folder of courses. A course has videos in its own folder, or
//! in sections that hold them directly (`Course/01 Intro/001 Welcome.mp4`). A
//! folder of courses has none of its own, and at least one subfolder with
//! sections of its own: two or more folders in it holding videos, more of
//! them than the videos beside them (`Courses/Rust/01 Intro/…`,
//! `Courses/Rust/02 Types/…`, and maybe `Courses/Rust/000 Promo.mp4`). Its
//! courses are the subfolders with videos, sectioned or not. A section's
//! folder of clips (`03 Project/demo/`), or a few beside its own lectures,
//! aren't sections, so that course stays a course. A
//! course whose videos all sit a level deeper (`Course/Part 1/Section 1/…`)
//! reads as a folder of courses too, so the caller offers importing it as
//! one; a folder of courses that all keep their videos loose reads as one
//! course of sections.

use super::structure::sort_key;
use crate::scanner::{is_video, SKIPPED_DIRS};
use std::collections::HashSet;
use std::ffi::OsString;
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
    let mut with_videos = Vec::new();
    let mut sectioned = false;
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) || skipped(&entry.file_name().to_string_lossy()) {
            continue;
        }
        let dir = entry.path();
        // Once one has sections, the rest only need a video (the first ends
        // the walk).
        let videos = if sectioned {
            if has_video(&dir, 1, usize::MAX) { Videos::Some } else { Videos::None }
        } else {
            videos_in(&dir)
        };
        match videos {
            Videos::None => continue,
            Videos::Some => {}
            Videos::Sections => sectioned = true,
        }
        with_videos.push(dir);
    }
    if with_videos.is_empty() {
        return Found::Nothing;
    }
    if !sectioned {
        return Found::Course;
    }
    with_videos.sort_by_cached_key(|p| sort_key(&p.file_name().unwrap_or_default().to_string_lossy()));
    Found::Courses(with_videos)
}

/// What a folder's videos make of it.
#[derive(Debug, PartialEq)]
enum Videos {
    None,
    /// Videos, in it or in one folder of it.
    Some,
    /// Sections of its own: two or more folders in it hold videos, more of
    /// them than the videos in it.
    Sections,
}

/// One walk of `dir`.
fn videos_in(dir: &Path) -> Videos {
    let mut own = 0;
    let mut folders: HashSet<OsString> = HashSet::new();
    for entry in walk(dir, 1, usize::MAX) {
        if entry.depth() == 1 {
            own += 1;
        } else if let Some(first) = entry.path().strip_prefix(dir).ok().and_then(|rel| rel.components().next()) {
            // The folder of `dir` it's in.
            folders.insert(first.as_os_str().to_owned());
        }
    }
    match (own, folders.len()) {
        (0, 0) => Videos::None,
        (own, folders) if folders >= 2 && folders > own => Videos::Sections,
        _ => Videos::Some,
    }
}

/// A video between `from` and `to` levels below `dir` (1: in it). Stops at
/// the first.
fn has_video(dir: &Path, from: usize, to: usize) -> bool {
    walk(dir, from, to).next().is_some()
}

/// The videos between `from` and `to` levels below `dir`, skipping the
/// folders the scanner skips.
fn walk(dir: &Path, from: usize, to: usize) -> impl Iterator<Item = walkdir::DirEntry> {
    walkdir::WalkDir::new(dir)
        .min_depth(from)
        .max_depth(to)
        .follow_links(false)
        .into_iter()
        .filter_entry(|e| !(e.file_type().is_dir() && skipped(&e.file_name().to_string_lossy())))
        .flatten()
        .filter(|e| e.file_type().is_file() && is_video(e.path()))
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
        touch(root, "10 Go/02 Types/002 Ints.mp4");
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
    fn a_section_with_a_folder_of_clips_is_still_a_section() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "Course/01 Intro/001 Welcome.mp4");
        touch(tmp.path(), "Course/02 Basics/002 Types.mp4");
        touch(tmp.path(), "Course/03 Project/demo/clip.mp4");
        touch(tmp.path(), "Course/04 More/004 Traits.mp4");
        touch(tmp.path(), "Course/04 More/extras/bonus.mp4");
        assert_eq!(courses_in(&tmp.path().join("Course")), Found::Course);
        // However few sections it has.
        touch(tmp.path(), "Short/01 Intro/001 Welcome.mp4");
        touch(tmp.path(), "Short/02 Project/demo/clip.mp4");
        assert_eq!(courses_in(&tmp.path().join("Short")), Found::Course);
        touch(tmp.path(), "One/01 Only/demo/clip.mp4");
        assert_eq!(courses_in(&tmp.path().join("One")), Found::Course);
        // Lectures of its own, and a few folders of them (the QA course's
        // `03 Edge Cases`).
        touch(tmp.path(), "Edge/01 Basics/001 One.mp4");
        touch(tmp.path(), "Edge/03 Edge Cases/010 Ten.mp4");
        touch(tmp.path(), "Edge/03 Edge Cases/011 Eleven.mp4");
        touch(tmp.path(), "Edge/03 Edge Cases/Extras/Even deeper/012 Twelve.mp4");
        touch(tmp.path(), "Edge/03 Edge Cases/Long folder/013 Thirteen.mp4");
        assert_eq!(courses_in(&tmp.path().join("Edge")), Found::Course);
    }

    #[test]
    fn one_sectioned_course_among_loose_ones_is_a_folder_of_courses() {
        let tmp = tempfile::tempdir().unwrap();
        touch(tmp.path(), "Courses/A/1.mp4");
        touch(tmp.path(), "Courses/B/1.mp4");
        touch(tmp.path(), "Courses/C/01 Intro/1.mp4");
        touch(tmp.path(), "Courses/C/02 More/2.mp4");
        assert!(matches!(courses_in(&tmp.path().join("Courses")), Found::Courses(c) if c.len() == 3));
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
