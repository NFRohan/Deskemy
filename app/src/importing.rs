//! Adding a course: pick a folder, probe it off the UI thread with live
//! progress, preview what it would create, then import — as the Tauri
//! sidebar's Add Folder. Also hosts the folder watcher (auto-rescan), which
//! shares the importer.

use crate::library::format_duration;
use crate::session::{Db, Session};
use crate::settings::{self, Config};
use crate::{AppWindow, Import};
use deskemy_core::domain::ImportPreview;
use deskemy_core::importer::{ImportPlan, ImportSnapshot, Importer};
use deskemy_core::media::mpv_prober::MpvProber;
use deskemy_core::media::stub::StubProber;
use deskemy_core::media::MediaProber;
use deskemy_core::watcher::{LibraryWatcher, RescanHost};
use deskemy_core::db::Connection;
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// The button's label while a folder is being probed.
pub fn progress_label(done: usize, total: usize) -> String {
    if total == 0 {
        "Scanning…".into()
    } else {
        format!("Probing {done}/{total}")
    }
}

/// The warning for videos mpv couldn't open ("" when there are none).
pub fn unplayable_note(n: i64) -> String {
    match n {
        0 => String::new(),
        1 => "1 video couldn't be opened — imported but flagged.".into(),
        n => format!("{n} videos couldn't be opened — imported but flagged."),
    }
}

/// A preview worth confirming, or why not: a folder with nothing playable
/// would only fail on import.
pub fn check(preview: &ImportPreview, folder: &Path) -> Result<(), String> {
    if preview.lectures == 0 {
        return Err(format!("No playable video files found in {}.", folder.display()));
    }
    Ok(())
}

type Staged = Arc<Mutex<Option<(PathBuf, ImportSnapshot, ImportPlan)>>>;
type Watcher = Arc<Mutex<Option<LibraryWatcher>>>;

/// What an auto-rescan reads from the app.
struct Rescans {
    db: Db,
    importer: Arc<Importer>,
    config: Config,
    session: Arc<Session>,
    ui: Mutex<slint::Weak<AppWindow>>,
}

impl RescanHost for Rescans {
    fn db(&self) -> &Mutex<Connection> {
        &self.db
    }
    fn importer(&self) -> &Importer {
        &self.importer
    }
    fn enabled(&self) -> bool {
        settings::lock(&self.config).auto_rescan
    }
    fn clean_titles(&self) -> bool {
        settings::lock(&self.config).clean_titles
    }
    // The session's lock comes before the db's, as everywhere in the app.
    fn active_lecture(&self) -> Option<String> {
        self.session.lecture_id()
    }
    fn changed(&self) {
        let weak = self.ui.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.invoke_refresh_library();
            }
        });
    }
}

/// Probe a folder for import: the snapshot under a brief lock, then the slow
/// probe without it. `progress` gets (done, total) per video.
pub fn probe(
    importer: &Importer,
    db: &Db,
    folder: &Path,
    clean_titles: bool,
    progress: impl Fn(usize, usize),
) -> Result<(ImportPreview, ImportSnapshot, ImportPlan), String> {
    let snap = importer
        .read_snapshot(&db.lock().unwrap_or_else(|e| e.into_inner()), folder)
        .map_err(|e| e.to_string())?;
    let plan = importer.build(folder, &snap, clean_titles, progress).map_err(|e| e.to_string())?;
    let preview = plan.preview(&snap);
    check(&preview, folder)?;
    Ok((preview, snap, plan))
}

pub struct Importing {
    importer: Arc<Importer>,
    db: Db,
    config: Config,
    /// The previewed folder's probed plan, so confirming doesn't re-probe.
    staged: Staged,
    watcher: Watcher,
}

impl Importing {
    pub fn importer(&self) -> &Importer {
        &self.importer
    }

    pub fn new(db: Db, config: Config) -> Self {
        // Prefer the real libmpv prober; without the DLL, import still works
        // but lacks durations and playability checks.
        let prober: Box<dyn MediaProber> = if MpvProber::available() {
            Box::new(MpvProber)
        } else {
            tracing::warn!("libmpv unavailable — importing with the stub prober (no durations)");
            Box::new(StubProber)
        };
        Importing {
            importer: Arc::new(Importer::new(prober)),
            db,
            config,
            staged: Arc::new(Mutex::new(None)),
            watcher: Arc::new(Mutex::new(None)),
        }
    }

    /// Start auto-rescan: watch every course folder (and library root) the
    /// library knows. Changes are acted on only while the setting is on.
    pub fn start_watching(&self, ui: &AppWindow, session: Arc<Session>) {
        let host = Rescans {
            db: self.db.clone(),
            importer: self.importer.clone(),
            config: self.config.clone(),
            session,
            ui: Mutex::new(ui.as_weak()),
        };
        match LibraryWatcher::start(Arc::new(host)) {
            Ok(watcher) => {
                *self.watcher.lock().unwrap_or_else(|e| e.into_inner()) = Some(watcher);
                self.sync_watcher();
            }
            Err(e) => tracing::warn!(error = %e, "library watcher failed to start"),
        }
    }

    /// Watch any course folder not watched yet (after one is relocated).
    pub fn sync_watcher(&self) {
        let conn = self.db.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(watcher) = self.watcher.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
            watcher.sync(&conn);
        }
    }

    /// Pick a folder and probe it; the preview opens when that's done.
    pub fn add_folder(&self, ui: &AppWindow) {
        let import = ui.global::<Import>();
        if import.get_scanning() || import.get_importing() {
            return;
        }
        let Some(folder) = rfd::FileDialog::new()
            .set_title("Add a course folder")
            .set_parent(&ui.window().window_handle())
            .pick_folder()
        else {
            return;
        };
        import.set_error("".into());
        import.set_scanning(true);
        import.set_progress(progress_label(0, 0).into());

        let (importer, db, staged, weak) = (self.importer.clone(), self.db.clone(), self.staged.clone(), ui.as_weak());
        let clean = settings::lock(&self.config).clean_titles;
        std::thread::spawn(move || {
            let progress = |done: usize, total: usize| {
                let weak = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.global::<Import>().set_progress(progress_label(done, total).into());
                    }
                });
            };
            let probed = probe(&importer, &db, &folder, clean, progress).map(|(preview, snap, plan)| {
                *staged.lock().unwrap_or_else(|e| e.into_inner()) = Some((folder.clone(), snap, plan));
                preview
            });
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let import = ui.global::<Import>();
                import.set_scanning(false);
                match probed {
                    Ok(preview) => show_preview(&import, &preview),
                    Err(e) => import.set_error(e.into()),
                }
            });
        });
    }

    /// Import the previewed folder.
    pub fn confirm(&self, ui: &AppWindow) {
        let import = ui.global::<Import>();
        if import.get_importing() {
            return;
        }
        let Some((folder, snap, plan)) = self.staged.lock().unwrap_or_else(|e| e.into_inner()).take() else { return };
        import.set_error("".into());
        import.set_importing(true);
        let (importer, db, watcher, weak) = (self.importer.clone(), self.db.clone(), self.watcher.clone(), ui.as_weak());
        std::thread::spawn(move || {
            // Phase 3: a brief lock to write it all.
            let result = importer
                .persist(&mut db.lock().unwrap_or_else(|e| e.into_inner()), None, &snap, &plan)
                .map_err(|e| e.to_string());
            if result.is_ok() {
                if let Some(w) = watcher.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    w.watch(&folder);
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let import = ui.global::<Import>();
                import.set_importing(false);
                match result {
                    Ok(id) => {
                        tracing::info!(course = %id, "imported");
                        import.set_open(false);
                        ui.invoke_refresh_library();
                    }
                    Err(e) => import.set_error(e.into()),
                }
            });
        });
    }

    pub fn cancel(&self, ui: &AppWindow) {
        *self.staged.lock().unwrap_or_else(|e| e.into_inner()) = None;
        let import = ui.global::<Import>();
        import.set_open(false);
        import.set_error("".into());
    }
}

/// Fill and open the preview dialog.
pub fn show_preview(import: &Import<'_>, p: &ImportPreview) {
    import.set_title(p.title.clone().into());
    import.set_reimport(p.is_reimport);
    import.set_sections(p.sections as i32);
    import.set_lectures(p.lectures as i32);
    import.set_resources(p.resources as i32);
    import.set_subtitles(p.subtitles as i32);
    import.set_runtime(p.total_duration.map(|d| format!("Total runtime · {}", format_duration(Some(d)))).unwrap_or_default().into());
    import.set_unplayable(unplayable_note(p.unplayable).into());
    import.set_open(true);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preview(lectures: i64) -> ImportPreview {
        ImportPreview {
            title: "Course".into(),
            is_reimport: false,
            sections: 1,
            lectures,
            resources: 0,
            subtitles: 0,
            unplayable: 0,
            total_duration: None,
        }
    }

    #[test]
    fn labels_read_like_the_tauri_sidebar() {
        assert_eq!(progress_label(0, 0), "Scanning…");
        assert_eq!(progress_label(3, 12), "Probing 3/12");
        assert_eq!(unplayable_note(0), "");
        assert_eq!(unplayable_note(1), "1 video couldn't be opened — imported but flagged.");
        assert_eq!(unplayable_note(4), "4 videos couldn't be opened — imported but flagged.");
    }

    #[test]
    fn a_folder_with_no_videos_is_refused_before_the_preview() {
        assert!(check(&preview(3), Path::new("C:/courses/rust")).is_ok());
        let err = check(&preview(0), Path::new("C:/courses/empty")).unwrap_err();
        assert!(err.starts_with("No playable video files found in"), "{err}");
    }
}
