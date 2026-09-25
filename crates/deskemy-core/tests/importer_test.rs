//! M1-IT: build a sample library on disk → scan + import → assert the full
//! course/section/lecture hierarchy, ordering, and resource association land in
//! SQLite. Uses the stub prober (no native media deps).

use deskemy_core::db;
use deskemy_core::importer::Importer;
use deskemy_core::media::stub::StubProber;
use std::fs;
use std::path::Path;

fn touch(path: &Path) {
    fs::write(path, b"x").unwrap();
}

#[test]
fn imports_hierarchy_ordering_and_resources() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Awesome Course");

    // Section "02 - Basics": two lectures, one subtitle, one numbered resource.
    let s2 = course.join("02 - Basics");
    fs::create_dir_all(&s2).unwrap();
    touch(&s2.join("001 Welcome.mp4"));
    touch(&s2.join("001 Welcome.srt")); // subtitle, exact stem
    touch(&s2.join("002 Setup.mp4"));
    touch(&s2.join("002 setup-code.sql")); // resource by leading number 002

    // Section "01 - Intro": deliberately out-of-order files.
    let s1 = course.join("01 - Intro");
    fs::create_dir_all(&s1).unwrap();
    touch(&s1.join("002 Second.mp4"));
    touch(&s1.join("001 First.mp4"));

    // Loose root video → implicit "Introduction" section, sorted first.
    touch(&course.join("00 Promo.mp4"));

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let course_id = importer.import_course(&mut conn, None, &course).unwrap();

    let detail = db::queries::get_course_detail(&conn, &course_id)
        .unwrap()
        .unwrap();

    // Sections ordered: Introduction, Intro, Basics.
    let section_titles: Vec<&str> = detail.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(section_titles, vec!["Introduction", "Intro", "Basics"]);

    // Intro lectures ordered First, Second (natural sort by leading number).
    let intro = &detail.sections[1];
    let intro_lectures: Vec<&str> = intro.lectures.iter().map(|l| l.title.as_str()).collect();
    assert_eq!(intro_lectures, vec!["First", "Second"]);

    // Titles are cleaned (prefix + extension stripped).
    let basics = &detail.sections[2];
    let basics_lectures: Vec<&str> = basics.lectures.iter().map(|l| l.title.as_str()).collect();
    assert_eq!(basics_lectures, vec!["Welcome", "Setup"]);

    // 5 lectures total (1 promo + 2 intro + 2 basics).
    let total: usize = detail.sections.iter().map(|s| s.lectures.len()).sum();
    assert_eq!(total, 5);
    assert_eq!(detail.total_duration, None); // stub prober reports no durations

    // Exactly one subtitle, attached to "Welcome".
    let sub_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM subtitles", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sub_count, 1);

    // The numbered .sql resource attached to a lecture (not just the section).
    let att_lecture: Option<String> = conn
        .query_row(
            "SELECT lecture_id FROM attachments WHERE name LIKE '%setup-code.sql'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(att_lecture.is_some(), "resource should bind to lecture 002");

    // FTS5 index is populated and queryable (validates bundled SQLite FTS5).
    let hits: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM search_index WHERE search_index MATCH 'Welcome'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(hits >= 1);
}

#[test]
fn reimport_preserves_user_data() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Preserve Course");
    let s1 = course.join("01 - Intro");
    fs::create_dir_all(&s1).unwrap();
    touch(&s1.join("001 First.mp4"));
    touch(&s1.join("002 Second.mp4"));

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let cid1 = importer.import_course(&mut conn, None, &course).unwrap();

    // Attach user data to the "First" lecture.
    let detail = db::queries::get_course_detail(&conn, &cid1).unwrap().unwrap();
    let lec = detail
        .sections
        .iter()
        .flat_map(|s| &s.lectures)
        .find(|l| l.title == "First")
        .unwrap();
    let old_lec_id = lec.id.clone();
    let file_path = lec.file_path.clone();

    db::queries::save_progress(&conn, &old_lec_id, 123.5, true).unwrap();
    db::queries::add_bookmark(&conn, &old_lec_id, 42.0, Some("note")).unwrap();
    db::queries::add_tag(&conn, &cid1, "sql").unwrap();
    db::queries::set_favorite(&conn, &cid1, true).unwrap();

    // Re-import the same folder → new ids, but user data must survive.
    let cid2 = importer.import_course(&mut conn, None, &course).unwrap();
    assert_ne!(cid1, cid2);

    let detail2 = db::queries::get_course_detail(&conn, &cid2).unwrap().unwrap();
    let lec2 = detail2
        .sections
        .iter()
        .flat_map(|s| &s.lectures)
        .find(|l| l.file_path == file_path)
        .unwrap();
    assert_ne!(lec2.id, old_lec_id, "new lecture id after re-import");

    // Progress remapped by file path.
    let (pos, completed, _) = db::queries::get_progress(&conn, &lec2.id).unwrap();
    assert!(completed);
    assert!((pos - 123.5).abs() < 0.01);
    assert!(lec2.completed);

    // Bookmark remapped.
    let bms = db::queries::list_bookmarks(&conn, &lec2.id).unwrap();
    assert_eq!(bms.len(), 1);
    assert_eq!(bms[0].label.as_deref(), Some("note"));

    // Tag + favorite preserved on the new course.
    assert_eq!(db::queries::tags_for_course(&conn, &cid2).unwrap(), vec!["sql".to_string()]);
    assert!(detail2.is_favorite);
}

#[test]
fn imports_nested_folders_and_numbering_variants() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Variants");

    // A section with a nested subfolder: deeper files collapse to the top-level
    // section, and numbering variants (`1 -`, `2.`, none) sort correctly.
    let adv = course.join("02 - Advanced");
    let nested = adv.join("Deep Dive");
    fs::create_dir_all(&nested).unwrap();
    touch(&adv.join("1 - Basics.mp4"));
    touch(&nested.join("2. Internals.mp4")); // nested → still "Advanced"
    touch(&adv.join("Wrapup.mp4")); // unnumbered → sorts after numbered
    fs::write(adv.join("cheatsheet.pdf"), b"pdf").unwrap(); // section-level resource

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let cid = importer.import_course(&mut conn, None, &course).unwrap();
    let detail = db::queries::get_course_detail(&conn, &cid).unwrap().unwrap();

    // Nested files collapse into the single top-level "Advanced" section.
    assert_eq!(detail.sections.len(), 1);
    let sec = &detail.sections[0];
    assert_eq!(sec.title, "Advanced");
    let titles: Vec<&str> = sec.lectures.iter().map(|l| l.title.as_str()).collect();
    assert_eq!(titles, vec!["Basics", "Internals", "Wrapup"]);

    // The .pdf is classified as a resource attachment.
    let att: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attachments WHERE name LIKE '%cheatsheet.pdf'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(att, 1, "pdf classified as an attachment");
}

#[test]
fn reimport_keeps_progress_across_a_rename() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Rename Course");
    let sec = course.join("01 - Intro");
    fs::create_dir_all(&sec).unwrap();
    // Distinct content → distinct content hashes (empty/identical files would
    // collide and defeat hash matching).
    let first = sec.join("001 Original Name.mp4");
    fs::write(&first, b"the-actual-video-bytes-of-lecture-one").unwrap();
    fs::write(sec.join("002 Second.mp4"), b"different-bytes-for-lecture-two").unwrap();

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let cid1 = importer.import_course(&mut conn, None, &course).unwrap();

    let detail = db::queries::get_course_detail(&conn, &cid1).unwrap().unwrap();
    let lec = detail
        .sections
        .iter()
        .flat_map(|s| &s.lectures)
        .find(|l| l.title == "Original Name")
        .unwrap();
    let old_id = lec.id.clone();
    db::queries::save_progress(&conn, &old_id, 200.0, true).unwrap();
    db::queries::add_bookmark(&conn, &old_id, 55.0, Some("bm")).unwrap();

    // Rename the file on disk (same content) — its path changes, hash doesn't.
    let renamed = sec.join("001 Renamed Lecture.mp4");
    fs::rename(&first, &renamed).unwrap();

    let cid2 = importer.import_course(&mut conn, None, &course).unwrap();
    let detail2 = db::queries::get_course_detail(&conn, &cid2).unwrap().unwrap();
    let lec2 = detail2
        .sections
        .iter()
        .flat_map(|s| &s.lectures)
        .find(|l| l.title == "Renamed Lecture")
        .expect("renamed lecture present under its new title");

    // Progress + bookmark carried over by content hash despite the new path/id.
    assert_ne!(lec2.id, old_id);
    let (pos, completed, _) = db::queries::get_progress(&conn, &lec2.id).unwrap();
    assert!(completed && (pos - 200.0).abs() < 0.01, "progress survived the rename");
    assert_eq!(
        db::queries::list_bookmarks(&conn, &lec2.id).unwrap().len(),
        1,
        "bookmark survived the rename"
    );
}

// Relocating a moved/renamed course rewrites the stored paths, keeps every id,
// and therefore preserves progress and bookmarks untouched.
#[test]
fn relocate_course_rewrites_paths_and_keeps_progress() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Old Name");
    let s = course.join("01 - Intro");
    fs::create_dir_all(&s).unwrap();
    touch(&s.join("001 First.mp4"));
    touch(&s.join("002 Second.mp4"));

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let course_id = importer.import_course(&mut conn, None, &course).unwrap();

    let before = db::queries::get_course_detail(&conn, &course_id).unwrap().unwrap();
    let old_folder = before.folder_path.clone();
    let lec_id = before.sections[0].lectures[0].id.clone();
    let lec_path = before.sections[0].lectures[0].file_path.clone();
    db::queries::save_progress(&conn, &lec_id, 90.0, true).unwrap();
    db::queries::add_bookmark(&conn, &lec_id, 10.0, Some("bm")).unwrap();

    // Simulate the folder being renamed/moved.
    let new_folder = r"D:\Moved\New Name";
    db::queries::relocate_course(&conn, &course_id, &old_folder, new_folder).unwrap();

    let after = db::queries::get_course_detail(&conn, &course_id).unwrap().unwrap();
    assert_eq!(after.folder_path, new_folder, "course folder repointed");
    let lec_after = &after.sections[0].lectures[0];
    assert_eq!(lec_after.id, lec_id, "lecture id preserved");
    assert!(lec_after.file_path.starts_with(new_folder), "lecture path repointed");
    assert_eq!(
        lec_path.strip_prefix(old_folder.as_str()).unwrap(),
        lec_after.file_path.strip_prefix(new_folder).unwrap(),
        "relative suffix unchanged",
    );

    let (pos, completed, _) = db::queries::get_progress(&conn, &lec_id).unwrap();
    assert!(completed && (pos - 90.0).abs() < 0.01, "progress preserved");
    assert_eq!(
        db::queries::list_bookmarks(&conn, &lec_id).unwrap().len(),
        1,
        "bookmark preserved"
    );

    let status: String = conn
        .query_row(
            "SELECT scan_status FROM courses WHERE id = ?1",
            [course_id.as_str()],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(status, "Ready", "course marked Ready again");
}

#[test]
fn dotted_section_folders_keep_their_names() {
    // "NN. Title" folders (the usual Udemy layout) used to import as just "NN".
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("AWS Course");
    for section in ["03. Getting started with AWS", "04. IAM & AWS CLI"] {
        fs::create_dir_all(course.join(section)).unwrap();
        touch(&course.join(section).join("1. Lecture.mp4"));
    }

    let mut conn = db::open_in_memory().unwrap();
    let course_id = Importer::new(Box::new(StubProber))
        .import_course(&mut conn, None, &course)
        .unwrap();
    let detail = db::queries::get_course_detail(&conn, &course_id).unwrap().unwrap();

    let titles: Vec<&str> = detail.sections.iter().map(|s| s.title.as_str()).collect();
    assert_eq!(titles, vec!["Getting started with AWS", "IAM & AWS CLI"]);
    assert_eq!(detail.sections[1].lectures[0].title, "Lecture");
}

#[test]
fn repair_fixes_only_titles_the_old_cleaner_broke() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("AWS Course");
    for section in ["04. IAM & AWS CLI", "05. EC2 Fundamentals", "06 - Networking"] {
        fs::create_dir_all(course.join(section)).unwrap();
        touch(&course.join(section).join("1. Lecture.mp4"));
    }
    let mut conn = db::open_in_memory().unwrap();
    let course_id = Importer::new(Box::new(StubProber))
        .import_course(&mut conn, None, &course)
        .unwrap();
    let ids: Vec<String> = db::queries::get_course_detail(&conn, &course_id)
        .unwrap()
        .unwrap()
        .sections
        .into_iter()
        .map(|s| s.id)
        .collect();

    // As a library imported before the fix looks: "04" and "05"; plus one
    // section the user renamed to something that happens to look similar.
    db::queries::set_section_title(&conn, &ids[0], "04").unwrap();
    db::queries::set_section_title(&conn, &ids[1], "05").unwrap();
    db::queries::set_section_title(&conn, &ids[2], "06").unwrap();

    assert_eq!(deskemy_core::importer::repair_section_titles(&mut conn).unwrap(), 2);
    let titles: Vec<String> = db::queries::get_course_detail(&conn, &course_id)
        .unwrap()
        .unwrap()
        .sections
        .into_iter()
        .map(|s| s.title)
        .collect();
    // "06 - Networking" was never broken by the bug, so "06" is the user's.
    assert_eq!(titles, vec!["IAM & AWS CLI", "EC2 Fundamentals", "06"]);

    // Searchable under the repaired title, and a second run is a no-op.
    let hits = db::queries::search(&conn, "IAM", 10).unwrap();
    assert!(hits.iter().any(|h| h.title == "IAM & AWS CLI"));
    assert_eq!(deskemy_core::importer::repair_section_titles(&mut conn).unwrap(), 0);
}

#[test]
fn relocate_accepts_the_moved_folder_and_refuses_an_unrelated_one() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Course");
    fs::create_dir_all(course.join("01 Intro")).unwrap();
    touch(&course.join("01 Intro").join("001 Welcome.mp4"));
    let mut conn = db::open_in_memory().unwrap();
    let course_id = Importer::new(Box::new(StubProber))
        .import_course(&mut conn, None, &course)
        .unwrap();

    let unrelated = tmp.path().join("Something else");
    fs::create_dir_all(&unrelated).unwrap();
    let err = deskemy_core::courses::relocate(&conn, &course_id, unrelated.to_str().unwrap());
    assert!(err.is_err(), "a folder without the course's files is refused");
    assert!(deskemy_core::courses::relocate(&conn, &course_id, "/does/not/exist").is_err());

    let moved = tmp.path().join("Course (moved)");
    fs::rename(&course, &moved).unwrap();
    // A trailing separator is tolerated.
    let target = format!("{}{}", moved.display(), std::path::MAIN_SEPARATOR);
    let stored = deskemy_core::courses::relocate(&conn, &course_id, &target).unwrap();
    assert_eq!(stored, moved.to_string_lossy());
    let lecture = db::queries::first_lecture_path(&conn, &course_id).unwrap().unwrap();
    assert!(std::path::Path::new(&lecture).exists(), "lectures follow the folder");
}

#[test]
fn delete_removes_the_course_and_nothing_on_disk() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Course");
    fs::create_dir_all(&course).unwrap();
    touch(&course.join("001 Welcome.mp4"));
    let mut conn = db::open_in_memory().unwrap();
    let course_id = Importer::new(Box::new(StubProber))
        .import_course(&mut conn, None, &course)
        .unwrap();

    deskemy_core::courses::delete(&mut conn, &course_id).unwrap();
    assert!(db::queries::get_course_detail(&conn, &course_id).unwrap().is_none());
    assert!(db::queries::search(&conn, "Welcome", 10).unwrap().is_empty(), "search index cleaned");
    assert!(course.join("001 Welcome.mp4").exists(), "files stay on disk");
}

#[test]
fn preview_counts_what_an_import_would_create() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Rust Course");
    let s1 = course.join("01 - Basics");
    fs::create_dir_all(&s1).unwrap();
    touch(&s1.join("001 Hello.mp4"));
    touch(&s1.join("001 Hello.srt"));
    touch(&s1.join("002 Types.mp4"));
    touch(&s1.join("notes.pdf"));

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let snap = importer.read_snapshot(&conn, &course).unwrap();
    let plan = importer.build(&course, &snap, true, |_, _| {}).unwrap();
    let p = plan.preview(&snap);
    assert_eq!(p.title, "Rust Course");
    assert!(!p.is_reimport);
    assert_eq!((p.sections, p.lectures, p.resources, p.subtitles, p.unplayable), (1, 2, 1, 1, 0));

    // Once imported, the same folder previews as a re-import.
    importer.persist(&mut conn, None, &snap, &plan).unwrap();
    let again = importer.read_snapshot(&conn, &course).unwrap();
    assert!(importer.build(&course, &again, true, |_, _| {}).unwrap().preview(&again).is_reimport);
}

#[test]
fn subtitle_search_marks_matched_words() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("K8s");
    fs::create_dir_all(&course).unwrap();
    touch(&course.join("001 Pods.mp4"));
    fs::write(
        course.join("001 Pods.srt"),
        "1\n00:00:05,000 --> 00:00:07,000\nA [Music] pod runs on a cluster node.\n",
    )
    .unwrap();

    let mut conn = db::open_in_memory().unwrap();
    Importer::new(Box::new(StubProber)).import_course(&mut conn, None, &course).unwrap();
    let db = std::sync::Mutex::new(conn);
    assert_eq!(deskemy_core::maintenance::reindex_subtitles(&db).unwrap(), 1);
    let conn = db.lock().unwrap();

    let hits = db::queries::subtitle_search_marked(&conn, "cluster", 10, ("\u{2}", "\u{3}")).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start_ms, 5000);
    assert_eq!(hits[0].snippet, "A [Music] pod runs on a \u{2}cluster\u{3} node.");
    // The default markers are the brackets the Tauri page shows.
    let plain = db::queries::subtitle_search(&conn, "cluster", 10).unwrap();
    assert_eq!(plain[0].snippet, "A [Music] pod runs on a [cluster] node.");
}

#[test]
fn exercise_pdfs_pair_with_their_lessons() {
    // A real course layout (from a user): numbered lessons, some with a PDF of
    // exercises sharing the lesson's name.
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Matematica");
    let s = course.join("DAI NUMERI NATURALI ALLE PRIME EQUAZIONI");
    fs::create_dir_all(&s).unwrap();
    for name in [
        "1. NAT01 – Espressioni con i numeri naturali (1).mp4",
        "1. NAT01 – Espressioni con i numeri naturali (1).pdf",
        "2. NAT02 – Espressioni con i numeri naturali (2).mp4",
        "4. NAT04 – Espressioni con i numeri naturali (4).mp4",
        "4. NAT04 – Espressioni con i numeri naturali (4).pdf",
        "9. POT01 – Proprietà delle potenze (1).mp4",
        "9. POT01 – Proprietà delle potenze (1).pdf",
        "10. POT02 – Proprietà delle potenze (2).mp4",
    ] {
        touch(&s.join(name));
    }
    let mut conn = db::open_in_memory().unwrap();
    let id = Importer::new(Box::new(StubProber)).import_course(&mut conn, None, &course).unwrap();
    let detail = db::queries::get_course_detail(&conn, &id).unwrap().unwrap();
    let lecture_title = |lid: &str| {
        detail.sections.iter().flat_map(|s| &s.lectures).find(|l| l.id == lid).map(|l| l.title.clone())
    };
    let mut pairs: Vec<(String, Option<String>)> = db::queries::list_course_attachments(&conn, &id)
        .unwrap()
        .into_iter()
        .map(|a| (a.name, a.lecture_id.as_deref().and_then(lecture_title)))
        .collect();
    pairs.sort();
    assert_eq!(
        pairs,
        vec![
            ("1. NAT01 – Espressioni con i numeri naturali (1).pdf".into(), Some("NAT01 – Espressioni con i numeri naturali (1)".into())),
            ("4. NAT04 – Espressioni con i numeri naturali (4).pdf".into(), Some("NAT04 – Espressioni con i numeri naturali (4)".into())),
            ("9. POT01 – Proprietà delle potenze (1).pdf".into(), Some("POT01 – Proprietà delle potenze (1)".into())),
        ]
    );
}

#[test]
fn resources_marked_done_survive_a_reimport() {
    let tmp = tempfile::tempdir().unwrap();
    let course = tmp.path().join("Course");
    fs::create_dir_all(&course).unwrap();
    touch(&course.join("01 Lesson.mp4"));
    touch(&course.join("01 Lesson.pdf"));
    touch(&course.join("02 Next.mp4"));
    touch(&course.join("02 Next.pdf"));

    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let id = importer.import_course(&mut conn, None, &course).unwrap();
    let attachments = db::queries::list_course_attachments(&conn, &id).unwrap();
    assert!(attachments.iter().all(|a| !a.completed));
    let first = attachments.iter().find(|a| a.name == "01 Lesson.pdf").unwrap();
    db::queries::set_attachment_completed(&conn, &first.id, true).unwrap();
    // Marking twice is harmless; unmarking works.
    db::queries::set_attachment_completed(&conn, &first.id, true).unwrap();

    // Re-import (new course / attachment ids): the tick follows the file.
    let id = importer.import_course(&mut conn, None, &course).unwrap();
    let done: Vec<(String, bool)> = db::queries::list_course_attachments(&conn, &id)
        .unwrap()
        .into_iter()
        .map(|a| (a.name, a.completed))
        .collect();
    assert_eq!(done, vec![("01 Lesson.pdf".to_string(), true), ("02 Next.pdf".to_string(), false)]);

    let first = db::queries::list_course_attachments(&conn, &id).unwrap().remove(0);
    db::queries::set_attachment_completed(&conn, &first.id, false).unwrap();
    assert!(!db::queries::list_course_attachments(&conn, &id).unwrap()[0].completed);
}
