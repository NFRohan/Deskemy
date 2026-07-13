//! Manual validation runner: import a real course folder into an in-memory DB
//! and print the detected structure.
//!
//!   cargo run --example import_sample -- "D:\\path\\to\\Course Folder"

use deskemy_lib::db;
use deskemy_lib::importer::Importer;
use deskemy_lib::media::stub::StubProber;
use std::path::Path;

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("usage: import_sample <course_dir> [db_path]");

    // Optional 2nd arg: a persistent DB path (e.g. the app's data dir) to seed.
    let mut conn = match std::env::args().nth(2) {
        Some(db_path) => db::open(std::path::Path::new(&db_path)).unwrap(),
        None => db::open_in_memory().unwrap(),
    };
    let importer = Importer::new(Box::new(StubProber));
    let id = importer
        .import_course(&mut conn, None, Path::new(&path))
        .expect("import failed");

    let d = db::queries::get_course_detail(&conn, &id).unwrap().unwrap();
    let total: usize = d.sections.iter().map(|s| s.lectures.len()).sum();

    println!("Course:   {}", d.title);
    println!("Sections: {}", d.sections.len());
    println!("Lectures: {}", total);
    println!("Thumbnail: {:?}", d.thumbnail_path);
    println!();

    for s in &d.sections {
        println!("[{:>2}] {}  ({} lectures)", s.position + 1, s.title, s.lectures.len());
        for l in s.lectures.iter().take(2) {
            println!("       - {}", l.title);
        }
        if s.lectures.len() > 2 {
            println!("       ... +{} more", s.lectures.len() - 2);
        }
    }

    let subs: i64 = conn
        .query_row("SELECT COUNT(*) FROM subtitles", [], |r| r.get(0))
        .unwrap();
    let atts: i64 = conn
        .query_row("SELECT COUNT(*) FROM attachments", [], |r| r.get(0))
        .unwrap();
    let att_lec: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM attachments WHERE lecture_id IS NOT NULL",
            [],
            |r| r.get(0),
        )
        .unwrap();
    let fts: i64 = conn
        .query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))
        .unwrap();

    println!();
    println!(
        "Subtitles: {subs}   Attachments: {atts} (lecture-linked: {att_lec})   FTS rows: {fts}"
    );
}
