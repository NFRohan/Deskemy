//! Validate the FTS search query against a Deskemy database (read-only).
//!   cargo run --example searchcheck -- "<db>" "<query>"

use deskemy_lib::db::queries;
use rusqlite::{Connection, OpenFlags};

fn main() {
    let mut args = std::env::args().skip(1);
    let path = args.next().expect("usage: searchcheck <db> <query>");
    let query = args.next().unwrap_or_else(|| "table".into());

    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let idx: i64 = conn
        .query_row("SELECT COUNT(*) FROM search_index", [], |r| r.get(0))
        .unwrap();
    println!("search_index rows: {idx}");

    let hits = queries::search(&conn, &query, 20).unwrap();
    println!("query {query:?} -> {} hits", hits.len());
    for h in hits {
        println!("  [{:<10}] {}  (course: {})", h.kind, h.title, h.course_title);
    }
}
