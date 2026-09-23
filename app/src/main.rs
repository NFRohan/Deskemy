// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod course_panel;
mod library;
mod session;
mod snapshot;
mod tracks;
mod video;

use deskemy_core::config::AppConfig;
use deskemy_core::{db, paths};
use session::Db;
use slint::ComponentHandle;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tracing_subscriber::EnvFilter;

slint::include_modules!();

enum Mode {
    /// The normal app.
    Window,
    /// `--snapshot <file.png>`: render one frame offscreen and exit.
    /// `--snapshot-player <file.png> [menu]` does the same for the player
    /// overlay, filled with sample state and optionally a menu open ("sleep",
    /// "speed", …); no mpv, so the video area stays blank.
    Snapshot { path: PathBuf, player: Option<String> },
    /// `--play <lecture id | video file>`: open straight into playback.
    Play(String),
}

fn parse_args() -> Mode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag @ ("--snapshot" | "--snapshot-player")) => Mode::Snapshot {
            path: args.next().unwrap_or("snapshot.png".into()).into(),
            player: (flag == "--snapshot-player").then(|| args.next().unwrap_or_default()),
        },
        Some("--play") => args.next().map_or(Mode::Window, Mode::Play),
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
    let (db, config) = open_library(&ui);
    library::show(&ui, &db);

    if let Mode::Snapshot { path, player } = mode {
        let window = offscreen.expect("snapshot platform installed");
        if let Some(menu) = player {
            snapshot::sample_playback(&ui, &menu, &db);
        }
        ui.show()?;
        snapshot::save(&window, &path).map_err(slint::PlatformError::Other)?;
        tracing::info!(path = %path.display(), "snapshot written");
        return Ok(());
    }

    let on_ready: Option<video::OnReady> = match mode {
        Mode::Play(target) => {
            ui.set_playing(true);
            Some(Box::new(move |session: &session::Session| {
                let file = PathBuf::from(&target);
                let opened = if file.is_file() {
                    session.open_file(&file)
                } else {
                    session.open(&target)
                };
                if let Err(e) = opened {
                    tracing::error!(error = %e, %target, "open");
                }
            }))
        }
        _ => None,
    };
    let player = video::Player::start(&ui, db.clone(), config, on_ready)
        .map_err(slint::PlatformError::Other)?;

    let (session, weak, library_db) = (player.session().clone(), ui.as_weak(), db.clone());
    ui.on_open_course(move |row| {
        let Some(ui) = weak.upgrade() else { return };
        let opened = library::lectures_to_open(&library_db, &row)
            .into_iter()
            .any(|id| match session.open(&id) {
                Ok(()) => true,
                Err(e) => {
                    tracing::warn!(error = %e, lecture = %id, "open lecture");
                    false
                }
            });
        if opened {
            ui.set_playing(true);
        }
    });
    let (weak, library_db) = (ui.as_weak(), db.clone());
    ui.on_refresh_library(move || {
        if let Some(ui) = weak.upgrade() {
            library::show(&ui, &library_db);
        }
    });

    let result = ui.run();
    player.shutdown();
    result
}

/// Open the user's library and config. Without a library yet (fresh install)
/// the app still runs on an empty in-memory one, so files can be played.
fn open_library(ui: &AppWindow) -> (Db, AppConfig) {
    let dir = paths::data_dir();
    let config = dir
        .as_ref()
        .map(|d| d.join(paths::CONFIG_FILE))
        .and_then(|p| AppConfig::load(&p).ok())
        .unwrap_or_default();

    let db_path = dir.map(|d| d.join(paths::DB_FILE));
    let conn = match db_path.as_ref().filter(|p| p.exists()).map(|p| db::open(p)) {
        Some(Ok(conn)) => {
            tracing::info!(db = %db_path.as_ref().unwrap().display(), "database ready");
            Some(conn)
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "could not open library");
            ui.set_status(format!("Could not open library: {e}").into());
            None
        }
        None => None,
    };
    let conn = conn.unwrap_or_else(|| db::open_in_memory().expect("in-memory database"));
    (Arc::new(Mutex::new(conn)), config)
}
