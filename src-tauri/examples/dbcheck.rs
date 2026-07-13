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

    // Course pointer + ordered list around it.
    let (cid, ctitle, last_lec): (String, String, Option<String>) = conn
        .query_row(
            "SELECT id, title, last_lecture_id FROM courses ORDER BY last_opened_at DESC LIMIT 1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    println!("\ncourse: {ctitle}\nlast_lecture_id: {last_lec:?}");

    let mut s3 = conn
        .prepare(
            "SELECT l.id, l.title, l.position, COALESCE(p.completed,0), COALESCE(p.position_seconds,0)
               FROM lectures l LEFT JOIN progress p ON p.lecture_id=l.id
              WHERE l.course_id=?1 ORDER BY l.position LIMIT 25",
        )
        .unwrap();
    let r3 = s3
        .query_map([&cid], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, f64>(4)?,
            ))
        })
        .unwrap();
    println!("\n#   done resume-> pos    title");
    for row in r3 {
        let (id, title, pos, done, ppos) = row.unwrap();
        let is_last = last_lec.as_deref() == Some(id.as_str());
        println!(
            "{:>2}   [{}]   {}   {:>5.0}  {}",
            pos,
            if done == 1 { "x" } else { " " },
            if is_last { "<==" } else { "   " },
            ppos,
            title
        );
    }
}
