// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod course_page;
mod course_panel;
mod library;
mod session;
mod snapshot;
mod stats;
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
    /// `--snapshot <file.png> [course]`: render one frame offscreen and exit;
    /// `course` shows the most recently opened course's page.
    /// `--snapshot-player <file.png> [menu]` does the same for the player
    /// overlay, filled with sample state and optionally a menu open ("sleep",
    /// "speed", …); no mpv, so the video area stays blank.
    Snapshot { path: PathBuf, player: Option<String>, page: Option<String> },
    /// `--play <lecture id | video file>`: open straight into playback.
    Play(String),
}

fn parse_args() -> Mode {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        Some(flag @ ("--snapshot" | "--snapshot-player")) => {
            let path = args.next().unwrap_or("snapshot.png".into()).into();
            let extra = args.next();
            match flag {
                "--snapshot-player" => Mode::Snapshot { path, player: Some(extra.unwrap_or_default()), page: None },
                _ => Mode::Snapshot { path, player: None, page: extra },
            }
        }
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
    ui.global::<Theme>().set_mode(config.theme.as_str().into());
    wire_window(&ui);
    let library = library::LibraryPage::new(db.clone());
    library.reload(&ui);
    let thumbs = paths::data_dir().map(|d| d.join(deskemy_core::courses::THUMBNAILS_DIR));
    let course = course_page::CoursePage::new(db.clone(), thumbs);

    if let Mode::Snapshot { path, player, page } = mode {
        let window = offscreen.expect("snapshot platform installed");
        if let Some(menu) = player {
            snapshot::sample_playback(&ui, &menu, &db);
        }
        // "course", or "course-cover" / "course-delete" with that dialog open.
        if let Some(page) = page.as_deref().filter(|p| p.starts_with("course")) {
            snapshot::sample_course(&ui, &db, &course);
            let dialog = page.strip_prefix("course-").unwrap_or_default();
            ui.global::<Course>().set_dialog(dialog.into());
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
    ui.global::<Library>().on_open_course(move |course, resume| {
        let Some(ui) = weak.upgrade() else { return };
        let opened = library::lectures_to_open(&library_db, &course, &resume)
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
    let (weak, page) = (ui.as_weak(), library.clone());
    ui.global::<Library>().on_apply(move || {
        if let Some(ui) = weak.upgrade() {
            page.apply(&ui);
        }
    });
    // After watching or an edit, both views may be stale.
    let (weak, page, course_view) = (ui.as_weak(), library.clone(), course.clone());
    ui.on_refresh_library(move || {
        if let Some(ui) = weak.upgrade() {
            page.reload(&ui);
            course_view.refresh(&ui);
        }
    });
    wire_course(&ui, &course, player.session());

    let result = ui.run();
    player.shutdown();
    result
}

/// The course page's actions.
fn wire_course(ui: &AppWindow, page: &std::rc::Rc<course_page::CoursePage>, session: &std::sync::Arc<session::Session>) {
    let library = ui.global::<Library>();
    let (p, weak) = (page.clone(), ui.as_weak());
    library.on_show_course(move |id| {
        if let Some(ui) = weak.upgrade() {
            p.open(&ui, &id);
        }
    });

    let course = ui.global::<Course>();
    let (s, weak) = (session.clone(), ui.as_weak());
    course.on_play(move |lecture| {
        let Some(ui) = weak.upgrade() else { return };
        match s.open(&lecture) {
            Ok(()) => ui.set_playing(true),
            Err(e) => tracing::warn!(error = %e, %lecture, "open lecture"),
        }
    });
    // Each edit redraws the page and refreshes the library behind it.
    macro_rules! action {
        ($on:ident, |$page:ident, $ui:ident $(, $arg:ident)?| $body:expr) => {{
            let (p, weak) = (page.clone(), ui.as_weak());
            course.$on(move |$($arg)?| {
                if let Some($ui) = weak.upgrade() {
                    let $page = &p;
                    $body;
                    $ui.invoke_refresh_library();
                }
            });
        }};
    }
    action!(on_toggle_favorite, |p, ui| p.toggle_favorite(&ui));
    action!(on_toggle_complete, |p, ui, id| p.toggle_complete(&ui, &id));
    action!(on_add_tag, |p, ui, tag| p.add_tag(&ui, &tag));
    action!(on_remove_tag, |p, ui, tag| p.remove_tag(&ui, &tag));
    action!(on_pick_cover, |p, ui| p.pick_cover(&ui));
    action!(on_paste_cover, |p, ui| p.paste_cover(&ui));
    action!(on_clear_cover, |p, ui| p.clear_cover(&ui));
    action!(on_relocate, |p, ui| p.relocate(&ui));
    let (p, weak) = (page.clone(), ui.as_weak());
    course.on_delete_course(move || {
        let Some(ui) = weak.upgrade() else { return };
        if p.delete(&ui) {
            let course = ui.global::<Course>();
            course.set_dialog("".into());
            let nav = ui.global::<Nav>();
            nav.set_page("library".into());
            nav.set_crumbs(course_panel::model(vec!["Library".into()]));
            nav.set_crumb_targets(course_panel::model(vec!["".into()]));
            ui.invoke_refresh_library();
        }
    });
    let (p, weak) = (page.clone(), ui.as_weak());
    course.on_toggle_section(move |id| {
        if let Some(ui) = weak.upgrade() {
            p.toggle_section(&ui, &id);
        }
    });
    course.on_open_resource(|path| {
        if let Err(e) = open::that_detached(path.as_str()) {
            tracing::warn!(error = %e, %path, "open resource");
        }
    });
}

/// Frameless-window actions for the custom title bars.
fn wire_window(ui: &AppWindow) {
    let chrome = ui.global::<WindowChrome>();
    let weak = ui.as_weak();
    chrome.on_minimize(move || {
        if let Some(ui) = weak.upgrade() {
            ui.window().set_minimized(true);
        }
    });
    let weak = ui.as_weak();
    chrome.on_toggle_maximize(move || {
        if let Some(ui) = weak.upgrade() {
            let window = ui.window();
            window.set_maximized(!window.is_maximized());
        }
    });
    // Hiding the only window ends the event loop, so shutdown saves as usual.
    let weak = ui.as_weak();
    chrome.on_close(move || {
        if let Some(ui) = weak.upgrade() {
            let _ = ui.hide();
        }
    });
}

/// Fix section titles broken by the old folder-name cleaner ("04. IAM" → "04").
fn repair_titles(conn: &mut db::Connection) {
    match deskemy_core::importer::repair_section_titles(conn) {
        Ok(0) => {}
        Ok(n) => tracing::info!(sections = n, "repaired section titles"),
        Err(e) => tracing::warn!(error = %e, "repair section titles"),
    }
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
        Some(Ok(mut conn)) => {
            tracing::info!(db = %db_path.as_ref().unwrap().display(), "database ready");
            repair_titles(&mut conn);
            Some(conn)
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "could not open library");
            ui.global::<Library>().set_status_text(format!("Could not open the library: {e}").into());
            None
        }
        None => None,
    };
    let conn = conn.unwrap_or_else(|| db::open_in_memory().expect("in-memory database"));
    (Arc::new(Mutex::new(conn)), config)
}
