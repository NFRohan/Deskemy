//! Adding a course: pick a folder, probe it off the UI thread with live
//! progress, preview what it would create, then import — as the Tauri
//! sidebar's Add Folder. Folders dropped on the window come the same way. A
//! folder of courses, or several folders, opens a list instead (the batch):
//! tick which, then they import one after another. Also hosts the folder
//! watcher (auto-rescan), which shares the importer.

use crate::dialogs::{self, Picked};
use crate::library::format_duration;
use crate::session::{Db, Session};
use crate::settings::{self, Config};
use crate::{AppWindow, BatchCourse, Import};
use deskemy_core::domain::ImportPreview;
use deskemy_core::importer::folders::{courses_in, Found};
use deskemy_core::importer::{ImportPlan, ImportSnapshot, Importer};
use deskemy_core::media::mpv_prober::MpvProber;
use deskemy_core::media::stub::StubProber;
use deskemy_core::media::MediaProber;
use deskemy_core::watcher::{LibraryWatcher, RescanHost};
use deskemy_core::db::Connection;
use slint::{ComponentHandle, Model};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
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

/// The course folders in what was picked or dropped.
#[derive(Debug, PartialEq)]
pub struct Courses {
    pub folders: Vec<PathBuf>,
    /// The one folder they're all in, when that's what was given: it can be
    /// imported as one course instead.
    pub parent: Option<PathBuf>,
}

/// Find the courses in what was picked or dropped: a course folder is one, a
/// folder of courses its courses.
pub fn find_courses(given: &[PathBuf]) -> Result<Courses, String> {
    let mut folders: Vec<PathBuf> = Vec::new();
    let mut parent = None;
    for folder in given {
        match courses_in(folder) {
            Found::Course => folders.push(folder.clone()),
            Found::Courses(found) => {
                if given.len() == 1 {
                    parent = Some(folder.clone());
                }
                folders.extend(found);
            }
            Found::Nothing => tracing::info!(folder = %folder.display(), "no videos to import"),
        }
    }
    // The same folder twice (dropped along with the folder it's in).
    let mut seen = std::collections::HashSet::new();
    folders.retain(|f| seen.insert(f.clone()));
    match (folders.is_empty(), given) {
        (true, [one]) => Err(format!("No playable video files found in {}.", one.display())),
        (true, _) => Err("No playable video files found in those folders.".into()),
        (false, _) => Ok(Courses { folders, parent }),
    }
}

/// The sidebar button's label during a batch.
pub fn batch_label(n: usize, total: usize) -> String {
    format!("Importing {n}/{total}")
}

fn lectures_label(n: i64) -> String {
    match n {
        1 => "Imported · 1 lecture".into(),
        n => format!("Imported · {n} lectures"),
    }
}

type Staged = Arc<Mutex<Option<(PathBuf, ImportSnapshot, ImportPlan)>>>;
type Watcher = Arc<Mutex<Option<LibraryWatcher>>>;
/// The batch's course folders (row by row) and the folder they came from.
type Batch = Arc<Mutex<Option<Courses>>>;

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
    batch: Batch,
    /// Stop the batch after the course being imported.
    stop: Arc<AtomicBool>,
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
            batch: Arc::new(Mutex::new(None)),
            stop: Arc::new(AtomicBool::new(false)),
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

    /// Nothing's being added: a folder can be picked or dropped.
    pub fn ready(&self, ui: &AppWindow) -> bool {
        let import = ui.global::<Import>();
        !(import.get_scanning()
            || import.get_importing()
            || import.get_open()
            || import.get_batch_open()
            || import.get_batch_running())
    }

    /// Add Folder: pick a folder and add it. During a batch it shows the
    /// batch again instead.
    pub fn add_folder(&self, ui: &AppWindow) {
        let import = ui.global::<Import>();
        if import.get_batch_running() {
            import.set_batch_open(true);
            return;
        }
        if !self.ready(ui) {
            return;
        }
        let folder = match dialogs::pick("add folder", || {
            rfd::FileDialog::new()
                .set_title("Add a course folder")
                .set_parent(&ui.window().window_handle())
                .pick_folder()
        }) {
            Picked::Chosen(folder) => folder,
            Picked::Cancelled => return,
            Picked::Unavailable(why) => {
                import.set_error(why.into());
                return;
            }
        };
        self.add(ui, vec![folder]);
    }

    /// Add picked or dropped folders: one course goes to the preview, several
    /// to the batch.
    pub fn add(&self, ui: &AppWindow, given: Vec<PathBuf>) {
        if !self.ready(ui) {
            tracing::info!(?given, "busy adding; ignored");
            return;
        }
        let import = ui.global::<Import>();
        let given: Vec<PathBuf> = given.into_iter().filter(|p| p.is_dir()).collect();
        if given.is_empty() {
            import.set_error("Only folders can be imported: a course folder, or a folder of courses.".into());
            return;
        }
        import.set_error("".into());
        import.set_scanning(true);
        import.set_progress(progress_label(0, 0).into());
        let (importer, db, staged, batch, weak) =
            (self.importer.clone(), self.db.clone(), self.staged.clone(), self.batch.clone(), ui.as_weak());
        let clean = settings::lock(&self.config).clean_titles;
        std::thread::spawn(move || match find_courses(&given) {
            Ok(courses) if courses.folders.len() == 1 => {
                probe_to_preview(&importer, &db, &staged, weak, courses.folders[0].clone(), clean);
            }
            Ok(courses) => {
                let rows = batch_rows(&db, &courses.folders);
                let source = match &courses.parent {
                    Some(parent) => folder_name(parent),
                    None => format!("{} folders", given.len()),
                };
                let one_folder = courses.parent.is_some();
                *batch.lock().unwrap_or_else(|e| e.into_inner()) = Some(courses);
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let import = ui.global::<Import>();
                    import.set_scanning(false);
                    show_batch(&import, source, one_folder, rows);
                });
            }
            Err(e) => {
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    let import = ui.global::<Import>();
                    import.set_scanning(false);
                    import.set_error(e.into());
                });
            }
        });
    }

    /// Probe one folder as a course; the preview opens when that's done.
    fn preview(&self, ui: &AppWindow, folder: PathBuf) {
        let import = ui.global::<Import>();
        import.set_error("".into());
        import.set_scanning(true);
        import.set_progress(progress_label(0, 0).into());
        let (importer, db, staged, weak) = (self.importer.clone(), self.db.clone(), self.staged.clone(), ui.as_weak());
        let clean = settings::lock(&self.config).clean_titles;
        std::thread::spawn(move || probe_to_preview(&importer, &db, &staged, weak, folder, clean));
    }

    /// Tick or untick a course in the batch.
    pub fn batch_toggle(&self, ui: &AppWindow, row: usize) {
        let import = ui.global::<Import>();
        if import.get_batch_running() || import.get_batch_done() {
            return;
        }
        let rows = import.get_batch();
        if let Some(mut course) = rows.row_data(row) {
            course.selected = !course.selected;
            rows.set_row_data(row, course);
        }
        import.set_batch_selected(rows.iter().filter(|c| c.selected).count() as i32);
    }

    /// The batch's folder as one course: its preview instead.
    pub fn batch_as_one(&self, ui: &AppWindow) {
        let parent = self.batch.lock().unwrap_or_else(|e| e.into_inner()).as_ref().and_then(|c| c.parent.clone());
        let Some(parent) = parent else { return };
        self.batch_close(ui);
        self.preview(ui, parent);
    }

    /// Import the ticked courses, one after another.
    pub fn batch_confirm(&self, ui: &AppWindow) {
        let import = ui.global::<Import>();
        if import.get_batch_running() || import.get_batch_done() {
            return;
        }
        let rows = import.get_batch();
        let picks: Vec<(usize, PathBuf)> = {
            let batch = self.batch.lock().unwrap_or_else(|e| e.into_inner());
            let Some(courses) = batch.as_ref() else { return };
            rows.iter()
                .zip(&courses.folders)
                .enumerate()
                .filter(|(_, (row, _))| row.selected)
                .map(|(i, (_, folder))| (i, folder.clone()))
                .collect()
        };
        if picks.is_empty() {
            return;
        }
        let total = picks.len();
        import.set_error("".into());
        import.set_batch_running(true);
        import.set_progress(batch_label(1, total).into());
        self.stop.store(false, Ordering::Relaxed);
        let (importer, db, watcher, stop, weak) =
            (self.importer.clone(), self.db.clone(), self.watcher.clone(), self.stop.clone(), ui.as_weak());
        let clean = settings::lock(&self.config).clean_titles;
        std::thread::spawn(move || {
            let mut failed = 0;
            for (n, (row, folder)) in picks.iter().enumerate() {
                if stop.load(Ordering::Relaxed) {
                    for (row, _) in &picks[n..] {
                        set_row(&weak, *row, 0, "Not imported: stopped".into());
                    }
                    break;
                }
                set_row(&weak, *row, 1, progress_label(0, 0));
                let label = batch_label(n + 1, total);
                let w = weak.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = w.upgrade() {
                        ui.global::<Import>().set_progress(label.into());
                    }
                });
                let progress = |done: usize, all: usize| set_row(&weak, *row, 1, progress_label(done, all));
                let imported = probe(&importer, &db, folder, clean, progress).and_then(|(preview, snap, plan)| {
                    // Bound first: the db guard ends with this statement.
                    let persisted = importer.persist(&mut db.lock().unwrap_or_else(|e| e.into_inner()), None, &snap, &plan);
                    persisted.map(|id| (id, preview.lectures)).map_err(|e| e.to_string())
                });
                match imported {
                    Ok((id, lectures)) => {
                        if let Some(w) = watcher.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                            w.watch(folder);
                        }
                        index_subtitles(&db, &id);
                        tracing::info!(course = %id, folder = %folder.display(), "imported (batch)");
                        set_row(&weak, *row, 2, lectures_label(lectures));
                        let w = weak.clone();
                        let _ = slint::invoke_from_event_loop(move || {
                            if let Some(ui) = w.upgrade() {
                                ui.invoke_refresh_library();
                            }
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        tracing::warn!(folder = %folder.display(), error = %e, "batch import");
                        set_row(&weak, *row, 3, e);
                    }
                }
            }
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let import = ui.global::<Import>();
                import.set_batch_running(false);
                import.set_batch_stopping(false);
                import.set_batch_done(true);
                import.set_progress("".into());
                // Hidden meanwhile: show it again if any failed; if all went
                // well there's nothing to say.
                if failed > 0 {
                    import.set_batch_open(true);
                } else if !import.get_batch_open() {
                    clear_batch(&import);
                }
            });
        });
    }

    /// Stop the batch after the course being imported.
    pub fn batch_stop(&self, ui: &AppWindow) {
        self.stop.store(true, Ordering::Relaxed);
        ui.global::<Import>().set_batch_stopping(true);
    }

    /// Close the batch; while it runs, only hide it.
    pub fn batch_close(&self, ui: &AppWindow) {
        let import = ui.global::<Import>();
        if import.get_batch_running() {
            import.set_batch_open(false);
            return;
        }
        *self.batch.lock().unwrap_or_else(|e| e.into_inner()) = None;
        clear_batch(&import);
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
            if let Ok(id) = &result {
                if let Some(w) = watcher.lock().unwrap_or_else(|e| e.into_inner()).as_mut() {
                    w.watch(&folder);
                }
                index_subtitles(&db, id);
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

/// Probe a folder as one course (on a worker thread); the preview opens when
/// that's done, or the error shows under Add Folder.
fn probe_to_preview(
    importer: &Importer,
    db: &Db,
    staged: &Staged,
    weak: slint::Weak<AppWindow>,
    folder: PathBuf,
    clean_titles: bool,
) {
    let progress = |done: usize, total: usize| {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                ui.global::<Import>().set_progress(progress_label(done, total).into());
            }
        });
    };
    let probed = probe(importer, db, &folder, clean_titles, progress).map(|(preview, snap, plan)| {
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
}

fn folder_name(folder: &Path) -> String {
    folder.file_name().map_or_else(|| folder.display().to_string(), |n| n.to_string_lossy().into_owned())
}

/// The batch's rows: new courses ticked, ones already in the library not.
pub fn batch_rows(db: &Db, folders: &[PathBuf]) -> Vec<BatchCourse> {
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    folders
        .iter()
        .map(|folder| {
            let in_library = deskemy_core::db::queries::find_course_by_path(&conn, &folder.to_string_lossy())
                .ok()
                .flatten()
                .is_some();
            BatchCourse {
                name: folder_name(folder).into(),
                in_library,
                selected: !in_library,
                state: 0,
                status: "".into(),
            }
        })
        .collect()
}

/// Fill and open the batch.
pub fn show_batch(import: &Import<'_>, source: String, one_folder: bool, rows: Vec<BatchCourse>) {
    import.set_batch_selected(rows.iter().filter(|c| c.selected).count() as i32);
    import.set_batch(crate::course_panel::model(rows));
    import.set_batch_source(source.into());
    import.set_batch_one_folder(one_folder);
    import.set_batch_running(false);
    import.set_batch_stopping(false);
    import.set_batch_done(false);
    import.set_batch_open(true);
}

fn clear_batch(import: &Import<'_>) {
    import.set_batch_open(false);
    import.set_batch_done(false);
    import.set_batch(crate::course_panel::model(Vec::new()));
}

/// A batch row's state (see `BatchCourse`), from a worker thread.
fn set_row(weak: &slint::Weak<AppWindow>, row: usize, state: i32, status: String) {
    let weak = weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = weak.upgrade() else { return };
        let rows = ui.global::<Import>().get_batch();
        if let Some(mut course) = rows.row_data(row) {
            course.state = state;
            course.status = status.into();
            rows.set_row_data(row, course);
        }
    });
}


/// Make a just-imported course's subtitles searchable (they were only indexed
/// from Settings before).
pub fn index_subtitles(db: &Db, course_id: &str) {
    match deskemy_core::maintenance::index_course_subtitles(db, course_id) {
        Ok(cues) => tracing::debug!(course = course_id, cues, "indexed subtitles"),
        Err(e) => tracing::warn!(course = course_id, error = %e, "index subtitles"),
    }
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
    fn dropped_folders_are_sorted_into_courses() {
        let tmp = tempfile::tempdir().unwrap();
        let touch = |rel: &str| {
            let path = tmp.path().join(rel);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"x").unwrap();
        };
        touch("Courses/Rust/01 Intro/001 Hello.mp4");
        touch("Courses/Go/01 Intro/001 Hello.mp4");
        touch("Single/01 Intro/001 Hello.mp4");
        touch("Empty/notes.pdf");
        let dir = |rel: &str| tmp.path().join(rel);

        // One course folder.
        let found = find_courses(&[dir("Single")]).unwrap();
        assert_eq!(found, Courses { folders: vec![dir("Single")], parent: None });
        // A folder of courses: its courses, and it, to import as one instead.
        let found = find_courses(&[dir("Courses")]).unwrap();
        assert_eq!(found.folders, [dir("Courses/Go"), dir("Courses/Rust")]);
        assert_eq!(found.parent, Some(dir("Courses")));
        // Several: each one's courses, once each, and no one parent.
        let found = find_courses(&[dir("Single"), dir("Courses"), dir("Courses/Go"), dir("Empty")]).unwrap();
        assert_eq!(found.folders, [dir("Single"), dir("Courses/Go"), dir("Courses/Rust")]);
        assert_eq!(found.parent, None);
        // Nothing to import.
        assert!(find_courses(&[dir("Empty")]).unwrap_err().starts_with("No playable video files found in"));
        assert_eq!(
            find_courses(&[dir("Empty"), dir("Empty")]).unwrap_err(),
            "No playable video files found in those folders."
        );
        assert_eq!(batch_label(2, 5), "Importing 2/5");
        assert_eq!(lectures_label(1), "Imported · 1 lecture");
    }

    #[test]
    fn a_folder_with_no_videos_is_refused_before_the_preview() {
        assert!(check(&preview(3), Path::new("C:/courses/rust")).is_ok());
        let err = check(&preview(0), Path::new("C:/courses/empty")).unwrap_err();
        assert!(err.starts_with("No playable video files found in"), "{err}");
    }
}
