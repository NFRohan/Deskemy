//! M1-IT: build a sample library on disk → scan + import → assert the full
//! course/section/lecture hierarchy, ordering, and resource association land in
//! SQLite. Uses the stub prober (no native media deps).

use deskemy_lib::db;
use deskemy_lib::importer::Importer;
use deskemy_lib::media::stub::StubProber;
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
