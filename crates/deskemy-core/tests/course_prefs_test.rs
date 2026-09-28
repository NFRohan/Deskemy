//! Per-course playback prefs: a saved speed overrides the default until it's
//! cleared (for one course, or all), and clearing keeps the track choices.

use deskemy_core::db::{self, queries};
use deskemy_core::importer::Importer;
use deskemy_core::media::stub::StubProber;
use std::fs;

#[test]
fn clearing_speeds_keeps_track_choices() {
    let tmp = tempfile::tempdir().unwrap();
    let mut conn = db::open_in_memory().unwrap();
    let importer = Importer::new(Box::new(StubProber));
    let mut import = |name: &str| {
        let dir = tmp.path().join(name);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("001 Lecture.mp4"), b"x").unwrap();
        importer.import_course(&mut conn, None, &dir).unwrap()
    };
    let (linux, docker, k8s) = (import("Linux"), import("Docker"), import("Kubernetes"));

    queries::set_pref_speed(&conn, &linux, 1.0).unwrap();
    queries::set_pref_subtitle(&conn, &linux, Some(2)).unwrap();
    queries::set_pref_speed(&conn, &docker, 1.5).unwrap();
    queries::set_pref_speed(&conn, &k8s, 2.0).unwrap();
    let speed = |conn: &db::Connection, id: &str| queries::get_course_prefs(conn, id).unwrap().and_then(|p| p.0);

    // One course.
    assert_eq!(queries::clear_pref_speed(&conn, Some(&linux)).unwrap(), 1);
    assert_eq!(speed(&conn, &linux), None);
    assert_eq!(queries::get_course_prefs(&conn, &linux).unwrap().unwrap().1, Some(2), "subtitle kept");
    assert_eq!(speed(&conn, &docker), Some(1.5));

    // Every course; already-cleared ones don't count.
    assert_eq!(queries::clear_pref_speed(&conn, None).unwrap(), 2);
    assert_eq!((speed(&conn, &docker), speed(&conn, &k8s)), (None, None));
}
