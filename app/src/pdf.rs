//! The PDF viewer: a course's PDFs read inside Deskemy (pages, scroll, zoom),
//! with "Open in your PDF app" for anything more.
//!
//! PDFium renders them, on a thread of its own that lives as long as the app
//! (PDFium can be loaded once per process). It holds one document at a time,
//! renders only the pages in view or next to it, at the width they're shown,
//! and the UI lets go of pages far out of view. Without PDFium, PDFs open in
//! the system's PDF app as before.

use crate::{AppWindow, Course, Pdf, PdfLink, PdfPage, Playback, ResourceItem};
use slint::{ComponentHandle, Model, Rgba8Pixel, SharedPixelBuffer};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};

/// Room above the first page and between pages (as in pdf.slint).
const PADDING: f32 = 24.0;
const SPACING: f32 = 12.0;
/// Zoom steps, over the fitted width.
const ZOOMS: &[f32] = &[0.5, 0.67, 0.75, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0];
/// Pages rendered beyond those in view, and kept beyond those.
const RENDER_AROUND: usize = 1;
const KEEP_AROUND: usize = 3;

/// Whether a file is one the viewer opens.
pub fn is_pdf(path: &str) -> bool {
    Path::new(path).extension().is_some_and(|e| e.eq_ignore_ascii_case("pdf"))
}

/// Where PDFium is: beside the exe (a test's exe sits a folder below), or in
/// a bundle's lib/ beside its bin/ (the AppImage). Not there, PDFs open in the
/// system's PDF app.
fn library() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?;
    let name = pdfium_render::prelude::Pdfium::pdfium_platform_library_name();
    let dirs = [Some(dir.to_path_buf()), dir.parent().map(Path::to_path_buf), Some(dir.join("../lib"))];
    dirs.into_iter().flatten().map(|d| d.join(&name)).find(|p| p.is_file())
}

/// Each page's top and height, at a page width.
pub fn layout(aspects: &[f32], page_width: f32) -> Vec<(f32, f32)> {
    let mut y = PADDING;
    aspects
        .iter()
        .map(|a| {
            let h = page_width * a;
            let top = y;
            y += h + SPACING;
            (top, h)
        })
        .collect()
}

/// The pages in view (first and last), and the one being read: the page at
/// a third of the way down the view.
pub fn in_view(pages: &[(f32, f32)], top: f32, height: f32) -> Option<(usize, usize, usize)> {
    let visible = |&(y, h): &(f32, f32)| y < top + height && y + h > top;
    let first = pages.iter().position(visible)?;
    let last = pages.iter().rposition(visible)?;
    let reading = top + height / 3.0;
    let current = pages.iter().position(|&(y, h)| reading < y + h + SPACING).unwrap_or(last);
    Some((first, last, current.clamp(first, last)))
}

/// The pixel width to render at: the shown width on this screen, rounded up
/// a little so small resizes don't re-render everything.
pub fn render_width(page_width: f32, scale: f32) -> u32 {
    let px = (page_width * scale).ceil().clamp(64.0, 4096.0) as u32;
    px.div_ceil(32) * 32
}

/// The next zoom step in or out (0: fit).
pub fn next_zoom(zoom: f32, direction: i32) -> f32 {
    match direction {
        0 => 1.0,
        d if d > 0 => ZOOMS.iter().copied().find(|z| *z > zoom + 0.001).unwrap_or(zoom),
        _ => ZOOMS.iter().rev().copied().find(|z| *z < zoom - 0.001).unwrap_or(zoom),
    }
}

/// Where a link on a page goes.
#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    /// A page of the same PDF, from 0.
    Page(usize),
    /// A web or mail address.
    Web(String),
}

/// A link on a page: its area (left, top, width, height, as fractions of the
/// page from its top left) and where it goes.
#[derive(Clone, Debug, PartialEq)]
pub struct Link {
    pub area: [f32; 4],
    pub target: Target,
}

/// A link's address, if it's one to follow: web and mail addresses only,
/// never a file or a program a PDF names.
pub fn web(uri: &str) -> Option<Target> {
    let uri = uri.trim();
    let (scheme, rest) = uri.split_once(':')?;
    let ok = matches!(scheme.to_ascii_lowercase().as_str(), "http" | "https" | "mailto") && !rest.is_empty();
    ok.then(|| Target::Web(uri.to_string()))
}

/// What the worker is asked to do.
#[derive(Default)]
struct Work {
    /// The document wanted now; a job of an older one is dropped.
    generation: u64,
    open: Option<PathBuf>,
    /// (page, pixel width), first first.
    renders: VecDeque<(usize, u32)>,
}

#[derive(Default)]
struct Shared {
    work: Mutex<Work>,
    wake: Condvar,
}

impl Shared {
    fn work(&self) -> std::sync::MutexGuard<'_, Work> {
        self.work.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The open document (UI thread).
#[derive(Default)]
struct State {
    generation: u64,
    path: String,
    aspects: Vec<f32>,
    /// The width each page was asked for (rendered or on its way).
    requested: HashMap<usize, u32>,
    /// Pages holding pixels.
    shown: HashSet<usize>,
    /// The view: top, height, page width.
    view: (f32, f32, f32),
}

/// Where a resource was opened: ticking it off in the viewer goes back there,
/// so that list shows it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Opener {
    Course,
    Player,
}

pub struct Viewer {
    db: crate::session::Db,
    shared: Arc<Shared>,
    state: RefCell<State>,
    started: RefCell<bool>,
    /// The open PDF's resource id, and where it was opened.
    resource: RefCell<Option<(String, Opener)>>,
}

impl Viewer {
    pub fn new(db: crate::session::Db) -> std::rc::Rc<Self> {
        std::rc::Rc::new(Viewer {
            db,
            shared: Arc::default(),
            state: RefCell::default(),
            started: RefCell::new(false),
            resource: RefCell::default(),
        })
    }

    /// Open a resource: a PDF in the viewer (when PDFium is here), anything
    /// else in its default app.
    pub fn open_resource(&self, ui: &AppWindow, item: &ResourceItem, from: Opener) {
        if !is_pdf(&item.path) || library().is_none() {
            crate::open_resource(&item.path);
            return;
        }
        self.open(ui, &item.path);
        *self.resource.borrow_mut() = Some((item.id.to_string(), from));
        let pdf = ui.global::<Pdf>();
        pdf.set_resource(item.id.clone());
        pdf.set_done(item.done);
    }

    /// Tick the open resource off (or not), in the list it was opened from.
    pub fn mark_done(&self, ui: &AppWindow, done: bool) {
        let Some((id, from)) = self.resource.borrow().clone() else { return };
        match from {
            Opener::Course => ui.global::<Course>().invoke_toggle_resource(id.clone().into(), done),
            Opener::Player => ui.global::<Playback>().invoke_toggle_resource(id.clone().into(), done),
        }
        // What was saved, not what was asked: a rescan may have replaced it.
        let saved = deskemy_core::db::queries::attachment_completed(&self.db.lock().unwrap_or_else(|e| e.into_inner()), &id);
        let pdf = ui.global::<Pdf>();
        match saved {
            Ok(Some(done)) => pdf.set_done(done),
            Ok(None) => {
                tracing::warn!(resource = %id, "resource gone (rescanned); its tick is off");
                *self.resource.borrow_mut() = None;
                pdf.set_resource("".into());
            }
            Err(e) => tracing::warn!(error = %e, "resource done"),
        }
    }

    fn open(&self, ui: &AppWindow, path: &str) {
        self.start(ui);
        // From the player: pause, and leave fullscreen or the mini player for
        // the full window. A lecture that has ended stays ended (toggling
        // pause would play it again), with its end card up for the way back.
        if ui.get_playing() {
            let playback = ui.global::<Playback>();
            playback.invoke_pause();
            if playback.get_mini() {
                playback.invoke_toggle_mini();
            } else if playback.get_fullscreen() {
                playback.invoke_toggle_fullscreen();
            }
            if playback.get_open_menu() != "exercise" {
                playback.set_open_menu("".into());
            }
        }
        *self.resource.borrow_mut() = None;
        let generation = {
            let mut state = self.state.borrow_mut();
            let generation = state.generation + 1;
            *state = State { generation, path: path.to_string(), view: state.view, ..State::default() };
            generation
        };
        {
            let mut work = self.shared.work();
            work.generation = generation;
            work.open = Some(PathBuf::from(path));
            work.renders.clear();
        }
        self.shared.wake.notify_one();

        let pdf = ui.global::<Pdf>();
        let name = Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().into_owned());
        pdf.set_name(name.into());
        pdf.set_pages(crate::course_panel::model(Vec::new()));
        pdf.set_current(1);
        pdf.set_zoom(1.0);
        pdf.set_scroll_y(0.0);
        pdf.set_error("".into());
        pdf.set_resource("".into());
        pdf.set_done(false);
        pdf.set_loading(true);
        pdf.set_open(true);
        tracing::info!(%path, "pdf open");
    }

    pub fn close(&self, ui: &AppWindow) {
        let generation = {
            let mut state = self.state.borrow_mut();
            state.generation += 1;
            state.aspects.clear();
            state.requested.clear();
            state.shown.clear();
            state.generation
        };
        // The worker lets go of the document (and its file) too.
        {
            let mut work = self.shared.work();
            work.generation = generation;
            work.open = None;
            work.renders.clear();
        }
        self.shared.wake.notify_one();
        *self.resource.borrow_mut() = None;
        let pdf = ui.global::<Pdf>();
        pdf.set_open(false);
        pdf.set_pages(crate::course_panel::model(Vec::new()));
    }

    pub fn open_outside(&self) {
        let path = self.state.borrow().path.clone();
        if !path.is_empty() {
            crate::open_resource(&path);
        }
    }

    pub fn zoom_by(&self, ui: &AppWindow, direction: i32) {
        let pdf = ui.global::<Pdf>();
        let (old, new) = (pdf.get_zoom(), next_zoom(pdf.get_zoom(), direction));
        if (new - old).abs() < 0.001 {
            return;
        }
        // Keep the same spot in view: scale the scroll position with the pages.
        let top = -pdf.get_scroll_y();
        let at = (top - PADDING).max(0.0) * new / old + PADDING;
        pdf.set_zoom(new);
        pdf.set_scroll_y(-(if top <= PADDING { top } else { at }));
    }

    /// To the previous or next page's top.
    pub fn step(&self, ui: &AppWindow, delta: i32) {
        self.go_to(ui, i64::from(ui.global::<Pdf>().get_current()) - 1 + i64::from(delta));
    }

    /// To a page's top (from 0; past either end, that end).
    fn go_to(&self, ui: &AppWindow, page: i64) {
        let pdf = ui.global::<Pdf>();
        let state = self.state.borrow();
        let (_, height, width) = state.view;
        let pages = layout(&state.aspects, width);
        let Some(last) = pages.last() else { return };
        let target = page.clamp(0, pages.len() as i64 - 1) as usize;
        let bottom = (last.0 + last.1 + PADDING - height).max(0.0);
        pdf.set_scroll_y(-(pages[target].0 - SPACING).clamp(0.0, bottom));
    }

    /// A link was clicked: to a page of this PDF, or a web address, opened in
    /// the browser.
    pub fn follow(&self, ui: &AppWindow, page: i32, url: &str) {
        if page >= 0 {
            self.go_to(ui, i64::from(page));
        } else if let Some(Target::Web(url)) = web(url) {
            tracing::info!(%url, "pdf link");
            if let Err(e) = open::that_detached(&url) {
                tracing::warn!(error = %e, %url, "open pdf link");
            }
        }
    }

    /// The view moved or resized: ask for the pages now in view, and let go
    /// of those far from it.
    pub fn viewport(&self, ui: &AppWindow, top: f32, height: f32, page_width: f32) {
        self.state.borrow_mut().view = (top, height, page_width);
        self.update(ui);
    }

    fn update(&self, ui: &AppWindow) {
        let mut state = self.state.borrow_mut();
        let (top, height, width) = state.view;
        let pages = layout(&state.aspects, width);
        let Some((first, last, current)) = in_view(&pages, top, height) else { return };
        let pdf = ui.global::<Pdf>();
        pdf.set_current(current as i32 + 1);

        let target = render_width(width, ui.window().scale_factor());
        let around = |n: usize| (first.saturating_sub(n), (last + n).min(pages.len() - 1));
        let (render_from, render_to) = around(RENDER_AROUND);
        let (keep_from, keep_to) = around(KEEP_AROUND);
        let keep = |p: usize| (keep_from..=keep_to).contains(&p);
        {
            let mut work = self.shared.work();
            if work.generation != state.generation {
                return;
            }
            // Drop asks no longer wanted: far away, or at an old width.
            let mut dropped = Vec::new();
            work.renders.retain(|&(p, w)| {
                let wanted = keep(p) && w == target;
                if !wanted {
                    dropped.push(p);
                }
                wanted
            });
            for p in dropped {
                state.requested.remove(&p);
            }
            // The page being read first, then outwards.
            let mut wanted: Vec<usize> = (render_from..=render_to).collect();
            wanted.sort_by_key(|p| p.abs_diff(current));
            for p in wanted {
                if state.requested.get(&p) != Some(&target) {
                    work.renders.push_back((p, target));
                    state.requested.insert(p, target);
                }
            }
        }
        self.shared.wake.notify_one();

        // Pixels far out of view go.
        let far: Vec<usize> = state.shown.iter().copied().filter(|p| !keep(*p)).collect();
        let rows = pdf.get_pages();
        for p in far {
            state.shown.remove(&p);
            state.requested.remove(&p);
            if let Some(mut page) = rows.row_data(p) {
                page.has_image = false;
                page.image = slint::Image::default();
                rows.set_row_data(p, page);
            }
        }
    }

    fn opened(&self, ui: &AppWindow, generation: u64, result: Result<Vec<f32>, String>) {
        if self.state.borrow().generation != generation {
            return;
        }
        let pdf = ui.global::<Pdf>();
        pdf.set_loading(false);
        match result {
            Ok(aspects) => {
                let rows = aspects
                    .iter()
                    .map(|&aspect| PdfPage { image: Default::default(), has_image: false, aspect, links: Default::default() })
                    .collect();
                pdf.set_pages(crate::course_panel::model(rows));
                self.state.borrow_mut().aspects = aspects;
                self.update(ui);
            }
            Err(e) => pdf.set_error(e.into()),
        }
    }

    fn rendered(&self, ui: &AppWindow, generation: u64, page: usize, width: u32, (pixels, links): Rendered) {
        let mut state = self.state.borrow_mut();
        if state.generation != generation || state.requested.get(&page) != Some(&width) {
            return;
        }
        let rows = ui.global::<Pdf>().get_pages();
        let Some(mut row) = rows.row_data(page) else { return };
        // The rendered page has the final say on its shape (a rotated page).
        let aspect = pixels.height() as f32 / pixels.width().max(1) as f32;
        if (aspect - row.aspect).abs() > 0.01 {
            row.aspect = aspect;
            if let Some(a) = state.aspects.get_mut(page) {
                *a = aspect;
            }
        }
        row.image = slint::Image::from_rgba8(pixels);
        row.has_image = true;
        row.links = link_rows(&links);
        rows.set_row_data(page, row);
        state.shown.insert(page);
    }

    /// Start the worker, once.
    fn start(&self, ui: &AppWindow) {
        if self.started.replace(true) {
            return;
        }
        let (shared, viewer) = (self.shared.clone(), ViewerHandle(ui.as_weak()));
        std::thread::Builder::new()
            .name("pdf".into())
            .spawn(move || run(&shared, viewer))
            .map_err(|e| tracing::error!(error = %e, "start the pdf thread"))
            .ok();
    }
}

/// Finds the app's viewer from the worker's results.
struct ViewerHandle(slint::Weak<AppWindow>);

thread_local! {
    static VIEWER: RefCell<Option<std::rc::Rc<Viewer>>> = const { RefCell::new(None) };
}

/// The viewer the worker's results go to (UI thread).
pub fn register(viewer: &std::rc::Rc<Viewer>) {
    VIEWER.with(|v| *v.borrow_mut() = Some(viewer.clone()));
}

impl ViewerHandle {
    fn send(&self, f: impl FnOnce(&Viewer, &AppWindow) + Send + 'static) {
        let weak = self.0.clone();
        let _ = slint::invoke_from_event_loop(move || {
            let Some(ui) = weak.upgrade() else { return };
            VIEWER.with(|v| {
                if let Some(viewer) = v.borrow().as_ref() {
                    f(viewer, &ui);
                }
            });
        });
    }
}

/// The worker: opens documents and renders pages, as asked.
fn run(shared: &Shared, viewer: ViewerHandle) {
    use pdfium_render::prelude::*;
    let pdfium = bind();
    if let Err(e) = &pdfium {
        tracing::error!(error = %e, "load PDFium");
    }
    let mut doc: Option<(u64, PdfDocument)> = None;
    loop {
        enum Job {
            Open(u64, PathBuf),
            Render(u64, usize, u32),
            Close,
        }
        let job = {
            let mut work = shared.work();
            loop {
                if let Some(path) = work.open.take() {
                    break Job::Open(work.generation, path);
                }
                if doc.as_ref().is_some_and(|(g, _)| *g != work.generation) {
                    break Job::Close;
                }
                if let Some((page, width)) = work.renders.pop_front() {
                    break Job::Render(work.generation, page, width);
                }
                work = shared.wake.wait(work).unwrap_or_else(|e| e.into_inner());
            }
        };
        match job {
            Job::Close => doc = None,
            Job::Open(generation, path) => {
                doc = None;
                let result = match &pdfium {
                    Ok(pdfium) => load(pdfium, &path).map(|(d, aspects)| {
                        doc = Some((generation, d));
                        aspects
                    }),
                    Err(e) => Err(format!("The PDF viewer couldn't start ({e}). Open it in your PDF app instead.")),
                };
                if let Err(e) = &result {
                    tracing::warn!(path = %path.display(), error = %e, "open pdf");
                }
                viewer.send(move |v, ui| v.opened(ui, generation, result));
            }
            Job::Render(generation, page, width) => {
                let Some((_, d)) = doc.as_ref().filter(|(g, _)| *g == generation) else { continue };
                let started = std::time::Instant::now();
                match render(d, page, width) {
                    Ok(rendered) => {
                        tracing::trace!(page, width, links = rendered.1.len(), elapsed = ?started.elapsed(), "rendered");
                        viewer.send(move |v, ui| v.rendered(ui, generation, page, width, rendered));
                    }
                    Err(e) => tracing::warn!(page, error = %e, "render pdf page"),
                }
            }
        }
    }
}

/// Load PDFium (once per process).
fn bind() -> Result<pdfium_render::prelude::Pdfium, String> {
    use pdfium_render::prelude::Pdfium;
    let path = library().ok_or("PDFium wasn't found")?;
    let bindings = Pdfium::bind_to_library(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(Pdfium::new(bindings))
}

/// Open a document, with each page's height over its width.
fn load<'a>(
    pdfium: &'a pdfium_render::prelude::Pdfium,
    path: &Path,
) -> Result<(pdfium_render::prelude::PdfDocument<'a>, Vec<f32>), String> {
    let doc = pdfium.load_pdf_from_file(path, None).map_err(|e| describe(&e))?;
    let sizes = doc.pages().page_sizes().map_err(|e| describe(&e))?;
    if sizes.is_empty() {
        return Err("This PDF has no pages.".into());
    }
    let aspects = sizes.iter().map(|s| s.height().value / s.width().value.max(1.0)).collect();
    Ok((doc, aspects))
}

/// A rendered page: its pixels, and its links.
type Rendered = (SharedPixelBuffer<Rgba8Pixel>, Vec<Link>);

/// A page's pixels at a width, and its links.
fn render(
    doc: &pdfium_render::prelude::PdfDocument,
    page: usize,
    width: u32,
) -> Result<Rendered, pdfium_render::prelude::PdfiumError> {
    use pdfium_render::prelude::{PdfPageIndex, PdfRenderConfig, Pixels};
    let config = PdfRenderConfig::new().set_target_width(width as Pixels).render_form_data(true);
    let page = doc.pages().get(page as PdfPageIndex)?;
    let bitmap = page.render_with_config(&config)?;
    let (w, h) = (bitmap.width().max(0) as u32, bitmap.height().max(0) as u32);
    let links = links(&page, &config, w, h);
    Ok((SharedPixelBuffer::clone_from_slice(&bitmap.as_rgba_bytes(), w, h), links))
}

/// A page's links, placed on its render (`config`, `w` x `h` pixels): PDFium
/// turns their page points into pixels, so a rotated or cropped page has
/// them in the right place. Links that go nowhere we follow are left out.
fn links(
    page: &pdfium_render::prelude::PdfPage,
    config: &pdfium_render::prelude::PdfRenderConfig,
    w: u32,
    h: u32,
) -> Vec<Link> {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    page.links()
        .iter()
        .filter_map(|link| {
            let target = target(&link)?;
            let rect = link.rect().ok()?;
            let a = page.points_to_pixels(rect.left(), rect.top(), config).ok()?;
            let b = page.points_to_pixels(rect.right(), rect.bottom(), config).ok()?;
            let (x0, x1) = (a.0.min(b.0) as f32 / w, a.0.max(b.0) as f32 / w);
            let (y0, y1) = (a.1.min(b.1) as f32 / h, a.1.max(b.1) as f32 / h);
            (x1 > x0 && y1 > y0).then_some(Link { area: [x0, y0, x1 - x0, y1 - y0], target })
        })
        .collect()
}

/// Where a link goes: a page of this PDF (its destination, or a go-to
/// action's), or a web address (a URI action's).
fn target(link: &pdfium_render::prelude::PdfLink) -> Option<Target> {
    if let Some(dest) = link.destination() {
        return dest.page_index().ok().map(|p| Target::Page(p as usize));
    }
    let action = link.action()?;
    if let Some(uri) = action.as_uri_action() {
        return web(&uri.uri().ok()?);
    }
    let local = action.as_local_destination_action()?;
    local.destination().ok()?.page_index().ok().map(|p| Target::Page(p as usize))
}

/// Links as a page's row has them.
fn link_rows(links: &[Link]) -> slint::ModelRc<PdfLink> {
    let rows = links
        .iter()
        .map(|l| {
            let [x, y, w, h] = l.area;
            let (page, url) = match &l.target {
                Target::Page(p) => (*p as i32, String::new()),
                Target::Web(url) => (-1, url.clone()),
            };
            PdfLink { x, y, w, h, page, url: url.into() }
        })
        .collect();
    crate::course_panel::model(rows)
}

/// Snapshots: open a PDF and render its first pages now, on this thread (no
/// worker, no event loop to hand results back on).
pub fn sample(ui: &AppWindow, path: &str, page_width: f32) {
    let pdf = ui.global::<Pdf>();
    pdf.set_name(Path::new(path).file_name().unwrap_or_default().to_string_lossy().to_string().into());
    // As a course's resource, with its "Mark as done".
    pdf.set_resource("sample".into());
    pdf.set_open(true);
    let shown = bind().and_then(|pdfium| {
        let (doc, aspects) = load(&pdfium, Path::new(path))?;
        let rows = aspects
            .iter()
            .enumerate()
            .map(|(i, &aspect)| {
                let rendered = (i < 2).then(|| render(&doc, i, render_width(page_width, 1.0)).ok()).flatten();
                let links = rendered.as_ref().map(|(_, links)| link_rows(links)).unwrap_or_default();
                let image = rendered.map(|(pixels, _)| slint::Image::from_rgba8(pixels));
                PdfPage { has_image: image.is_some(), image: image.unwrap_or_default(), aspect, links }
            })
            .collect();
        pdf.set_pages(crate::course_panel::model(rows));
        Ok(())
    });
    if let Err(e) = shown {
        pdf.set_error(e.into());
    }
}

/// Why a PDF didn't open, for the viewer.
fn describe(e: &pdfium_render::prelude::PdfiumError) -> String {
    use pdfium_render::prelude::{PdfiumError, PdfiumInternalError};
    match e {
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::PasswordError) => {
            "This PDF needs a password. Open it in your PDF app instead.".into()
        }
        PdfiumError::IoError(e) => format!("The file couldn't be read: {e}"),
        PdfiumError::PdfiumLibraryInternalError(PdfiumInternalError::FormatError) => {
            "This file doesn't look like a PDF (or it's damaged).".into()
        }
        e => format!("This PDF couldn't be opened ({e})."),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pages_stack_with_their_spacing() {
        let pages = layout(&[1.5, 0.5, 1.0], 100.0);
        assert_eq!(pages, vec![(24.0, 150.0), (186.0, 50.0), (248.0, 100.0)]);
        // A 120px view from the top: page 1 only (page 2 starts at 186).
        assert_eq!(in_view(&pages, 0.0, 120.0), Some((0, 0, 0)));
        // Scrolled to 150: pages 1 and 2 show; a third of the way down (190)
        // is page 2.
        assert_eq!(in_view(&pages, 150.0, 120.0), Some((0, 2, 1)));
        assert_eq!(in_view(&[], 0.0, 100.0), None);
    }

    #[test]
    fn renders_at_the_shown_width_in_steps() {
        assert_eq!(render_width(800.0, 1.0), 800);
        assert_eq!(render_width(801.0, 1.0), 832);
        assert_eq!(render_width(800.0, 1.5), 1216);
        assert_eq!(render_width(10.0, 1.0), 64);
        assert_eq!(render_width(9000.0, 1.0), 4096);
    }

    /// A page 200x100 points, its left half blue. No xref table: PDFium
    /// rebuilds it, as it does for many PDFs in the wild.
    const HALF_BLUE: &str = "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
        2 0 obj << /Type /Pages /Kids [3 0 R] /Count 1 >> endobj\n\
        3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Contents 4 0 R >> endobj\n\
        4 0 obj << /Length 25 >> stream\n0 0 1 rg 0 0 100 100 re f\nendstream endobj\n\
        trailer << /Root 1 0 R >>\n%%EOF\n";

    #[test]
    fn pages_render_at_the_asked_width() {
        if library().is_none() {
            eprintln!("skipped: no PDFium beside the test (app/scripts/get-pdfium.ps1)");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("half blue.pdf");
        std::fs::write(&path, HALF_BLUE).unwrap();
        let pdfium = bind().unwrap();
        let (doc, aspects) = load(&pdfium, &path).unwrap();
        assert_eq!(aspects, vec![0.5]);
        let (pixels, links) = render(&doc, 0, 400).unwrap();
        assert!(links.is_empty());
        assert_eq!((pixels.width(), pixels.height()), (400, 200));
        let at = |x: usize, y: usize| pixels.as_slice()[y * 400 + x];
        assert_eq!(at(20, 100), Rgba8Pixel { r: 0, g: 0, b: 255, a: 255 });
        assert_eq!(at(380, 100), Rgba8Pixel { r: 255, g: 255, b: 255, a: 255 });
        assert!(render(&doc, 1, 400).is_err(), "there's one page");

        // Not a PDF, or not there: said plainly.
        let junk = tmp.path().join("junk.pdf");
        std::fs::write(&junk, "hello").unwrap();
        assert!(load(&pdfium, &junk).unwrap_err().contains("doesn't look like a PDF"));
        assert!(load(&pdfium, &tmp.path().join("gone.pdf")).unwrap_err().starts_with("The file couldn't be read"));
    }

    /// Two 200x100 pages. Page 1's links: its top left quarter to a web page,
    /// its bottom right to page 2, and its bottom left to a program (one we
    /// never open).
    const LINKED: &str = "%PDF-1.4\n1 0 obj << /Type /Catalog /Pages 2 0 R >> endobj\n\
        2 0 obj << /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 >> endobj\n\
        3 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] /Annots [5 0 R 6 0 R 7 0 R] >> endobj\n\
        4 0 obj << /Type /Page /Parent 2 0 R /MediaBox [0 0 200 100] >> endobj\n\
        5 0 obj << /Type /Annot /Subtype /Link /Rect [0 50 100 100] /A << /S /URI /URI (https://example.com/a) >> >> endobj\n\
        6 0 obj << /Type /Annot /Subtype /Link /Rect [100 0 200 50] /Dest [4 0 R /Fit] >> endobj\n\
        7 0 obj << /Type /Annot /Subtype /Link /Rect [0 0 50 50] /A << /S /URI /URI (file:///C:/Windows/System32/calc.exe) >> >> endobj\n\
        trailer << /Root 1 0 R >>\n%%EOF\n";

    #[test]
    fn links_are_placed_on_the_page() {
        if library().is_none() {
            eprintln!("skipped: no PDFium beside the test (app/scripts/get-pdfium.ps1)");
            return;
        }
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("linked.pdf");
        std::fs::write(&path, LINKED).unwrap();
        let pdfium = bind().unwrap();
        let (doc, _) = load(&pdfium, &path).unwrap();
        let (_, mut links) = render(&doc, 0, 400).unwrap();
        links.sort_by(|a, b| a.area[0].total_cmp(&b.area[0]));
        let near = |a: [f32; 4], b: [f32; 4]| a.iter().zip(b).all(|(x, y)| (x - y).abs() < 0.01);
        assert_eq!(links.len(), 2, "{links:?}");
        assert!(near(links[0].area, [0.0, 0.0, 0.5, 0.5]), "{links:?}");
        assert_eq!(links[0].target, Target::Web("https://example.com/a".into()));
        assert!(near(links[1].area, [0.5, 0.5, 0.5, 0.5]), "{links:?}");
        assert_eq!(links[1].target, Target::Page(1));
        assert!(render(&doc, 1, 400).unwrap().1.is_empty());
    }

    #[test]
    fn only_web_and_mail_links_open() {
        assert_eq!(web(" https://example.com "), Some(Target::Web("https://example.com".into())));
        assert_eq!(web("HTTP://x"), Some(Target::Web("HTTP://x".into())));
        assert_eq!(web("mailto:a@b.c"), Some(Target::Web("mailto:a@b.c".into())));
        for no in ["file:///C:/x.exe", "C:\\x.exe", "javascript:alert(1)", "ms-settings:", "smb://host/x", "https:", "example.com"] {
            assert_eq!(web(no), None, "{no}");
        }
    }

    #[test]
    fn zoom_steps_and_fits() {
        assert_eq!(next_zoom(1.0, 1), 1.1);
        assert_eq!(next_zoom(1.0, -1), 0.9);
        assert_eq!(next_zoom(3.0, 1), 3.0);
        assert_eq!(next_zoom(0.5, -1), 0.5);
        assert_eq!(next_zoom(2.2, 0), 1.0);
        assert!(is_pdf("C:/c/13.1 Notes.PDF") && !is_pdf("C:/c/notes.pdfx"));
    }
}
