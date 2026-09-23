// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod snapshot;

use deskemy_core::{db, paths};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("deskemy=debug,deskemy_core=debug,info")),
        )
        .init();

    let mut args = std::env::args().skip(1);
    let snapshot_path = match args.next().as_deref() {
        Some("--snapshot") => Some(PathBuf::from(args.next().unwrap_or("snapshot.png".into()))),
        _ => None,
    };
    let offscreen = match &snapshot_path {
        Some(_) => Some(snapshot::install(1280, 800)?),
        None => None,
    };

    let ui = AppWindow::new()?;

    match load_courses() {
        Ok(rows) => {
            ui.set_status(format!("{} courses", rows.len()).into());
            ui.set_courses(slint::ModelRc::new(slint::VecModel::from(rows)));
        }
        Err(e) => {
            tracing::error!(error = %e, "could not open library");
            ui.set_status(format!("Could not open library: {e}").into());
        }
    }

    if let (Some(window), Some(path)) = (offscreen, snapshot_path) {
        ui.show()?;
        snapshot::save(&window, &path).map_err(slint::PlatformError::Other)?;
        tracing::info!(path = %path.display(), "snapshot written");
        return Ok(());
    }

    ui.run()
}

fn load_courses() -> Result<Vec<CourseRow>, String> {
    let dir = paths::data_dir().ok_or("no data directory for this platform")?;
    let db_path = dir.join(paths::DB_FILE);
    if !db_path.exists() {
        return Err(format!("no library at {}", db_path.display()));
    }
    let conn = db::open(&db_path).map_err(|e| e.to_string())?;
    tracing::info!(db = %db_path.display(), "database ready");
    let courses = db::queries::list_course_summaries(&conn).map_err(|e| e.to_string())?;
    Ok(courses
        .into_iter()
        .map(|c| CourseRow {
            title: c.title.into(),
            lectures: c.lecture_count as i32,
            completed: c.completed_count as i32,
        })
        .collect())
}
