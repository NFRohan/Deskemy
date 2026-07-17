mod backup;
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

/// Whether the compositing player path is active this run (see `compositor::decide`
/// — default-on on Windows, GPU/DComp probe-gated, with a native `wid` fallback and
/// a `DESKEMY_COMPOSITOR` override). The UI makes the video pane transparent so the
/// composited video shows through, and floats overlays over it.
#[tauri::command]
fn compositor_enabled() -> bool {
    #[cfg(windows)]
    {
        compositor::decide()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// Enter/exit the player's fullscreen, with a native-feeling transition from a
/// maximized window: exactly one visible snap in each direction (like a
/// browser's F11). The maximized state is staged around tao's own fullscreen
/// with metadata-only edits — see the `imm` module for the mechanism.
///
/// Deliberately `async`: it runs off the main thread, so the staging closures
/// and tao's fullscreen message queue behind one another (FIFO) on the event
/// loop instead of interleaving, and there are no IPC-length gaps between the
/// visual states like the previous JS-driven sequence had.
#[tauri::command]
async fn window_set_immersive(
    window: tauri::WebviewWindow,
    on: bool,
) -> std::result::Result<(), String> {
    #[cfg(windows)]
    {
        let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as isize;
        if on {
            if window.is_maximized().unwrap_or(false) {
                window
                    .run_on_main_thread(move || unsafe { imm::stage_enter_from_maximized(hwnd) })
                    .map_err(|e| e.to_string())?;
                window.set_fullscreen(true).map_err(|e| e.to_string())?;
                // Re-enable DWM transition animations once the (queued)
                // fullscreen transition has been applied.
                window
                    .run_on_main_thread(move || unsafe {
                        imm::transitions_suppressed(hwnd, false)
                    })
                    .map_err(|e| e.to_string())
            } else {
                imm::clear_saved();
                window.set_fullscreen(true).map_err(|e| e.to_string())
            }
        } else {
            // Suppress animations across tao's exit-restore and the re-maximize
            // (stage_exit_to_maximized re-enables them when done).
            window
                .run_on_main_thread(move || unsafe { imm::transitions_suppressed(hwnd, true) })
                .map_err(|e| e.to_string())?;
            window.set_fullscreen(false).map_err(|e| e.to_string())?;
            window
                .run_on_main_thread(move || unsafe { imm::stage_exit_to_maximized(hwnd) })
                .map_err(|e| e.to_string())
        }
    }
    #[cfg(not(windows))]
    {
        window.set_fullscreen(on).map_err(|e| e.to_string())
    }
}

/// Maximized-state staging for `window_set_immersive`.
///
/// Two hard-won constraints shape this (see tao 0.35 sources):
/// 1. tao's fullscreen enter is `save placement → SetWindowPos(monitor rect)`,
///    and Windows clamps that resize to the work area iff the window is zoomed
///    — so the maximized state must be left *before* entering fullscreen.
/// 2. tao syncs its internal MAXIMIZED flag from WM_SIZE, and re-asserts both
///    the WS_MAXIMIZE style and ShowWindow(SW_MAXIMIZE) from that flag on any
///    style recompute (set_fullscreen does one). So the state change MUST be a
///    real transition that fires WM_SIZE — silently editing the style bit
///    desyncs tao and it re-maximizes mid-set_fullscreen.
///
/// The jank is then removed by making the real transitions invisible:
/// geometrically, by pre-widening the placement's restore rect to the monitor
/// so leaving the maximized state never shrinks the window; and temporally, by
/// suppressing DWM's state-transition animations (the restore/maximize zoom
/// effects) for the duration of the swap via DWMWA_TRANSITIONS_FORCEDISABLED.
#[cfg(windows)]
mod imm {
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::sync::Mutex;
    use windows::Win32::Foundation::{HWND, RECT};
    use windows::Win32::Graphics::Dwm::{DwmSetWindowAttribute, DWMWA_TRANSITIONS_FORCEDISABLED};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowPlacement, SetWindowPlacement, SW_RESTORE, SW_SHOWMAXIMIZED, WINDOWPLACEMENT,
    };

    /// The maximized window's original restore rect, stashed on enter so exit
    /// can put it back. None = fullscreen wasn't entered from a maximized state.
    static SAVED: Mutex<Option<RECT>> = Mutex::new(None);

    pub fn clear_saved() {
        *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Suppress (or restore) DWM's state-transition animations for this window.
    pub unsafe fn transitions_suppressed(hwnd: isize, suppressed: bool) {
        let hwnd = HWND(hwnd as *mut _);
        let v: i32 = suppressed as i32; // Win32 BOOL
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_TRANSITIONS_FORCEDISABLED,
            &v as *const i32 as *const c_void,
            size_of::<i32>() as u32,
        );
    }

    /// Before tao enters fullscreen from a maximized window: stash the real
    /// restore rect, widen it to the monitor bounds, and leave the maximized
    /// state through a real (WM_SIZE-firing, tao-visible) restore — which now
    /// lands at full-monitor size, so nothing on screen shrinks. Animations are
    /// suppressed until after the fullscreen transition completes.
    pub unsafe fn stage_enter_from_maximized(hwnd_raw: isize) {
        transitions_suppressed(hwnd_raw, true);
        let hwnd = HWND(hwnd_raw as *mut _);
        let mut wp = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if GetWindowPlacement(hwnd, &mut wp).is_err() {
            return;
        }
        *SAVED.lock().unwrap_or_else(|e| e.into_inner()) = Some(wp.rcNormalPosition);

        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(mon, &mut mi).ok().is_err() {
            return;
        }
        wp.rcNormalPosition = mi.rcMonitor;
        wp.showCmd = SW_RESTORE.0 as u32;
        let _ = SetWindowPlacement(hwnd, &wp);
    }

    /// After tao exits fullscreen (its saved placement restores the monitor-
    /// sized normal window — a geometric no-op): re-maximize with the animation
    /// suppressed (a single instant snap to the work area), then put the
    /// original restore rect back as pure metadata so a later manual
    /// un-maximize returns the window to its real pre-fullscreen size.
    pub unsafe fn stage_exit_to_maximized(hwnd_raw: isize) {
        let taken = SAVED.lock().unwrap_or_else(|e| e.into_inner()).take();
        let Some(saved) = taken else {
            transitions_suppressed(hwnd_raw, false);
            return;
        };
        let hwnd = HWND(hwnd_raw as *mut _);
        let mut wp = WINDOWPLACEMENT {
            length: size_of::<WINDOWPLACEMENT>() as u32,
            ..Default::default()
        };
        if GetWindowPlacement(hwnd, &mut wp).is_err() {
            transitions_suppressed(hwnd_raw, false);
            return;
        }
        wp.showCmd = SW_SHOWMAXIMIZED.0 as u32;
        if SetWindowPlacement(hwnd, &wp).is_ok() {
            // Metadata-only while maximized: nothing repaints.
            wp.rcNormalPosition = saved;
            let _ = SetWindowPlacement(hwnd, &wp);
        }
        transitions_suppressed(hwnd_raw, false);
    }
}

/// Portable mode: a `.portable` marker file next to the executable redirects all
/// app data into a sibling `data/` folder, so a portable copy writes nothing
/// outside its own directory. Returns None for a normal install (→ %APPDATA%).
/// Whether this is a portable copy (data next to the exe). The updater can only
/// self-install the installer build, so the UI points portable users at the
/// release page instead of downloading in place.
#[tauri::command]
fn is_portable() -> bool {
    portable_data_dir().is_some()
}

fn portable_data_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    dir.join(".portable").exists().then(|| dir.join("data"))
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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // Portable copies keep their data in a sibling `data/` folder; a
            // normal install uses %APPDATA%.
            let data_dir = match portable_data_dir() {
                Some(d) => {
                    tracing::info!(dir = %d.display(), "portable mode");
                    d
                }
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(&data_dir)?;

            // Apply a data import staged from Settings → Data (the db can't be
            // swapped while it's open, so import stages files and we swap here,
            // before opening, on the next launch).
            if let Err(e) = backup::apply_pending_import(&data_dir) {
                tracing::error!(error = %e, "failed to apply staged data import");
            }

            // Thumbnails are handed to the WebView via the asset protocol, whose
            // static config scope only covers %APPDATA%. In portable mode the
            // data dir sits next to the exe, outside that scope, so every
            // thumbnail request 403s and renders broken. Grant the chosen data
            // dir explicitly — a no-op for installs (already under %APPDATA%).
            app.asset_protocol_scope().allow_directory(&data_dir, true)?;

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

            // Decide the player compositing path once, up front, on the main
            // thread (DirectComposition is UI-thread bound). Both the UI and the
            // lazily-created player read this cached decision so their layout and
            // rendering always agree; the probe falls back to the wid player on
            // machines whose GPU/DComp can't host the composited video.
            #[cfg(windows)]
            tracing::info!(active = compositor::decide(), "player compositing path");

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
            is_portable,
            window_set_immersive,
            commands::library_add_root,
            commands::library_list_roots,
            commands::library_remove_root,
            commands::library_import_course,
            commands::library_preview_import,
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
            commands::history_list,
            commands::track_list,
            commands::track_get,
            commands::track_create,
            commands::track_update,
            commands::track_delete,
            commands::track_add_course,
            commands::track_remove_course,
            commands::track_reorder_courses,
            commands::search_query,
            commands::search_reindex,
            commands::subtitle_search,
            commands::subtitles_reindex,
            commands::stats_get,
            commands::library_reconcile,
            commands::thumbnails_gc,
            commands::storage_stats,
            commands::db_compact,
            commands::subtitle_index_clear,
            commands::config_get,
            commands::config_set,
            commands::data_export,
            commands::data_import,
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
