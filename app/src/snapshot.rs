//! `--snapshot <file.png>`: render one frame of the UI offscreen with Slint's
//! software renderer and write it as a PNG — no window is ever created, so it
//! is safe to run anywhere (CI, a headless box, or while the desktop is busy).
//! Anything drawn through OpenGL (the video surface) is not captured.

use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{Platform, WindowAdapter};
use slint::{PhysicalSize, PlatformError, Rgb8Pixel};
use std::path::Path;
use std::rc::Rc;

struct SnapshotPlatform {
    window: Rc<MinimalSoftwareWindow>,
}

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(self.window.clone())
    }
}

/// Install the offscreen platform. Must run before any component is created.
pub fn install(width: u32, height: u32) -> Result<Rc<MinimalSoftwareWindow>, PlatformError> {
    let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
    window.set_size(PhysicalSize::new(width, height));
    slint::platform::set_platform(Box::new(SnapshotPlatform {
        window: window.clone(),
    }))
    .map_err(|e| PlatformError::Other(e.to_string()))?;
    Ok(window)
}

/// Put the player overlay on screen with plausible state, for layout checks.
pub fn sample_playback(ui: &crate::AppWindow, menu: &str, db: &crate::session::Db) {
    use slint::ComponentHandle;
    ui.set_playing(true);
    let playback = ui.global::<crate::Playback>();
    playback.set_title("IAM Introduction: Users, Groups, Policies".into());
    playback.set_subtitle("IAM & AWS CLI".into());
    // Playback info as it looks for a typical course lecture.
    let line = |label, value: &str| (label, value.to_string());
    let groups: crate::stats::Groups = vec![
        ("Video", vec![line("Codec", "H.264"), line("Resolution", "1920x1080"), line("FPS", "30.00"),
            line("Bitrate", "1.9 Mbps"), line("Decoder", "d3d11va-copy")]),
        ("Audio", vec![line("Codec", "AAC"), line("Sample rate", "48.0 kHz"), line("Channels", "2"),
            line("Bitrate", "128 kbps")]),
        ("Color", vec![line("Pixel format", "nv12"), line("HW format", "d3d11"), line("Matrix", "bt.709"),
            line("Primaries", "bt.709"), line("Transfer", "bt.1886")]),
        ("Performance", vec![line("Render FPS", "30.00"), line("A/V sync", "0ms"), line("Dropped", "0")]),
        ("Buffer", vec![line("Cached", "142.3s"), line("Speed", "3.1 MB/s")]),
        ("App", vec![line("Player", "mpv v0.40.0"), line("Renderer", "Slint · OpenGL")]),
    ];
    playback.set_stats(crate::course_panel::model(crate::stats::rows(groups)));
    playback.set_stats_open(menu == "stats");
    playback.set_clock("00:32".into());
    playback.set_ends_at("Ends at 00:35".into());
    playback.set_position(54.0);
    playback.set_duration(202.0);
    playback.set_paused(true);
    playback.set_volume(80.0);
    playback.set_has_next(true);
    playback.set_up_next("IAM Users & Groups Hands On".into());

    // A file with chapters and two subtitle tracks, a sleep countdown running.
    let t = crate::tracks::Tracks {
        subtitles: vec![
            crate::tracks::Track { id: 1, lang: Some("en".into()), filename: Some("1. IAM Introduction.en.srt".into()), ..Default::default() },
            crate::tracks::Track { id: 2, lang: Some("es".into()), filename: Some("1. IAM Introduction.es.srt".into()), ..Default::default() },
        ],
        chapters: ["Welcome", "Users and groups", "Policies", "Recap"]
            .iter()
            .enumerate()
            .map(|(i, title)| crate::tracks::Chapter { title: Some(title.to_string()), time: i as f64 * 48.0 })
            .collect(),
        sid: Some(1),
        chapter: 1,
        speed: 1.25,
        ..Default::default()
    };
    let model = |items: Vec<crate::MenuItem>| slint::ModelRc::new(slint::VecModel::from(items));
    playback.set_speed_label(crate::tracks::speed_label(t.speed).into());
    playback.set_subtitles_on(true);
    playback.set_speeds(model(crate::tracks::speed_menu(&t)));
    playback.set_subtitles(model(crate::tracks::subtitle_menu(&t)));
    playback.set_chapters(model(crate::tracks::chapter_menu(&t)));
    playback.set_sleep_mode("minutes".into());
    playback.set_sleep_badge("27m".into());
    playback.set_sleep_minutes(30);
    playback.set_in_library(true);
    playback.set_bookmark_time("0:54".into());
    let marks = [("0:18", "Root account warning", 18.0), ("1:41", "Policy JSON structure", 101.0)]
        .map(|(time, label, position)| crate::BookmarkRow {
            id: label.into(),
            time: time.into(),
            label: label.into(),
            position,
        });
    playback.set_bookmarks(slint::ModelRc::new(slint::VecModel::from(marks.to_vec())));
    match menu {
        // The course panel, filled from the most recently watched course.
        "content" | "resources" => {
            playback.set_panel_tab(menu.into());
            playback.set_panel_open(true);
            sample_panel(&playback, db);
        }
        "stats" => {}
        _ => playback.set_open_menu(menu.into()),
    }
}

fn sample_panel(playback: &crate::Playback, db: &crate::session::Db) {
    use crate::course_panel;
    use deskemy_core::db::queries;
    let conn = db.lock().unwrap_or_else(|e| e.into_inner());
    let Some((course, lecture)) = queries::list_course_summaries(&conn)
        .ok()
        .and_then(|cs| cs.into_iter().find_map(|c| Some((c.id, c.last_lecture_id?))))
        .and_then(|(id, lecture)| Some((queries::get_course_detail(&conn, &id).ok()??, lecture)))
    else {
        return;
    };
    let attachments = queries::list_course_attachments(&conn, &course.id).unwrap_or_default();
    let expanded = course_panel::current_section(&course, Some(&lecture))
        .map(|s| std::iter::once(s.id.clone()).collect())
        .unwrap_or_default();
    playback.set_panel_sections(course_panel::model(course_panel::sections(&course, Some(&lecture), &expanded)));
    let resources = course_panel::resources(&course, &attachments, Some(&lecture));
    playback.set_resources_section(resources.section.into());
    playback.set_resources_count(resources.count as i32);
    playback.set_panel_resources(course_panel::model(resources.groups));
}

/// Draw the current frame of `window` and save it to `path`.
pub fn save(window: &MinimalSoftwareWindow, path: &Path) -> Result<(), String> {
    slint::platform::update_timers_and_animations();
    let size = window.size();
    let (w, h) = (size.width as usize, size.height as usize);
    let mut pixels = vec![Rgb8Pixel::new(0, 0, 0); w * h];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, w);
    });

    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), w as u32, h as u32);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    let bytes: Vec<u8> = pixels.iter().flat_map(|p| [p.r, p.g, p.b]).collect();
    encoder
        .write_header()
        .and_then(|mut writer| writer.write_image_data(&bytes))
        .map_err(|e| e.to_string())
}
