mod commands;
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
pub mod thumbnails;

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
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_health,
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
