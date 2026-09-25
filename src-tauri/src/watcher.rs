//! Filesystem watcher for auto-rescan — the core's `LibraryWatcher`, hosted
//! by the Tauri app: rescans read its state, and a re-import emits
//! `library:changed` so the UI refreshes.

use crate::importer::Importer;
use crate::player::PlayerService;
use crate::state::AppState;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager};

pub use deskemy_core::watcher::LibraryWatcher;

struct Host(AppHandle);

impl deskemy_core::watcher::RescanHost for Host {
    fn db(&self) -> &Mutex<Connection> {
        &self.0.state::<AppState>().inner().db
    }
    fn importer(&self) -> &Importer {
        &self.0.state::<AppState>().inner().importer
    }
    fn enabled(&self) -> bool {
        self.0.state::<AppState>().config.lock().map(|c| c.auto_rescan).unwrap_or(false)
    }
    fn clean_titles(&self) -> bool {
        self.0.state::<AppState>().config.lock().map(|c| c.clean_titles).unwrap_or(true)
    }
    // Locks the player before the db (the app-wide player→db order): the core
    // asks for this before it locks the db.
    fn active_lecture(&self) -> Option<String> {
        self.0
            .state::<AppState>()
            .player
            .lock()
            .ok()
            .and_then(|g| g.as_ref().and_then(|p| p.state().lecture_id))
    }
    fn changed(&self) {
        let _ = self.0.emit("library:changed", ());
    }
}

/// Start the watcher for this app.
pub fn start(app: AppHandle) -> Result<LibraryWatcher, notify_debouncer_mini::notify::Error> {
    LibraryWatcher::start(Arc::new(Host(app)))
}
