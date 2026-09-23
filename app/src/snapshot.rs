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
pub fn sample_playback(ui: &crate::AppWindow) {
    use slint::ComponentHandle;
    ui.set_playing(true);
    let playback = ui.global::<crate::Playback>();
    playback.set_title("IAM Introduction: Users, Groups, Policies".into());
    playback.set_subtitle("IAM & AWS CLI".into());
    playback.set_stats("Decoder: d3d11va-copy".into());
    playback.set_clock("00:32".into());
    playback.set_ends_at("Ends at 00:35".into());
    playback.set_position(54.0);
    playback.set_duration(202.0);
    playback.set_paused(true);
    playback.set_volume(80.0);
    playback.set_has_next(true);
    playback.set_up_next("IAM Users & Groups Hands On".into());
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
