mod commands;
#[cfg(windows)]
mod compositor;
mod state;

// Exposed for integration tests (tests/ is a separate crate).
pub mod config;
pub mod db;
pub mod domain;
pub mod error;
pub mod hashing;
pub mod importer;
pub mod media;
pub mod mpv;
pub mod player;
pub mod scanner;
pub mod subtitles;
pub mod thumbnails;
pub mod watcher;

use config::AppConfig;
use state::AppState;
use tauri::Manager;
use tracing_subscriber::EnvFilter;

/// Lightweight backend health check used by the frontend to confirm the
/// Rust <-> WebView bridge is wired up.
#[tauri::command]
fn app_health() -> String {
    "ok".to_string()
}

/// Phase-1 compositing spike: paint a DirectComposition test layer behind the
/// (transparent) webview. See docs/player-compositing.md.
#[tauri::command]
fn compositor_test(app: tauri::AppHandle) -> std::result::Result<(), String> {
    #[cfg(windows)]
    {
        compositor::feasibility_test(&app).map_err(|e| e.to_string())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Ok(())
    }
}

/// Whether the compositing player path is active (Windows + DESKEMY_COMPOSITOR).
/// The UI makes the video pane transparent so the composited video shows through.
#[tauri::command]
fn compositor_enabled() -> bool {
    cfg!(windows) && std::env::var_os("DESKEMY_COMPOSITOR").is_some()
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("deskemy_lib=debug,info")),
        )
        .init();

    tracing::info!("Deskemy starting");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;

            let db_path = data_dir.join("deskemy.db");
            let config_path = data_dir.join("config.json");

            let conn = db::open(&db_path)?;
            let config = AppConfig::load(&config_path)?;

            // Rebuild the search index on startup — cheap for a local library
            // and keeps it consistent with the base tables (and any entities
            // indexed after they were first imported).
            db::queries::rebuild_search_index(&conn)?;

            tracing::info!(db = %db_path.display(), "database ready");
            app.manage(AppState::new(conn, config, data_dir, config_path));

            // Filesystem watcher: auto-rescan course folders on change.
            match watcher::LibraryWatcher::start(app.handle().clone()) {
                Ok(mut w) => {
                    if let Ok(conn) = app.state::<AppState>().db.lock() {
                        w.sync(&conn);
                    }
                    app.manage(std::sync::Mutex::new(w));
                }
                Err(e) => tracing::warn!(error = %e, "library watcher failed to start"),
            }
            Ok(())
        })
        // Native window-resize hook: keep the compositor video glued to the pane
        // during fullscreen / edge-drag resizes without the JS round-trip lag.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::Resized(size) = event {
                if let Some(state) = window.try_state::<AppState>() {
                    if let Ok(guard) = state.player.lock() {
                        if let Some(p) = guard.as_ref() {
                            p.on_window_resize(size.width as i32, size.height as i32);
                        }
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            app_health,
            compositor_test,
            compositor_enabled,
            commands::library_add_root,
            commands::library_list_roots,
            commands::library_remove_root,
            commands::library_import_course,
            commands::library_scan_root,
            commands::library_list_courses,
            commands::course_get,
            commands::course_set_favorite,
            commands::course_touch_opened,
            commands::library_delete_course,
            commands::course_attachments,
            commands::open_resource,
            commands::course_tags,
            commands::course_add_tag,
            commands::course_remove_tag,
            commands::course_set_thumbnail_file,
            commands::course_set_thumbnail_bytes,
            commands::course_clear_thumbnail,
            commands::lecture_set_completed,
            commands::bookmark_add,
            commands::bookmark_list,
            commands::bookmark_delete,
            commands::bookmark_list_all,
            commands::search_query,
            commands::search_reindex,
            commands::subtitle_search,
            commands::subtitles_reindex,
            commands::stats_get,
            commands::library_reconcile,
            commands::thumbnails_gc,
            commands::config_get,
            commands::config_set,
            commands::player::player_available,
            commands::player::player_open,
            commands::player::player_toggle_pause,
            commands::player::player_set_paused,
            commands::player::player_seek,
            commands::player::player_set_speed,
            commands::player::player_next,
            commands::player::player_prev,
            commands::player::player_tracks,
            commands::player::player_set_subtitle,
            commands::player::player_set_audio,
            commands::player::player_set_chapter,
            commands::player::player_set_volume,
            commands::player::player_set_muted,
            commands::player::player_set_rect,
            commands::player::player_stop,
            commands::player::player_grab_resume_frame,
            commands::player::player_state,
            commands::player::lecture_get,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
