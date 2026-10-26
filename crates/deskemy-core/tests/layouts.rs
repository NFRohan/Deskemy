//! Course layouts from the wild, replayed through the importer.
//!
//! Each `tests/layouts/*.txt` is a course folder's file list and the outline
//! the importer should make of it. The files are recreated empty (the stub
//! prober reads none of them), the folder goes through Add Folder's course
//! detection and the import, and the outline is compared with the expected
//! one. A tricky course someone reports becomes a fixture here, so the fix
//! for it can't quietly break the others.
//!
//! ```text
//! # What the layout is and why it's here.
//! root: Course folder name          (optional; "Course" by default)
//! --- files
//! 01 - Intro/001 Welcome.mp4
//! 01 - Intro/002 clip.ts  (mpeg-ts)   (a .ts video rather than TypeScript)
//! --- expected
//! found: one course
//! course: Course folder name
//! == Intro
//!   Welcome
//! ```
//!
//! After a deliberate change, `DESKEMY_BLESS=1 cargo test --test layouts`
//! rewrites each fixture's expected outline with what the importer makes now;
//! review the diff.

use deskemy_core::importer::folders::{courses_in, Found};
use deskemy_core::{db, importer::Importer, media::stub::StubProber};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

struct Fixture {
    path: PathBuf,
    head: String,
    root: String,
    files: Vec<(String, bool)>,
    expected: String,
}

fn read(path: &Path) -> Fixture {
    let text = std::fs::read_to_string(path).unwrap().replace("\r\n", "\n");
    let (head, rest) = text.split_once("--- files\n").expect("a `--- files` line");
    let (files, expected) = rest.split_once("--- expected\n").expect("an `--- expected` line");
    let root = head
        .lines()
        .find_map(|l| l.strip_prefix("root: "))
        .unwrap_or("Course")
        .to_string();
    let files = files
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| match l.strip_suffix("  (mpeg-ts)") {
            Some(name) => (name.to_string(), true),
            None => (l.to_string(), false),
        })
        .collect();
    Fixture { path: path.to_path_buf(), head: head.to_string(), root, files, expected: expected.to_string() }
}

/// The empty files, a `.ts` video's with transport-stream sync bytes.
fn build(fixture: &Fixture, at: &Path) -> PathBuf {
    let root = at.join(&fixture.root);
    for (rel, mpeg_ts) in &fixture.files {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut bytes = vec![b'x'];
        if *mpeg_ts {
            bytes = vec![0; 400];
            bytes[0] = 0x47;
            bytes[188] = 0x47;
        }
        std::fs::write(path, bytes).unwrap();
    }
    root
}

/// What Add Folder finds in `folder`, and each course's outline.
fn outline(folder: &Path) -> String {
    let mut out = String::new();
    let courses = match courses_in(folder) {
        Found::Course => {
            writeln!(out, "found: one course").unwrap();
            vec![folder.to_path_buf()]
        }
        Found::Courses(list) => {
            writeln!(out, "found: {} courses", list.len()).unwrap();
            list
        }
        Found::Nothing => {
            writeln!(out, "found: nothing").unwrap();
            vec![]
        }
    };
    for course in courses {
        course_outline(&course, &mut out);
    }
    out
}

fn course_outline(course: &Path, out: &mut String) {
    let mut conn = db::open_in_memory().unwrap();
    let id = Importer::new(Box::new(StubProber)).import_course(&mut conn, None, course).unwrap();
    let detail = db::queries::get_course_detail(&conn, &id).unwrap().unwrap();
    let resources = db::queries::list_course_attachments(&conn, &id).unwrap();
    let mut stmt = conn
        .prepare("SELECT lecture_id, COALESCE(lang, '-') FROM subtitles ORDER BY lang")
        .unwrap();
    let subtitles: Vec<(String, String)> =
        stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?))).unwrap().map(Result::unwrap).collect();

    writeln!(out, "course: {}", detail.title).unwrap();
    for section in &detail.sections {
        writeln!(out, "== {}", section.title).unwrap();
        for r in resources.iter().filter(|r| r.section_id.as_ref() == Some(&section.id) && r.lecture_id.is_none()) {
            writeln!(out, "  [section] {}", r.name).unwrap();
        }
        for lecture in &section.lectures {
            writeln!(out, "  {}", lecture.title).unwrap();
            let langs: Vec<&str> =
                subtitles.iter().filter(|(l, _)| *l == lecture.id).map(|(_, lang)| lang.as_str()).collect();
            if !langs.is_empty() {
                writeln!(out, "    ~ {}", langs.join(" ")).unwrap();
            }
            for r in resources.iter().filter(|r| r.lecture_id.as_ref() == Some(&lecture.id)) {
                writeln!(out, "    + {}", r.name).unwrap();
            }
        }
    }
    for r in resources.iter().filter(|r| r.section_id.is_none() && r.lecture_id.is_none()) {
        writeln!(out, "[course] {}", r.name).unwrap();
    }
}

#[test]
fn layouts_import_as_expected() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/layouts");
    let mut paths: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e == "txt"))
        .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures in {}", dir.display());
    let bless = std::env::var_os("DESKEMY_BLESS").is_some();

    let mut failed = Vec::new();
    for path in paths {
        let fixture = read(&path);
        let tmp = tempfile::tempdir().unwrap();
        let actual = outline(&build(&fixture, tmp.path()));
        if actual == fixture.expected {
            continue;
        }
        if bless {
            let files: String = fixture
                .files
                .iter()
                .map(|(f, ts)| if *ts { format!("{f}  (mpeg-ts)\n") } else { format!("{f}\n") })
                .collect();
            let text = format!("{}--- files\n{files}--- expected\n{actual}", fixture.head);
            std::fs::write(&fixture.path, text).unwrap();
            continue;
        }
        eprintln!("--- {}\nexpected:\n{}\nactual:\n{}", path.display(), fixture.expected, actual);
        failed.push(path.file_name().unwrap().to_string_lossy().to_string());
    }
    assert!(failed.is_empty(), "layouts that import differently: {failed:?} (see above)");
}
