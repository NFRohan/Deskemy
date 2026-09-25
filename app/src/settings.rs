//! Settings — preferences saved to config.json — as the Tauri app's
//! routes/settings.

use crate::course_panel::model;
use crate::session::Db;
use crate::{AppWindow, Playback, Prefs, SelectOption, Theme};
use deskemy_core::config::AppConfig;
use deskemy_core::maintenance::{self, GcReport, ReconcileReport};
use deskemy_core::{backup, courses, db, paths};
use slint::ComponentHandle;
use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

/// The one config, shared by the pages and the player (which reads the
/// default speed and autoplay as lectures open and end).
pub type Config = Arc<Mutex<AppConfig>>;

pub fn shared(config: AppConfig) -> Config {
    Arc::new(Mutex::new(config))
}

pub fn lock(config: &Config) -> MutexGuard<'_, AppConfig> {
    config.lock().unwrap_or_else(|e| e.into_inner())
}

pub const THEMES: [(&str, &str); 3] = [("dark", "Dark"), ("light", "Light"), ("system", "System")];
pub const SPEEDS: [f64; 7] = [0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0];
pub const GOALS: [i64; 6] = [15, 30, 45, 60, 90, 120];

fn speed_label(speed: f64) -> String {
    format!("{speed}×")
}

fn goal_label(minutes: i64) -> String {
    format!("{minutes} min")
}

/// Apply one change from the page to a config. Returns false for an unknown
/// key or a value that doesn't parse, leaving the config as it was.
pub fn apply(config: &mut AppConfig, key: &str, value: &str) -> bool {
    let on = value == "true";
    match key {
        "theme" if THEMES.iter().any(|(v, _)| *v == value) => config.theme = value.into(),
        "speed" => match value.parse::<f64>() {
            Ok(speed) if speed > 0.0 => config.default_speed = speed,
            _ => return false,
        },
        "goal" => match value.parse::<i64>() {
            Ok(minutes) if minutes > 0 => config.daily_goal_minutes = minutes,
            _ => return false,
        },
        "autoplay" => config.autoplay_next = on,
        "autohide" => config.autohide_controls = on,
        "clean-titles" => config.clean_titles = on,
        "auto-rescan" => config.auto_rescan = on,
        "resources-inline" => config.resources_inline = on,
        "pause-at-resources" => config.pause_at_resources = on,
        _ => return false,
    }
    true
}

/// 129015 → "129,015".
pub fn grouped(n: i64) -> String {
    let digits = n.unsigned_abs().to_string();
    let mut out = String::new();
    for (i, d) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(d);
    }
    if n < 0 { format!("-{out}") } else { out }
}

fn plural(n: i64, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// "512 B", "12 KB", "3.4 MB" — as the Tauri page.
pub fn bytes(n: u64) -> String {
    match n {
        n if n < 1024 => format!("{n} B"),
        n if n < 1024 * 1024 => format!("{:.0} KB", n as f64 / 1024.0),
        n => format!("{:.1} MB", n as f64 / 1024.0 / 1024.0),
    }
}

pub fn reconcile_message(r: &ReconcileReport) -> String {
    if r.files_missing == 0 {
        format!("All files present across {}.", plural(r.courses_checked, "course"))
    } else {
        format!(
            "{} across {} — flagged in the library.",
            plural(r.files_missing, "missing file"),
            plural(r.courses_missing, "course")
        )
    }
}

pub fn subtitles_message(cues: i64) -> String {
    if cues == 0 {
        "No sidecar subtitle files found.".into()
    } else {
        format!("Indexed {}.", plural(cues, "subtitle line"))
    }
}

pub fn gc_message(r: &GcReport) -> String {
    if r.removed == 0 {
        "Cache already clean.".into()
    } else {
        format!("Removed {} ({}).", plural(r.removed, "file"), bytes(r.freed_bytes.max(0) as u64))
    }
}

pub fn clear_message(cues: i64) -> String {
    if cues == 0 {
        "Subtitle index already empty.".into()
    } else {
        format!("Cleared {}. Compact the database to reclaim the space.", plural(cues, "cue"))
    }
}

/// A maintenance action by its page key, run off the UI thread; returns the
/// line the page shows under it (the error, if it failed).
pub fn run_task(key: &str, db: &Db, data_dir: Option<&std::path::Path>) -> String {
    let conn = || db.lock().unwrap_or_else(|e| e.into_inner());
    let thumbs = data_dir.map(|d| d.join(courses::THUMBNAILS_DIR));
    let result = match (key, data_dir, thumbs.as_deref()) {
        ("reconcile", ..) => maintenance::reconcile(db).map(|r| reconcile_message(&r)),
        ("reindex", ..) => maintenance::reindex_search(&conn()).map(|n| format!("Reindexed {}.", plural(n, "item"))),
        ("subs", ..) => maintenance::reindex_subtitles(db).map(subtitles_message),
        ("clearsubs", ..) => maintenance::clear_subtitles(&conn()).map(clear_message),
        ("compact", Some(dir), _) => {
            maintenance::compact(&conn(), dir).map(|n| format!("Database compacted — now {}.", bytes(n)))
        }
        ("gc", _, Some(thumbs)) => maintenance::gc_thumbnails(&conn(), thumbs).map(|r| gc_message(&r)),
        _ => return "Not available without a data folder.".into(),
    };
    result.unwrap_or_else(|e| e.to_string())
}

/// Fill the Storage section.
pub fn show_storage(ui: &AppWindow, db: &Db, data_dir: Option<&std::path::Path>) {
    let prefs = ui.global::<Prefs>();
    let stats = data_dir.and_then(|dir| {
        let conn = db.lock().unwrap_or_else(|e| e.into_inner());
        maintenance::storage(&conn, dir, &dir.join(courses::THUMBNAILS_DIR)).ok()
    });
    match stats {
        Some(s) => {
            prefs.set_db_size(bytes(s.db_bytes).into());
            prefs.set_thumbnail_size(bytes(s.thumbnail_bytes).into());
            prefs.set_cues(s.subtitle_cues as i32);
            prefs.set_cues_label(format!("{} cues", grouped(s.subtitle_cues)).into());
        }
        None => {
            prefs.set_db_size("—".into());
            prefs.set_thumbnail_size("—".into());
            prefs.set_cues(-1);
            prefs.set_cues_label("—".into());
        }
    }
}

/// This build's version, as the backup manifest records it.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Write a backup of the library to `dest`.
pub fn export(db: &Db, data_dir: &Path, dest: &Path) -> String {
    let (config, thumbs) = (data_dir.join(paths::CONFIG_FILE), data_dir.join(courses::THUMBNAILS_DIR));
    match maintenance::export_backup(db, data_dir, &config, &thumbs, dest, VERSION) {
        Ok(()) => "Backup saved.".into(),
        Err(e) => e.to_string(),
    }
}

/// The Settings page: preferences (saved to config.json in the data
/// directory; without one, changes last only for this run), maintenance and
/// backups.
pub struct SettingsPage {
    config: Config,
    db: Db,
    data_dir: Option<PathBuf>,
    /// The backup chosen for import, awaiting confirmation.
    import_from: RefCell<Option<PathBuf>>,
    /// An import is staged: relaunch once this run has let go of the data.
    restart: Cell<bool>,
}

impl SettingsPage {
    pub fn new(config: Config, db: Db, data_dir: Option<PathBuf>) -> Self {
        SettingsPage { config, db, data_dir, import_from: RefCell::new(None), restart: Cell::new(false) }
    }

    pub fn restart_requested(&self) -> bool {
        self.restart.get()
    }

    /// Run a page action by key. Maintenance runs in the background; its
    /// result line, fresh storage figures and (when the library changed) the
    /// library follow.
    pub fn run(&self, ui: &AppWindow, key: &str) {
        match key {
            "export" => return self.export(ui),
            "import" => return self.choose_import(ui),
            _ => {}
        }
        let key = key.to_string();
        self.spawn(ui, key.clone(), move |db, dir| run_task(&key, db, dir));
    }

    fn spawn<F>(&self, ui: &AppWindow, key: String, task: F)
    where
        F: FnOnce(&Db, Option<&Path>) -> String + Send + 'static,
    {
        let prefs = ui.global::<Prefs>();
        if prefs.get_busy() != "" {
            return;
        }
        prefs.set_busy(key.as_str().into());
        let (db, dir, weak) = (self.db.clone(), self.data_dir.clone(), ui.as_weak());
        std::thread::spawn(move || {
            let message = task(&db, dir.as_deref());
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let prefs = ui.global::<Prefs>();
                prefs.set_busy("".into());
                prefs.invoke_set_result(key.as_str().into(), message.into());
                show_storage(&ui, &db, dir.as_deref());
                // Missing flags and titles show in the library.
                if key == "reconcile" || key == "reindex" {
                    ui.invoke_refresh_library();
                }
            });
        });
    }

    pub fn show_storage(&self, ui: &AppWindow) {
        show_storage(ui, &self.db, self.data_dir.as_deref());
    }

    fn export(&self, ui: &AppWindow) {
        if self.data_dir.is_none() {
            ui.global::<Prefs>().invoke_set_result("export".into(), "Not available without a data folder.".into());
            return;
        }
        let Some(dest) = rfd::FileDialog::new()
            .set_title("Export backup")
            .set_file_name("deskemy-backup.zip")
            .add_filter("Deskemy backup", &["zip"])
            .set_parent(&ui.window().window_handle())
            .save_file()
        else {
            return;
        };
        self.spawn(ui, "export".into(), move |db, dir| match dir {
            Some(dir) => export(db, dir, &dest),
            None => "Not available without a data folder.".into(),
        });
    }

    /// Pick a backup, then ask before replacing the library with it.
    fn choose_import(&self, ui: &AppWindow) {
        let Some(src) = rfd::FileDialog::new()
            .set_title("Import backup")
            .add_filter("Deskemy backup", &["zip"])
            .set_parent(&ui.window().window_handle())
            .pick_file()
        else {
            return;
        };
        *self.import_from.borrow_mut() = Some(src);
        let prefs = ui.global::<Prefs>();
        prefs.set_result_import("".into());
        prefs.set_import_confirm(true);
    }

    /// Stage the chosen backup and close; `main` relaunches, and the next
    /// start swaps it in before opening the library.
    pub fn confirm_import(&self, ui: &AppWindow) {
        let prefs = ui.global::<Prefs>();
        let staged = match (self.data_dir.as_deref(), self.import_from.borrow_mut().take()) {
            (Some(dir), Some(src)) => backup::stage_import(dir, &src, db::SCHEMA_VERSION).map_err(|e| e.to_string()),
            _ => Err("Not available without a data folder.".into()),
        };
        match staged {
            Ok(()) => {
                self.restart.set(true);
                // Hiding the only window ends the event loop.
                let _ = ui.hide();
            }
            Err(e) => {
                prefs.set_import_confirm(false);
                prefs.set_result_import(e.into());
            }
        }
    }

    /// Fill the page, and apply what the rest of the UI shows from config.
    pub fn show(&self, ui: &AppWindow) {
        let c = lock(&self.config).clone();
        ui.global::<Theme>().set_mode(c.theme.as_str().into());
        ui.global::<Playback>().set_autohide_fullscreen(c.autohide_controls);

        let prefs = ui.global::<Prefs>();
        let options = |items: Vec<(String, String)>| {
            model(items.into_iter().map(|(value, label)| SelectOption { value: value.into(), label: label.into() }).collect())
        };
        prefs.set_themes(options(THEMES.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect()));
        prefs.set_speeds(options(SPEEDS.iter().map(|s| (s.to_string(), speed_label(*s))).collect()));
        prefs.set_goals(options(GOALS.iter().map(|m| (m.to_string(), goal_label(*m))).collect()));

        prefs.set_theme(c.theme.clone().into());
        let theme = THEMES.iter().find(|(v, _)| *v == c.theme).map_or("Dark", |(_, l)| *l);
        prefs.set_theme_label(theme.into());
        prefs.set_speed(c.default_speed.to_string().into());
        prefs.set_speed_label(speed_label(c.default_speed).into());
        prefs.set_goal(c.daily_goal_minutes.to_string().into());
        prefs.set_goal_label(goal_label(c.daily_goal_minutes).into());
        prefs.set_autoplay(c.autoplay_next);
        prefs.set_autohide(c.autohide_controls);
        prefs.set_clean_titles(c.clean_titles);
        prefs.set_auto_rescan(c.auto_rescan);
        prefs.set_resources_inline(c.resources_inline);
        prefs.set_pause_at_resources(c.pause_at_resources);
    }

    /// Change a setting, save it, and redraw.
    pub fn change(&self, ui: &AppWindow, key: &str, value: &str) {
        let saved = {
            let mut c = lock(&self.config);
            if !apply(&mut c, key, value) {
                tracing::warn!(key, value, "unknown setting");
                return;
            }
            match &self.data_dir {
                Some(dir) => c.save(&dir.join(paths::CONFIG_FILE)).map_err(|e| e.to_string()),
                None => Ok(()),
            }
        };
        if let Err(e) = saved {
            tracing::warn!(error = %e, "save config");
        }
        self.show(ui);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn applies_known_settings_and_rejects_the_rest() {
        let mut c = AppConfig::default();
        assert!(apply(&mut c, "theme", "system"));
        assert!(apply(&mut c, "speed", "1.25"));
        assert!(apply(&mut c, "goal", "45"));
        assert!(apply(&mut c, "autoplay", "false"));
        assert!(apply(&mut c, "autohide", "true"));
        assert!(apply(&mut c, "resources-inline", "false"));
        assert!(apply(&mut c, "pause-at-resources", "true"));
        assert!(!c.resources_inline && c.pause_at_resources);
        assert_eq!(
            (c.theme.as_str(), c.default_speed, c.daily_goal_minutes, c.autoplay_next, c.autohide_controls),
            ("system", 1.25, 45, false, true)
        );

        let before = format!("{c:?}");
        assert!(!apply(&mut c, "theme", "neon"));
        assert!(!apply(&mut c, "speed", "fast"));
        assert!(!apply(&mut c, "speed", "0"));
        assert!(!apply(&mut c, "goal", "-5"));
        assert!(!apply(&mut c, "volume", "11"));
        assert_eq!(format!("{c:?}"), before, "rejected changes leave it alone");
    }

    #[test]
    fn labels_match_the_tauri_page() {
        assert_eq!(speed_label(1.0), "1×");
        assert_eq!(speed_label(0.75), "0.75×");
        assert_eq!(goal_label(90), "90 min");
    }

    #[test]
    fn maintenance_results_read_like_the_tauri_page() {
        assert_eq!(grouped(129015), "129,015");
        assert_eq!(grouped(1000000), "1,000,000");
        assert_eq!(grouped(999), "999");
        assert_eq!(bytes(512), "512 B");
        assert_eq!(bytes(12 * 1024 + 300), "12 KB");
        assert_eq!(bytes(3_565_158), "3.4 MB");
        let ok = ReconcileReport { courses_checked: 1, courses_missing: 0, files_missing: 0 };
        assert_eq!(reconcile_message(&ok), "All files present across 1 course.");
        let bad = ReconcileReport { courses_checked: 7, courses_missing: 2, files_missing: 1 };
        assert_eq!(reconcile_message(&bad), "1 missing file across 2 courses — flagged in the library.");
        assert_eq!(subtitles_message(0), "No sidecar subtitle files found.");
        assert_eq!(subtitles_message(2), "Indexed 2 subtitle lines.");
        assert_eq!(gc_message(&GcReport { removed: 0, freed_bytes: 0 }), "Cache already clean.");
        assert_eq!(gc_message(&GcReport { removed: 3, freed_bytes: 2048 }), "Removed 3 files (2 KB).");
        assert_eq!(clear_message(1), "Cleared 1 cue. Compact the database to reclaim the space.");
    }

    #[test]
    fn tasks_need_a_data_folder_for_files_on_disk() {
        let db: Db = std::sync::Arc::new(std::sync::Mutex::new(deskemy_core::db::open_in_memory().unwrap()));
        assert_eq!(run_task("reindex", &db, None), "Reindexed 0 items.");
        assert_eq!(run_task("clearsubs", &db, None), "Subtitle index already empty.");
        assert_eq!(run_task("compact", &db, None), "Not available without a data folder.");
        assert_eq!(run_task("gc", &db, None), "Not available without a data folder.");
    }
}
