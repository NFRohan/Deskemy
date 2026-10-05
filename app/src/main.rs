// Release builds are GUI-only on Windows (no console window).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod career;
mod course_page;
mod course_panel;
mod importing;
mod library;
mod mini;
mod pages;
mod search;
mod session;
mod settings;
mod snapshot;
mod stats;
mod stats_page;
mod tracks;
mod updates;
mod video;

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
        // Release check: does this signed installer verify against the key
        // this build trusts? Prints the answer (and exits 1 if not).
        Some("--verify-update") => {
            let (Some(installer), Some(sig)) = (args.next(), args.next()) else {
                eprintln!("usage: deskemy --verify-update <installer.exe> <installer.exe.sig>");
                std::process::exit(2);
            };
            match updates::verify_file(std::path::Path::new(&installer), std::path::Path::new(&sig)) {
                Ok(()) => {
                    println!("OK: {installer} is signed with the release key.");
                    std::process::exit(0);
                }
                Err(e) => {
                    eprintln!("NOT VERIFIED: {e}");
                    std::process::exit(1);
                }
            }
        }
        _ => Mode::Window,
    }
}

/// Where this run logs: the console in debug builds; in release (no console)
/// `<data dir>/logs/deskemy.log`, rewritten each launch with the previous
/// run's kept as `deskemy.prev.log`.
fn log_file() -> Option<std::fs::File> {
    if cfg!(debug_assertions) {
        return None;
    }
    let dir = paths::data_dir()?.join("logs");
    std::fs::create_dir_all(&dir).ok()?;
    let log = dir.join("deskemy.log");
    let _ = std::fs::rename(&log, dir.join("deskemy.prev.log"));
    std::fs::File::create(log).ok()
}

fn main() -> Result<(), slint::PlatformError> {
    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("deskemy=debug,deskemy_core=debug,info"));
    match log_file() {
        Some(file) => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .with_writer(Mutex::new(file))
            .init(),
        None => tracing_subscriber::fmt().with_env_filter(filter).init(),
    }
    // A panic in release would otherwise vanish with no console to print to.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        tracing::error!(%info, "panic");
        default_hook(info);
    }));

    let mode = parse_args();
    let offscreen = match &mode {
        // DESKEMY_SNAPSHOT_HEIGHT renders a taller frame, to see a long page
        // whole; DESKEMY_SNAPSHOT_WIDTH a narrower one (the mini player).
        Mode::Snapshot { .. } => {
            let dimension = |name: &str| std::env::var(name).ok().and_then(|v| v.parse().ok());
            let width = dimension("DESKEMY_SNAPSHOT_WIDTH").unwrap_or(1280);
            Some(snapshot::install(width, dimension("DESKEMY_SNAPSHOT_HEIGHT").unwrap_or(800))?)
        }
        // Video is rendered by mpv through OpenGL, so real windows need an
        // OpenGL-backed renderer: Skia, for its text, or FemtoVG with
        // DESKEMY_RENDERER=femtovg.
        _ => {
            let renderer = std::env::var("DESKEMY_RENDERER").unwrap_or_else(|_| "skia-opengl".into());
            tracing::info!(%renderer, "renderer");
            slint::BackendSelector::new().renderer_name(renderer).require_opengl().select()?;
            None
        }
    };

    let ui = AppWindow::new()?;
    let (db, config) = open_library(&ui);
    let prefs = std::rc::Rc::new(settings::SettingsPage::new(config.clone(), db.clone(), paths::data_dir()));
    // Theme, and what the player reads from config.
    prefs.show(&ui);
    ui.global::<Prefs>().set_version(settings::VERSION.into());
    wire_window(&ui);
    wire_mouse_back(&ui);
    let library = library::LibraryPage::new(db.clone());
    library.reload(&ui);
    let thumbs = paths::data_dir().map(|d| d.join(deskemy_core::courses::THUMBNAILS_DIR));
    let course = course_page::CoursePage::new(db.clone(), config.clone(), thumbs);
    let tracks = career::TracksPage::new(db.clone(), library.clone());

    if let Mode::Snapshot { path, player, page } = mode {
        let window = offscreen.expect("snapshot platform installed");
        if let Some(menu) = player {
            snapshot::sample_playback(&ui, &menu, &db);
        }
        if let Some(list @ ("favorites" | "history" | "bookmarks" | "stats" | "settings")) = page.as_deref() {
            let nav = ui.global::<Nav>();
            nav.set_page(list.into());
            let title = format!("{}{}", list[..1].to_uppercase(), &list[1..]);
            nav.set_crumbs(course_panel::model(vec![title.into()]));
            show_list(&ui, &db, &library, list, &config);
            prefs.show_storage(&ui);
        }
        // "settings-run:<action>" runs a maintenance action (inline — no event
        // loop here) and shows Settings with its result.
        if let Some(key) = page.as_deref().and_then(|p| p.strip_prefix("settings-run:")) {
            ui.global::<Nav>().set_page("settings".into());
            ui.global::<Nav>().set_crumbs(course_panel::model(vec!["Settings".into()]));
            let message = settings::run_task(key, &db, paths::data_dir().as_deref());
            ui.global::<Prefs>().invoke_set_result(key.into(), message.into());
            prefs.show_storage(&ui);
        }
        // "import-preview" / "import-scanning": the Add Folder flow, with sample
        // figures (no folder is probed).
        if page.as_deref() == Some("import-preview") {
            snapshot::sample_import_preview(&ui);
        }
        // "import-probe:<folder>" probes a real folder (reading it only) and
        // shows its preview.
        if let Some(folder) = page.as_deref().and_then(|p| p.strip_prefix("import-probe:")) {
            let importer = importing::Importing::new(db.clone(), config.clone());
            let started = std::time::Instant::now();
            match importing::probe(importer.importer(), &db, std::path::Path::new(folder), true, |_, _| {}) {
                Ok((preview, ..)) => importing::show_preview(&ui.global::<Import>(), &preview),
                Err(e) => ui.global::<Import>().set_error(e.into()),
            }
            tracing::info!(elapsed = ?started.elapsed(), "probed");
        }
        // "import-course:<folder>" imports a real folder (reading it only) into
        // the data directory's library and shows its page.
        if let Some(folder) = page.as_deref().and_then(|p| p.strip_prefix("import-course:")) {
            let importer = importing::Importing::new(db.clone(), config.clone());
            match importing::probe(importer.importer(), &db, std::path::Path::new(folder), true, |_, _| {}) {
                Ok((_, snap, plan)) => {
                    let mut conn = db.lock().unwrap_or_else(|e| e.into_inner());
                    match importer.importer().persist(&mut conn, None, &snap, &plan) {
                        Ok(id) => {
                            drop(conn);
                            course.show(&ui, &id);
                        }
                        Err(e) => tracing::error!(error = %e, "import"),
                    }
                }
                Err(e) => tracing::error!(error = %e, "probe"),
            }
        }
        if page.as_deref() == Some("import-scanning") {
            ui.global::<Import>().set_scanning(true);
            ui.global::<Import>().set_progress(importing::progress_label(7, 42).into());
        }
        // "update:<page>": that page with an update on offer (banner, Settings).
        if let Some(rest) = page.as_deref().and_then(|p| p.strip_prefix("update:")) {
            let updates = ui.global::<Updates>();
            updates.set_version("2.1.0".into());
            updates.set_status(if rest.ends_with("-downloading") { "downloading" } else { "available" }.into());
            updates.set_progress(42);
            let name = rest.trim_end_matches("-downloading");
            ui.global::<Nav>().set_page(name.into());
            ui.global::<Nav>().set_crumbs(course_panel::model(vec![if name == "settings" { "Settings" } else { "Library" }.into()]));
        }
        // "settings-import": Settings with the import confirmation open.
        if page.as_deref() == Some("settings-import") {
            ui.global::<Nav>().set_page("settings".into());
            ui.global::<Nav>().set_crumbs(course_panel::model(vec!["Settings".into()]));
            ui.global::<Prefs>().set_import_confirm(true);
        }
        // "search:<query>" shows the search page with that query's results.
        if let Some(query) = page.as_deref().and_then(|p| p.strip_prefix("search:")) {
            ui.global::<Nav>().set_page("search".into());
            ui.global::<Nav>().set_crumbs(course_panel::model(vec!["Search".into()]));
            ui.global::<Search>().set_query(query.into());
            search::run(&ui, &db, query);
        }
        // "tracks" (the list), or "track" — the first track — optionally with a
        // dialog open ("track-add", "track-edit", "track-delete").
        if let Some(page) = page.as_deref().filter(|p| p.starts_with("track")) {
            snapshot::sample_tracks(&ui, &tracks, page);
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
    let player = video::Player::start(&ui, db.clone(), config.clone(), on_ready)
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
    // After watching or an edit, every view may be stale.
    let (weak, page, course_view, track_view, lists_db, cfg) =
        (ui.as_weak(), library.clone(), course.clone(), tracks.clone(), db.clone(), config.clone());
    ui.on_refresh_library(move || {
        if let Some(ui) = weak.upgrade() {
            page.reload(&ui);
            course_view.refresh(&ui);
            track_view.refresh(&ui);
            show_list(&ui, &lists_db, &page, &ui.global::<Nav>().get_page(), &cfg);
            if ui.global::<Nav>().get_page() == "tracks" {
                track_view.show_list(&ui);
            }
        }
    });
    let (weak, page, track_view, lists_db, cfg, p) =
        (ui.as_weak(), library.clone(), tracks.clone(), db.clone(), config.clone(), prefs.clone());
    ui.on_page_shown(move |name| {
        if let Some(ui) = weak.upgrade() {
            show_list(&ui, &lists_db, &page, &name, &cfg);
            match name.as_str() {
                "tracks" => track_view.show_list(&ui),
                "settings" => p.show_storage(&ui),
                _ => {}
            }
        }
    });
    wire_tracks(&ui, &tracks);
    let importing = std::rc::Rc::new(importing::Importing::new(db.clone(), config.clone()));
    importing.start_watching(&ui, player.session().clone());
    wire_import(&ui, &importing);
    let mini = mini::MiniPlayer::new(&ui, config.clone());

    // Updates: a quiet check shortly after launch (the Tauri app's), and
    // sweep the installer a previous update left in the temp dir.
    let updater = updates::Updater::new();
    std::thread::spawn(updates::sweep);
    let (u, weak) = (updater.clone(), ui.as_weak());
    ui.global::<Updates>().on_check(move || {
        if let Some(ui) = weak.upgrade() {
            u.check(&ui, false);
        }
    });
    let (u, weak) = (updater.clone(), ui.as_weak());
    ui.global::<Updates>().on_install(move || {
        if let Some(ui) = weak.upgrade() {
            u.install(&ui);
        }
    });
    let (u, weak) = (updater.clone(), ui.as_weak());
    slint::Timer::single_shot(std::time::Duration::from_secs(3), move || {
        if let Some(ui) = weak.upgrade() {
            u.check(&ui, true);
        }
    });
    let (p, weak) = (prefs.clone(), ui.as_weak());
    ui.global::<Prefs>().on_set(move |key, value| {
        if let Some(ui) = weak.upgrade() {
            p.change(&ui, &key, &value);
            // Some settings change what pages show (resources inline).
            ui.invoke_refresh_library();
        }
    });
    let (p, weak) = (prefs.clone(), ui.as_weak());
    ui.global::<Prefs>().on_run(move |key| {
        if let Some(ui) = weak.upgrade() {
            p.run(&ui, &key);
        }
    });
    let (p, weak) = (prefs.clone(), ui.as_weak());
    ui.global::<Prefs>().on_confirm_import(move || {
        if let Some(ui) = weak.upgrade() {
            p.confirm_import(&ui);
        }
    });
    wire_lists(&ui, &db, player.session());
    wire_search(&ui, &db, &course, player.session());
    wire_course(&ui, &course, player.session(), &importing);

    log_launch_window(&ui);
    let result = ui.run();
    // Quitting from the mini player: keep where it sat for next time.
    mini.remember(&ui);
    player.shutdown();

    // A staged backup import is swapped in by the next start. Let go of the
    // library first — the window's callbacks hold it too — so the files
    // aren't locked when it does.
    let restart = prefs.restart_requested();
    let installer = updater.pending_installer();
    drop((ui, library, course, tracks, prefs, importing, updater, db, config));
    // A downloaded, verified update: its installer replaces this build and
    // relaunches it.
    if let Some(installer) = installer {
        match updates::run_installer(&installer) {
            Ok(()) => tracing::info!(installer = %installer.display(), "installing the update"),
            Err(e) => tracing::error!(error = %e, "run the update installer"),
        }
        return result;
    }
    if restart {
        match std::env::current_exe().and_then(|exe| std::process::Command::new(exe).spawn()) {
            Ok(_) => tracing::info!("relaunching to apply the imported backup"),
            Err(e) => tracing::error!(error = %e, "relaunch after import"),
        }
    }
    result
}

/// Load a simple page's data (Favorites, History, Bookmarks, Stats) when it
/// shows.
fn show_list(ui: &AppWindow, db: &Db, library: &library::LibraryPage, page: &str, config: &settings::Config) {
    match page {
        "favorites" => ui.global::<Lists>().set_favorites(course_panel::model(library.favorites())),
        "history" | "bookmarks" => pages::show(ui, db, page),
        "stats" => stats_page::show(ui, db, settings::lock(config).daily_goal_minutes),
        _ => {}
    }
}

fn wire_lists(ui: &AppWindow, db: &Db, session: &std::sync::Arc<session::Session>) {
    let lists = ui.global::<Lists>();
    let (s, weak) = (session.clone(), ui.as_weak());
    lists.on_play_at(move |lecture, start| {
        let Some(ui) = weak.upgrade() else { return };
        match s.open_at(&lecture, Some(start as f64)) {
            Ok(()) => ui.set_playing(true),
            Err(e) => tracing::warn!(error = %e, %lecture, "open lecture"),
        }
    });
    let (db, weak) = (db.clone(), ui.as_weak());
    lists.on_delete_bookmark(move |id| {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = deskemy_core::db::queries::delete_bookmark(&conn, &id) {
            tracing::warn!(error = %e, "delete bookmark");
        }
        drop(conn);
        if let Some(ui) = weak.upgrade() {
            pages::show(&ui, &db, "bookmarks");
        }
    });
}

fn wire_search(
    ui: &AppWindow,
    db: &Db,
    course: &std::rc::Rc<course_page::CoursePage>,
    session: &std::sync::Arc<session::Session>,
) {
    let search = ui.global::<Search>();
    let (db, weak) = (db.clone(), ui.as_weak());
    search.on_run(move |query| {
        if let Some(ui) = weak.upgrade() {
            search::run(&ui, &db, &query);
        }
    });
    // Lectures play (resuming); anything else opens its course's page.
    let (s, page, weak) = (session.clone(), course.clone(), ui.as_weak());
    search.on_open(move |hit| {
        let Some(ui) = weak.upgrade() else { return };
        if hit.kind == "lecture" {
            match s.open(&hit.id) {
                Ok(()) => ui.set_playing(true),
                Err(e) => tracing::warn!(error = %e, "open lecture"),
            }
        } else {
            page.open(&ui, &hit.course);
        }
    });
    let (s, weak) = (session.clone(), ui.as_weak());
    search.on_play_at(move |lecture, start| {
        let Some(ui) = weak.upgrade() else { return };
        match s.open_at(&lecture, Some(start as f64)) {
            Ok(()) => ui.set_playing(true),
            Err(e) => tracing::warn!(error = %e, %lecture, "open lecture"),
        }
    });
}

/// Add Folder: pick, probe, preview, import.
fn wire_import(ui: &AppWindow, importing: &std::rc::Rc<importing::Importing>) {
    let import = ui.global::<Import>();
    let (i, weak) = (importing.clone(), ui.as_weak());
    import.on_add_folder(move || {
        if let Some(ui) = weak.upgrade() {
            i.add_folder(&ui);
        }
    });
    let (i, weak) = (importing.clone(), ui.as_weak());
    import.on_confirm(move || {
        if let Some(ui) = weak.upgrade() {
            i.confirm(&ui);
        }
    });
    let (i, weak) = (importing.clone(), ui.as_weak());
    import.on_cancel(move || {
        if let Some(ui) = weak.upgrade() {
            i.cancel(&ui);
        }
    });
}

/// The career tracks pages' actions. Membership and order feed the library's
/// track filter, so edits refresh it too.
fn wire_tracks(ui: &AppWindow, page: &std::rc::Rc<career::TracksPage>) {
    let tracks = ui.global::<Tracks>();
    macro_rules! action {
        ($on:ident, |$page:ident, $ui:ident $(, $arg:ident)*| $body:expr) => {{
            let (p, weak) = (page.clone(), ui.as_weak());
            tracks.$on(move |$($arg),*| {
                if let Some($ui) = weak.upgrade() {
                    let $page = &p;
                    $body;
                }
            });
        }};
    }
    action!(on_open, |p, ui, id| p.open(&ui, &id));
    action!(on_filter, |p, ui, query| p.filter(&ui, &query));
    action!(on_create, |p, ui, name, description| {
        p.create(&ui, &name, &description);
        ui.invoke_refresh_library()
    });
    action!(on_save, |p, ui, name, description| {
        p.save(&ui, &name, &description);
        ui.invoke_refresh_library()
    });
    action!(on_delete_track, |p, ui| {
        p.delete(&ui);
        ui.invoke_refresh_library()
    });
    action!(on_add_course, |p, ui, id| {
        p.add(&ui, &id);
        ui.invoke_refresh_library()
    });
    action!(on_remove_course, |p, ui, id| {
        p.remove(&ui, &id);
        ui.invoke_refresh_library()
    });
    action!(on_move, |p, ui, index, dir| {
        p.move_course(&ui, index.max(0) as usize, dir);
        ui.invoke_refresh_library()
    });
}

/// The course page's actions.
fn wire_course(
    ui: &AppWindow,
    page: &std::rc::Rc<course_page::CoursePage>,
    session: &std::sync::Arc<session::Session>,
    importing: &std::rc::Rc<importing::Importing>,
) {
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
    let watch = importing.clone();
    action!(on_relocate, |p, ui| {
        p.relocate(&ui);
        // The course's new folder.
        watch.sync_watcher()
    });
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
    let (p, weak) = (page.clone(), ui.as_weak());
    course.on_toggle_resource(move |id, done| {
        if let Some(ui) = weak.upgrade() {
            p.set_resource_done(&ui, &id, done);
        }
    });
    course.on_open_resource(|path| {
        if let Err(e) = open::that_detached(path.as_str()) {
            tracing::warn!(error = %e, %path, "open resource");
        }
    });
}

/// Diagnostics for "the first run after installing comes up minimized / behind
/// other windows" (roadmap): who launched Deskemy, and the window's state a
/// moment after it's up — minimized, visible, in front — logged at 0.3s, 1s
/// and 3s.
#[cfg(windows)]
fn log_launch_window(ui: &AppWindow) {
    tracing::info!(launcher = %parent_process_name().unwrap_or_else(|| "?".into()), "launched by");
    for ms in [300, 1000, 3000] {
        let weak = ui.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(ms), move || {
            let Some(ui) = weak.upgrade() else { return };
            let Some(hwnd) = mini::hwnd(&ui) else { return };
            use windows_sys::Win32::UI::WindowsAndMessaging::{
                GetForegroundWindow, GetWindowPlacement, IsIconic, IsWindowVisible, WINDOWPLACEMENT,
            };
            // SAFETY: a live window handle; WINDOWPLACEMENT is sized for the call.
            let (minimized, visible, foreground, show) = unsafe {
                let mut p: WINDOWPLACEMENT = std::mem::zeroed();
                p.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
                let show = (GetWindowPlacement(hwnd, &mut p) != 0).then_some(p.showCmd);
                (IsIconic(hwnd) != 0, IsWindowVisible(hwnd) != 0, GetForegroundWindow() == hwnd, show)
            };
            tracing::info!(after_ms = ms, minimized, visible, foreground, ?show, "launch window state");
        });
    }
}

#[cfg(not(windows))]
fn log_launch_window(_: &AppWindow) {}

/// The executable name of the process that started this one.
#[cfg(windows)]
fn parent_process_name() -> Option<String> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcessId;
    // SAFETY: a process snapshot walked with a correctly sized entry, then closed.
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return None;
        }
        let name = |e: &PROCESSENTRY32W| {
            let len = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            String::from_utf16_lossy(&e.szExeFile[..len])
        };
        let mut entries = Vec::new();
        let mut e: PROCESSENTRY32W = std::mem::zeroed();
        e.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        if Process32FirstW(snap, &mut e) != 0 {
            loop {
                entries.push((e.th32ProcessID, e.th32ParentProcessID, name(&e)));
                if Process32NextW(snap, &mut e) == 0 {
                    break;
                }
            }
        }
        CloseHandle(snap);
        let me = GetCurrentProcessId();
        let parent = entries.iter().find(|(pid, _, _)| *pid == me)?.1;
        Some(
            entries
                .iter()
                .find(|(pid, _, _)| *pid == parent)
                .map(|(_, _, n)| n.clone())
                .unwrap_or_else(|| format!("pid {parent} (exited)")),
        )
    }
}

/// The mouse's back button, anywhere in the player's window — over the
/// video, the control bar, the title bar or the mini player (a button under
/// the pointer would otherwise take the click). As the browser's back did in
/// the Tauri app: close an open dialog or menu first, then leave the mini
/// player, then the player.
fn wire_mouse_back(ui: &AppWindow) {
    use slint::winit_030::winit::event::{ElementState, MouseButton, WindowEvent};
    use slint::winit_030::{EventResult, WinitWindowAccessor};
    let weak = ui.as_weak();
    ui.window().on_winit_window_event(move |_, event| {
        let WindowEvent::MouseInput { button: MouseButton::Back, state, .. } = event else {
            return EventResult::Propagate;
        };
        let Some(ui) = weak.upgrade() else { return EventResult::Propagate };
        if !ui.get_playing() {
            return EventResult::Propagate;
        }
        if *state == ElementState::Released {
            let playback = ui.global::<Playback>();
            match playback.get_open_menu().as_str() {
                "bookmark" => playback.invoke_close_bookmark(),
                "" if playback.get_mini() => playback.invoke_toggle_mini(),
                "" => playback.invoke_back(),
                _ => playback.set_open_menu("".into()),
            }
        }
        EventResult::PreventDefault
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
fn open_library(ui: &AppWindow) -> (Db, settings::Config) {
    let dir = paths::data_dir();
    // A backup import staged from Settings is swapped in now, before
    // anything opens the database.
    if let Some(dir) = &dir {
        if let Err(e) = deskemy_core::backup::apply_pending_import(dir) {
            tracing::error!(error = %e, "apply staged data import");
        }
    }
    let config = dir
        .as_ref()
        .map(|d| d.join(paths::CONFIG_FILE))
        .and_then(|p| deskemy_core::config::AppConfig::load(&p).ok())
        .unwrap_or_default();

    // Created on first run (a fresh install has none). Only with no data
    // directory at all does the library live in memory.
    let db_path = dir.map(|d| d.join(paths::DB_FILE));
    let conn = match db_path.as_ref().map(|p| db::open(p)) {
        Some(Ok(mut conn)) => {
            tracing::info!(db = %db_path.as_ref().unwrap().display(), "database ready");
            repair_titles(&mut conn);
            // As the Tauri app: cheap for a local library, and keeps search in
            // step with the base tables.
            if let Err(e) = db::queries::rebuild_search_index(&conn) {
                tracing::warn!(error = %e, "rebuild search index");
            }
            Some(conn)
        }
        Some(Err(e)) => {
            tracing::error!(error = %e, "could not open library");
            ui.global::<Library>().set_status_text(format!("Could not open the library: {e}").into());
            None
        }
        None => {
            tracing::warn!("no data directory: the library won't be saved");
            None
        }
    };
    let conn = conn.unwrap_or_else(|| db::open_in_memory().expect("in-memory database"));
    (Arc::new(Mutex::new(conn)), settings::shared(config))
}
