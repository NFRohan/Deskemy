//! Career tracks: create → add → reorder → remove → delete, with completion
//! aggregated over the member courses. Uses the stub prober on a scratch
//! library.

use deskemy_core::db::{self, queries};
use deskemy_core::importer::Importer;
use deskemy_core::media::stub::StubProber;
use std::fs;

#[test]
fn track_lifecycle_and_completion() {
    let tmp = tempfile::tempdir().unwrap();
    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let mut import = |name: &str, lectures: usize| {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        for i in 1..=lectures {
            fs::write(dir.join(format!("{i:03} Lecture.mp4")), b"x").unwrap();
        }
        importer.import_course(&mut conn, None, &dir).unwrap()
    };
    let linux = import("Linux", 2);
    let docker = import("Docker", 3);
    let k8s = import("Kubernetes", 1);

    let id = queries::create_track(&conn, "Platform", Some("Linux → Docker → K8s")).unwrap();
    for course in [&linux, &docker, &k8s] {
        queries::add_course_to_track(&conn, &id, course).unwrap();
    }
    // Adding twice is a no-op.
    queries::add_course_to_track(&conn, &id, &docker).unwrap();

    // Finish Linux.
    let linux_lectures = queries::get_course_detail(&conn, &linux).unwrap().unwrap();
    for l in linux_lectures.sections.iter().flat_map(|s| &s.lectures) {
        queries::set_completed(&conn, &l.id, true).unwrap();
    }

    let order = |conn: &db::Connection| -> Vec<String> {
        let t = queries::get_track(conn, &id).unwrap().unwrap();
        t.courses.into_iter().map(|c| c.title).collect()
    };
    assert_eq!(order(&conn), ["Linux", "Docker", "Kubernetes"]);

    let list = queries::list_tracks(&conn).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].course_count, list[0].completed_lectures, list[0].total_lectures), (3, 2, 6));

    // Move Kubernetes up one.
    queries::reorder_track_courses(&conn, &id, &[linux.clone(), k8s.clone(), docker.clone()]).unwrap();
    assert_eq!(order(&conn), ["Linux", "Kubernetes", "Docker"]);

    queries::remove_course_from_track(&conn, &id, &k8s).unwrap();
    assert_eq!(order(&conn), ["Linux", "Docker"]);

    queries::update_track(&conn, &id, "Platform Engineering", None).unwrap();
    let t = queries::get_track(&conn, &id).unwrap().unwrap();
    assert_eq!((t.name.as_str(), t.description), ("Platform Engineering", None));

    // Deleting the track drops only the grouping.
    queries::delete_track(&conn, &id).unwrap();
    assert!(queries::get_track(&conn, &id).unwrap().is_none());
    assert!(queries::list_tracks(&conn).unwrap().is_empty());
    assert_eq!(queries::list_course_summaries(&conn).unwrap().len(), 3);
    let members: i64 = conn.query_row("SELECT COUNT(*) FROM track_courses", [], |r| r.get(0)).unwrap();
    assert_eq!(members, 0);
}
