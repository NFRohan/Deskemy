//! Inspect progress/completion state in a Deskemy database.
//!   cargo run --example dbcheck -- "<path to deskemy.db>"

use deskemy_lib::db;
use std::path::Path;

fn main() {
    let path = std::env::args().nth(1).expect("usage: dbcheck <db>");
    let conn = db::open(Path::new(&path)).unwrap();

    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM progress", [], |r| r.get(0))
        .unwrap();
    let done: i64 = conn
        .query_row("SELECT COUNT(*) FROM progress WHERE completed = 1", [], |r| r.get(0))
        .unwrap();
    println!("progress rows: {total}   completed: {done}\n");

    let mut stmt = conn
        .prepare(
            "SELECT l.title, p.position_seconds, p.completed, l.duration
               FROM progress p JOIN lectures l ON l.id = p.lecture_id
              ORDER BY p.last_watched_at DESC LIMIT 15",
        )
        .unwrap();
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, f64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Option<f64>>(3)?,
            ))
        })
        .unwrap();

    println!("done  pos     dur     title");
    for row in rows {
        let (title, pos, completed, dur) = row.unwrap();
        println!(
            " {}   {:>6.0}  {:>6}  {}",
            if completed == 1 { "x" } else { " " },
            pos,
            dur.map(|d| format!("{d:.0}")).unwrap_or_else(|| "—".into()),
            title
        );
    }
}
