// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod snapshot;
mod video;

use deskemy_core::{db, paths};
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

slint::include_modules!();

enum Mode {
    /// The normal app.
    Window,
    /// `--snapshot <file.png>`: render one frame offscreen and exit.
    /// `--snapshot-player <file.png>` does the same for the player overlay,
    /// filled with sample state (no mpv; the video area stays blank).
    Snapshot { path: PathBuf, player: bool },
    /// `--play <file>`: open straight into playback (port spike).
    Play(PathBuf),
}

fn parse_args() -> Mode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag @ ("--snapshot" | "--snapshot-player")) => Mode::Snapshot {
            player: flag == "--snapshot-player",
            path: args.next().unwrap_or("snapshot.png".into()).into(),
        },
        Some("--play") => match args.next() {
            Some(file) => Mode::Play(file.into()),
            None => Mode::Window,
        },
        _ => Mode::Window,
    }
}

fn main() -> Result<(), slint::PlatformError> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("deskemy=debug,deskemy_core=debug,info")),
        )
        .init();

    let mode = parse_args();
    let offscreen = match &mode {
        Mode::Snapshot { .. } => Some(snapshot::install(1280, 800)?),
        // Video is rendered by mpv through OpenGL, so real windows need an
        // OpenGL-backed renderer (FemtoVG by default).
        _ => {
            slint::BackendSelector::new().require_opengl().select()?;
            None
        }
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

    let player = match mode {
        Mode::Snapshot { path, player } => {
            let window = offscreen.expect("snapshot platform installed");
            if player {
                snapshot::sample_playback(&ui);
            }
            ui.show()?;
            snapshot::save(&window, &path).map_err(slint::PlatformError::Other)?;
            tracing::info!(path = %path.display(), "snapshot written");
            return Ok(());
        }
        Mode::Play(file) => {
            ui.set_playing(true);
            Some(video::Player::start(&ui, file).map_err(slint::PlatformError::Other)?)
        }
        Mode::Window => None,
    };

    let result = ui.run();
    if let Some(player) = player {
        player.shutdown();
    }
    result
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
