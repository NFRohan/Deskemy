//! Build the subtitle index from a Deskemy db's sidecar subs and search it.
//!   cargo run --example subcheck -- "<db>" "<query>"
//! (Requires the app to have already created the subtitle_index table.)

use deskemy_lib::db::{self, queries};
use deskemy_lib::subtitles;

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: subcheck <db> <query>");
    let query = args.next().unwrap_or_else(|| "database".into());

    // db::open runs migrations; also self-heal a dev DB whose version was
    // bumped past 4 before the table existed.
    let mut conn = db::open(std::path::Path::new(&path)).unwrap();
    conn.execute_batch(
        "CREATE VIRTUAL TABLE IF NOT EXISTS subtitle_index USING fts5(
             lecture_id UNINDEXED, course_id UNINDEXED, start_ms UNINDEXED,
             text, tokenize='unicode61');",
    )
    .unwrap();
    let files = queries::all_subtitle_files(&conn).unwrap();
    println!("sidecar subtitle files: {}", files.len());

    let tx = conn.transaction().unwrap();
    queries::clear_subtitle_index(&tx).unwrap();
    let mut total = 0i64;
    for (lid, cid, p) in &files {
        if let Ok(bytes) = std::fs::read(p) {
            for (ms, text) in subtitles::parse(&String::from_utf8_lossy(&bytes)) {
                queries::insert_subtitle_cue(&tx, lid, cid, ms, &text).unwrap();
                total += 1;
            }
        }
    }
    tx.commit().unwrap();
    println!("indexed cues: {total}");

    let hits = queries::subtitle_search(&conn, &query, 10).unwrap();
    println!("query {query:?} -> {} hits", hits.len());
    for h in hits {
        println!("  [{:>7}ms] {} | {}", h.start_ms, h.lecture_title, h.snippet);
    }
}
