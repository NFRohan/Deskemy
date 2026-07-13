//! Validate the stats aggregate query against a Deskemy database (read-only).
//!   cargo run --example statscheck -- "<db>"

use deskemy_lib::db::queries;
use rusqlite::{Connection, OpenFlags};

fn main() {
    let path = std::env::args().nth(1).expect("usage: statscheck <db>");
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let s = queries::stats(&conn).unwrap();
    println!("{s:#?}");
}
