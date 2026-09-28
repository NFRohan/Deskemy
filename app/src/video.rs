//! The video surface. libmpv renders each frame into a texture in Slint's own
//! OpenGL context, and an ordinary `Image` element shows that texture — so
//! anything declared after it in the `.slint` tree simply draws on top of the
//! video. No native child window, no rect reporting, on any platform.

use crate::session::{Db, Session, Sleep};
use crate::{course_panel, stats, tracks};
use crate::{AppWindow, BookmarkRow, MenuItem, Playback};
use crate::settings::Config;
use deskemy_core::mpv::{
    Mpv, MpvEventEndFile, MpvRenderContext, NativeDisplay, MPV_END_FILE_REASON_EOF, MPV_EVENT_END_FILE,
    MPV_EVENT_FILE_LOADED, MPV_EVENT_PLAYBACK_RESTART, MPV_EVENT_SHUTDOWN,
    MPV_RENDER_UPDATE_FRAME,
};
use glow::HasContext;
use slint::{ComponentHandle, GraphicsAPI, RenderingState};
use std::ffi::c_void;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::num::NonZeroU32;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;

/// Work to run once mpv can draw (vo=libmpv has no output before that).
pub type OnReady = Box<dyn FnOnce(&Session)>;

/// A running player: the mpv core, its playback session and its event
/// thread. The render side lives in the window's rendering notifier (see
/// [`Surface`]).
pub struct Player {
    session: Arc<Session>,
    mpv: Arc<Mpv>,
    events: Option<JoinHandle<()>>,
}

impl Player {
    /// Create the mpv core and session and hook them up to `ui`. `on_ready`
    /// runs once the GL render context exists — the earliest a file can load.
    pub fn start(
        ui: &AppWindow,
        db: Db,
        config: Config,
        on_ready: Option<OnReady>,
    ) -> Result<Self, String> {
        let mpv = Arc::new(Mpv::new().map_err(|e| e.to_string())?);
        // GPU decode by default; DESKEMY_HWDEC overrides (e.g. `no` to compare).
        let hwdec = std::env::var("DESKEMY_HWDEC").unwrap_or_else(|_| "auto-safe".into());
        for (name, value) in [("vo", "libmpv"), ("hwdec", hwdec.as_str())] {
            mpv.set_option(name, value)
                .map_err(|e| format!("mpv option {name}={value}: {e}"))?;
        }
        // Nice-to-haves: some libmpv builds lack e.g. `osc` (no Lua), so a
        // missing one is logged rather than fatal.
        for (name, value) in [
            // End-of-file must fire END_FILE so the lecture completes/advances.
            ("keep-open", "no"),
            ("idle", "yes"),
            // Pick up sidecar subtitles (e.g. Udemy .srt) matching the video.
            ("sub-auto", "fuzzy"),
            ("config", "no"),
            ("terminal", "no"),
            ("osc", "no"),
            ("osd-level", "0"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
        ] {
            if let Err(e) = mpv.set_option(name, value) {
                tracing::warn!(error = %e, "mpv option {name}={value} not supported");
            }
        }
        // mpv's own log, beside ours: how it decodes and renders (texture
        // formats, hwdec interop, errors).
        if let Some(dir) = deskemy_core::paths::data_dir().map(|d| d.join("logs")) {
            let _ = std::fs::create_dir_all(&dir);
            if let Err(e) = mpv.set_option("log-file", &dir.join("mpv.log").to_string_lossy()) {
                tracing::warn!(error = %e, "mpv log-file");
            }
        }
        mpv.initialize().map_err(|e| e.to_string())?;

        let session = Arc::new(Session::new(mpv.clone(), db, config));
        wire_controls(ui, &session, &mpv);

        // Playback info is only read from mpv while its overlay is open.
        let stats_open = Arc::new(AtomicBool::new(false));
        ui.global::<Playback>().on_toggle_stats({
            let (open, weak) = (stats_open.clone(), ui.as_weak());
            move || {
                let now = !open.fetch_xor(true, Ordering::Relaxed);
                if let Some(ui) = weak.upgrade() {
                    ui.global::<Playback>().set_stats_open(now);
                }
            }
        });

        let events = {
            let (session, mpv, ui) = (session.clone(), mpv.clone(), ui.as_weak());
            std::thread::Builder::new()
                .name("mpv-events".into())
                .spawn(move || pump_events(&mpv, &session, &stats_open, ui))
                .map_err(|e| e.to_string())?
        };

        let mut surface = Surface {
            render: None,
            target: None,
            gl: None,
            rendered: false,
            warned_before: false,
            warned_after: false,
            on_ready,
            wake: Box::into_raw(Box::new(ui.as_weak())),
            ui: ui.as_weak(),
            session: session.clone(),
            mpv: mpv.clone(),
        };
        ui.window()
            .set_rendering_notifier(move |state, api| surface.on_rendering(state, api))
            .map_err(|e| format!("rendering notifier: {e:?}"))?;

        Ok(Player {
            session,
            mpv,
            events: Some(events),
        })
    }

    pub fn session(&self) -> &Arc<Session> {
        &self.session
    }

    /// Save progress, stop mpv and wait for its event thread to finish.
    pub fn shutdown(mut self) {
        self.session.save_now();
        let _ = self.mpv.command(&["quit"]);
        if let Some(events) = self.events.take() {
            let _ = events.join();
        }
    }
}

/// Render-side state, owned by the rendering notifier and only ever touched
/// on the UI thread with Slint's GL context current.
struct Surface {
    // Declared first so it drops before `mpv`: the render context must be
    // freed while the core is still alive.
    render: Option<MpvRenderContext>,
    target: Option<Target>,
    gl: Option<glow::Context>,
    /// Whether this frame drew a new video frame (→ report the swap).
    rendered: bool,
    /// GL errors around mpv's frame have been logged (once each).
    warned_before: bool,
    warned_after: bool,
    /// Runs once the render context exists (vo=libmpv needs it to load).
    on_ready: Option<OnReady>,
    /// Handed to mpv's update callback; freed after the render context.
    wake: *mut slint::Weak<AppWindow>,
    ui: slint::Weak<AppWindow>,
    session: Arc<Session>,
    mpv: Arc<Mpv>,
}

impl Surface {
    fn on_rendering(&mut self, state: RenderingState, api: &GraphicsAPI) {
        match state {
            RenderingState::RenderingSetup => self.setup(api),
            RenderingState::BeforeRendering => self.draw(),
            RenderingState::AfterRendering => {
                if std::mem::take(&mut self.rendered) {
                    if let Some(render) = &self.render {
                        render.report_swap();
                    }
                }
            }
            RenderingState::RenderingTeardown => self.teardown(),
            _ => {}
        }
    }

    fn setup(&mut self, api: &GraphicsAPI) {
        let GraphicsAPI::NativeOpenGL { get_proc_address } = api else {
            tracing::error!("video needs Slint's OpenGL renderer; got another graphics API");
            return;
        };
        let gl = unsafe { glow::Context::from_loader_function_cstr(|name| get_proc_address(name)) };
        unsafe {
            let profile = gl.get_parameter_i32(glow::CONTEXT_PROFILE_MASK);
            tracing::info!(
                version = %gl.get_parameter_string(glow::VERSION),
                renderer = %gl.get_parameter_string(glow::RENDERER),
                vendor = %gl.get_parameter_string(glow::VENDOR),
                profile = if profile & glow::CONTEXT_CORE_PROFILE_BIT as i32 != 0 { "core" } else { "compatibility" },
                "OpenGL context"
            );
        }
        let display = self.ui.upgrade().map_or(NativeDisplay::None, |ui| native_display(&ui));
        match unsafe { MpvRenderContext::new_gl(&self.mpv, *get_proc_address, display) } {
            Ok(render) => {
                // `wake` lives until after the render context is dropped
                // (see Drop for Surface), and a slint::Weak is safe to use
                // from mpv's thread.
                unsafe { render.set_update_callback(on_mpv_frame, self.wake as *mut c_void) };
                self.render = Some(render);
            }
            Err(e) => {
                tracing::error!(error = %e, "mpv OpenGL render context");
                return;
            }
        }
        self.gl = Some(gl);
        tracing::info!("mpv rendering into Slint's OpenGL context");

        if let Some(ready) = self.on_ready.take() {
            ready(&self.session);
        }
    }

    fn draw(&mut self) {
        let (Some(gl), Some(render), Some(ui)) = (&self.gl, &self.render, self.ui.upgrade()) else {
            return;
        };
        let scale = ui.window().scale_factor();
        let w = (ui.get_video_width() * scale).round() as i32;
        let h = (ui.get_video_height() * scale).round() as i32;
        if w <= 0 || h <= 0 {
            return;
        }

        // Always ask mpv what it wants — it expects an update() per callback.
        let new_frame = render.update() & MPV_RENDER_UPDATE_FRAME != 0;

        let resized = self.target.as_ref().is_none_or(|t| t.w != w || t.h != h);
        if resized {
            if let Some(old) = self.target.take() {
                unsafe { old.delete(gl) };
            }
            match unsafe { Target::new(gl, w, h) } {
                Ok(target) => {
                    let image = unsafe {
                        slint::BorrowedOpenGLTextureBuilder::new_gl_2d_rgba_texture(
                            target.texture.0,
                            (w as u32, h as u32).into(),
                        )
                    }
                    .origin(slint::BorrowedOpenGLTextureOrigin::TopLeft)
                    .build();
                    ui.set_video_frame(image);
                    self.target = Some(target);
                }
                Err(e) => {
                    tracing::error!(error = %e, "video texture");
                    return;
                }
            }
        }

        // Redraw on a new frame, or after a resize so the texture isn't blank.
        if new_frame || resized {
            let Some(target) = &self.target else { return };
            let _state = GlState::save(gl);
            unsafe {
                // Errors pending from the renderer would be blamed on mpv.
                let before = gl_errors(gl);
                if !before.is_empty() && !self.warned_before {
                    self.warned_before = true;
                    tracing::warn!(errors = ?before, "GL errors pending before mpv's frame");
                }
                reset_for_mpv(gl);
            }
            if let Err(e) = unsafe { render.render_gl(target.fbo.0.get(), w, h) } {
                tracing::warn!(error = %e, "mpv render");
                return;
            }
            let after = unsafe { gl_errors(gl) };
            if !after.is_empty() && !self.warned_after {
                self.warned_after = true;
                tracing::warn!(errors = ?after, "GL errors from mpv's frame");
            }
            self.rendered = true;
        }
    }

    fn teardown(&mut self) {
        // GL is still current here — the only safe place to free GL objects
        // and the mpv render context.
        self.render = None;
        if let (Some(gl), Some(target)) = (&self.gl, self.target.take()) {
            unsafe { target.delete(gl) };
        }
        self.gl = None;
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        // The render context unregisters mpv's callback when dropped, so only
        // then is it safe to free the pointer that callback was given.
        if self.render.take().is_some() {
            tracing::warn!("mpv render context freed outside RenderingTeardown");
        }
        drop(unsafe { Box::from_raw(self.wake) });
    }
}

/// mpv calls this from its own thread when a frame is ready: just ask the UI
/// thread for a redraw, whose BeforeRendering pass does the actual render.
extern "C" fn on_mpv_frame(data: *mut c_void) {
    let ui = unsafe { &*(data as *const slint::Weak<AppWindow>) };
    let _ = ui.upgrade_in_event_loop(|ui| ui.window().request_redraw());
}

/// The texture mpv draws into, and the framebuffer object wrapping it.
struct Target {
    texture: glow::NativeTexture,
    fbo: glow::NativeFramebuffer,
    w: i32,
    h: i32,
}

impl Target {
    unsafe fn new(gl: &glow::Context, w: i32, h: i32) -> Result<Self, String> {
        let _state = GlState::save(gl);
        let texture = gl.create_texture()?;
        gl.bind_texture(glow::TEXTURE_2D, Some(texture));
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MIN_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_MAG_FILTER, glow::LINEAR as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_S, glow::CLAMP_TO_EDGE as i32);
        gl.tex_parameter_i32(glow::TEXTURE_2D, glow::TEXTURE_WRAP_T, glow::CLAMP_TO_EDGE as i32);
        gl.tex_image_2d(
            glow::TEXTURE_2D,
            0,
            glow::RGBA8 as i32,
            w,
            h,
            0,
            glow::RGBA,
            glow::UNSIGNED_BYTE,
            glow::PixelUnpackData::Slice(None),
        );
        let fbo = gl.create_framebuffer()?;
        gl.bind_framebuffer(glow::FRAMEBUFFER, Some(fbo));
        gl.framebuffer_texture_2d(
            glow::FRAMEBUFFER,
            glow::COLOR_ATTACHMENT0,
            glow::TEXTURE_2D,
            Some(texture),
            0,
        );
        let status = gl.check_framebuffer_status(glow::FRAMEBUFFER);
        if status != glow::FRAMEBUFFER_COMPLETE {
            gl.delete_framebuffer(fbo);
            gl.delete_texture(texture);
            return Err(format!("framebuffer incomplete (0x{status:x})"));
        }
        Ok(Target { texture, fbo, w, h })
    }

    unsafe fn delete(self, gl: &glow::Context) {
        gl.delete_framebuffer(self.fbo);
        gl.delete_texture(self.texture);
    }
}

/// Drain the GL error queue (as hex codes).
unsafe fn gl_errors(gl: &glow::Context) -> Vec<String> {
    let mut errors = Vec::new();
    loop {
        let e = gl.get_error();
        if e == glow::NO_ERROR || errors.len() >= 8 {
            break errors;
        }
        errors.push(format!("{e:#06x}"));
    }
}

/// Texture units whose bindings are saved (mpv's shaders use the first few).
const UNITS: u32 = 8;

/// Saves the GL state Slint's renderer relies on and restores it on drop, so
/// mpv drawing into our framebuffer can't leak state into Slint's frame. Skia
/// caches GL state between draws rather than re-setting it, so this covers
/// everything mpv's renderer is known to touch.
struct GlState<'a> {
    gl: &'a glow::Context,
    draw_fbo: i32,
    read_fbo: i32,
    viewport: [i32; 4],
    scissor_box: [i32; 4],
    caps: Vec<(u32, bool)>,
    blend_func: [i32; 4],
    blend_equation: [i32; 2],
    color_mask: [i32; 4],
    program: i32,
    active_texture: i32,
    /// (TEXTURE_2D, sampler) per unit.
    units: Vec<(i32, i32)>,
    vertex_array: i32,
    array_buffer: i32,
    element_buffer: i32,
    unpack_alignment: i32,
    unpack_row_length: i32,
    pack_alignment: i32,
    pixel_unpack_buffer: i32,
    pixel_pack_buffer: i32,
}

/// Put the GL state mpv relies on back to GL's defaults before it draws.
/// mpv's renderer assumes blending is off unless a pass turns it on, and
/// Skia leaves it on: the intermediate chroma-scaling pass then blends
/// instead of writing, the chroma comes out zero and the picture turns
/// green (until a size where mpv skips that pass, e.g. fullscreen). Skia
/// also leaves its sampler objects bound, which would override the
/// filtering of mpv's textures, and may leave a pixel buffer bound,
/// scissoring on or a partial colour mask. `GlState` restores Skia's state
/// afterwards.
unsafe fn reset_for_mpv(gl: &glow::Context) {
    if has_samplers(gl) {
        for unit in 0..UNITS {
            gl.bind_sampler(unit, None);
        }
    }
    gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, None);
    gl.bind_buffer(glow::PIXEL_PACK_BUFFER, None);
    gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, 4);
    gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, 0);
    gl.pixel_store_i32(glow::PACK_ALIGNMENT, 4);
    for cap in [glow::BLEND, glow::SCISSOR_TEST, glow::STENCIL_TEST, glow::DEPTH_TEST, glow::CULL_FACE] {
        gl.disable(cap);
    }
    gl.blend_func(glow::ONE, glow::ZERO);
    gl.blend_equation(glow::FUNC_ADD);
    if !gl.version().is_embedded {
        gl.disable(glow::FRAMEBUFFER_SRGB);
    }
    gl.color_mask(true, true, true, true);
    gl.active_texture(glow::TEXTURE0);
}

/// Sampler objects arrived in OpenGL 3.3 (and ES 3.0); older contexts lack
/// the entry points entirely.
fn has_samplers(gl: &glow::Context) -> bool {
    let v = gl.version();
    (v.major, v.minor) >= (if v.is_embedded { (3, 0) } else { (3, 3) })
}

/// Capabilities saved and restored as on/off.
const CAPS: [u32; 7] = [
    glow::SCISSOR_TEST,
    glow::BLEND,
    glow::DEPTH_TEST,
    glow::STENCIL_TEST,
    glow::CULL_FACE,
    glow::DITHER,
    glow::FRAMEBUFFER_SRGB,
];

impl<'a> GlState<'a> {
    fn save(gl: &'a glow::Context) -> Self {
        unsafe {
            let slice = |name| {
                let mut v = [0; 4];
                gl.get_parameter_i32_slice(name, &mut v);
                v
            };
            let active_texture = gl.get_parameter_i32(glow::ACTIVE_TEXTURE);
            let samplers = has_samplers(gl);
            let units = (0..UNITS)
                .map(|i| {
                    gl.active_texture(glow::TEXTURE0 + i);
                    let sampler = if samplers { gl.get_parameter_i32(glow::SAMPLER_BINDING) } else { 0 };
                    (gl.get_parameter_i32(glow::TEXTURE_BINDING_2D), sampler)
                })
                .collect();
            gl.active_texture(active_texture as u32);
            GlState {
                gl,
                draw_fbo: gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
                read_fbo: gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
                viewport: slice(glow::VIEWPORT),
                scissor_box: slice(glow::SCISSOR_BOX),
                // sRGB framebuffer control is desktop GL only.
                caps: CAPS
                    .iter()
                    .filter(|&&cap| cap != glow::FRAMEBUFFER_SRGB || !gl.version().is_embedded)
                    .map(|&cap| (cap, gl.is_enabled(cap)))
                    .collect(),
                blend_func: [
                    gl.get_parameter_i32(glow::BLEND_SRC_RGB),
                    gl.get_parameter_i32(glow::BLEND_DST_RGB),
                    gl.get_parameter_i32(glow::BLEND_SRC_ALPHA),
                    gl.get_parameter_i32(glow::BLEND_DST_ALPHA),
                ],
                blend_equation: [
                    gl.get_parameter_i32(glow::BLEND_EQUATION_RGB),
                    gl.get_parameter_i32(glow::BLEND_EQUATION_ALPHA),
                ],
                color_mask: slice(glow::COLOR_WRITEMASK),
                program: gl.get_parameter_i32(glow::CURRENT_PROGRAM),
                active_texture,
                units,
                vertex_array: gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING),
                array_buffer: gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING),
                element_buffer: gl.get_parameter_i32(glow::ELEMENT_ARRAY_BUFFER_BINDING),
                unpack_alignment: gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
                unpack_row_length: gl.get_parameter_i32(glow::UNPACK_ROW_LENGTH),
                pack_alignment: gl.get_parameter_i32(glow::PACK_ALIGNMENT),
                pixel_unpack_buffer: gl.get_parameter_i32(glow::PIXEL_UNPACK_BUFFER_BINDING),
                pixel_pack_buffer: gl.get_parameter_i32(glow::PIXEL_PACK_BUFFER_BINDING),
            }
        }
    }
}

impl Drop for GlState<'_> {
    fn drop(&mut self) {
        fn name(v: i32) -> Option<NonZeroU32> {
            NonZeroU32::new(v as u32)
        }
        let gl = self.gl;
        unsafe {
            gl.bind_framebuffer(glow::DRAW_FRAMEBUFFER, name(self.draw_fbo).map(glow::NativeFramebuffer));
            gl.bind_framebuffer(glow::READ_FRAMEBUFFER, name(self.read_fbo).map(glow::NativeFramebuffer));
            let [x, y, w, h] = self.viewport;
            gl.viewport(x, y, w, h);
            let [x, y, w, h] = self.scissor_box;
            gl.scissor(x, y, w, h);
            for &(cap, on) in &self.caps {
                if on {
                    gl.enable(cap)
                } else {
                    gl.disable(cap)
                }
            }
            let [src_rgb, dst_rgb, src_a, dst_a] = self.blend_func.map(|v| v as u32);
            gl.blend_func_separate(src_rgb, dst_rgb, src_a, dst_a);
            gl.blend_equation_separate(self.blend_equation[0] as u32, self.blend_equation[1] as u32);
            let [r, g, b, a] = self.color_mask.map(|v| v != 0);
            gl.color_mask(r, g, b, a);
            gl.use_program(name(self.program).map(glow::NativeProgram));
            let samplers = has_samplers(gl);
            for (i, &(texture, sampler)) in self.units.iter().enumerate() {
                gl.active_texture(glow::TEXTURE0 + i as u32);
                gl.bind_texture(glow::TEXTURE_2D, name(texture).map(glow::NativeTexture));
                if samplers {
                    gl.bind_sampler(i as u32, name(sampler).map(glow::NativeSampler));
                }
            }
            gl.active_texture(self.active_texture as u32);
            // The element buffer belongs to the vertex array: bind that first.
            gl.bind_vertex_array(name(self.vertex_array).map(glow::NativeVertexArray));
            gl.bind_buffer(glow::ELEMENT_ARRAY_BUFFER, name(self.element_buffer).map(glow::NativeBuffer));
            gl.bind_buffer(glow::ARRAY_BUFFER, name(self.array_buffer).map(glow::NativeBuffer));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
            gl.pixel_store_i32(glow::UNPACK_ROW_LENGTH, self.unpack_row_length);
            gl.pixel_store_i32(glow::PACK_ALIGNMENT, self.pack_alignment);
            gl.bind_buffer(glow::PIXEL_UNPACK_BUFFER, name(self.pixel_unpack_buffer).map(glow::NativeBuffer));
            gl.bind_buffer(glow::PIXEL_PACK_BUFFER, name(self.pixel_pack_buffer).map(glow::NativeBuffer));
        }
    }
}

/// Route the overlay's actions to mpv and the session.
fn wire_controls(ui: &AppWindow, session: &Arc<Session>, mpv: &Arc<Mpv>) {
    let playback = ui.global::<Playback>();
    let run = |mpv: &Arc<Mpv>, args: &[&str]| {
        if let Err(e) = mpv.command(args) {
            tracing::warn!(error = %e, ?args, "mpv command");
        }
    };

    let (s, m) = (session.clone(), mpv.clone());
    playback.on_toggle_pause(move || {
        // After the last lecture ends mpv is idle; play means "watch again".
        if !s.replay_if_ended() {
            run(&m, &["cycle", "pause"]);
        }
    });
    let m = mpv.clone();
    playback.on_seek(move |t| run(&m, &["seek", &format!("{t:.3}"), "absolute"]));
    let m = mpv.clone();
    playback.on_seek_by(move |d| run(&m, &["seek", &format!("{d:.3}"), "relative"]));
    let m = mpv.clone();
    playback.on_toggle_mute(move || run(&m, &["cycle", "mute"]));
    let m = mpv.clone();
    playback.on_set_volume(move |v| {
        let _ = m.set_property("mute", "no");
        if let Err(e) = m.set_property("volume", &format!("{v:.0}")) {
            tracing::warn!(error = %e, "set volume");
        }
    });

    let s = session.clone();
    playback.on_pick_speed(move |i| {
        if let Some(&speed) = tracks::SPEEDS.get(i as usize) {
            s.set_speed(speed);
        }
    });
    let s = session.clone();
    playback.on_pick_subtitle(move |id| s.set_subtitle((id >= 0).then_some(id as i64)));
    let s = session.clone();
    playback.on_pick_audio(move |id| s.set_audio(id as i64));
    let s = session.clone();
    playback.on_pick_chapter(move |i| s.set_chapter(i as i64));

    let s = session.clone();
    playback.on_sleep_for(move |minutes| s.set_sleep(Sleep::after(minutes.max(1) as u32)));
    let s = session.clone();
    playback.on_sleep_at_end(move || s.set_sleep(Sleep::EndOfLecture));
    let s = session.clone();
    playback.on_sleep_off(move || s.set_sleep(Sleep::Off));

    wire_bookmarks(ui, session, mpv);
    wire_panel(ui, session);

    let (s, m) = (session.clone(), mpv.clone());
    playback.on_toggle_subtitles(move || {
        let t = tracks::read(&m);
        if t.sid.is_some() {
            s.set_subtitle(None);
        } else if let Some(first) = t.subtitles.first() {
            s.set_subtitle(Some(first.id));
        }
    });
    let s = session.clone();
    playback.on_use_default_speed(move || s.use_default_speed());

    let (s, m) = (session.clone(), mpv.clone());
    playback.on_step_speed(move |dir| {
        let current = m.get_f64("speed").unwrap_or(1.0);
        s.set_speed(tracks::stepped_speed(current, dir));
    });

    let step = |session: &Arc<Session>, delta: i32| {
        let s = session.clone();
        move || {
            if let Err(e) = s.step(delta) {
                tracing::warn!(error = %e, delta, "change lecture");
            }
        }
    };
    playback.on_previous(step(session, -1));
    playback.on_next(step(session, 1));

    let weak = ui.as_weak();
    playback.on_toggle_fullscreen(move || {
        if let Some(ui) = weak.upgrade() {
            let full = !ui.window().is_fullscreen();
            ui.window().set_fullscreen(full);
            ui.global::<Playback>().set_fullscreen(full);
        }
    });

    let weak = ui.as_weak();
    playback.on_cursor_hidden(move |hidden| {
        if let Some(ui) = weak.upgrade() {
            set_cursor_visible(&ui, !hidden);
        }
    });

    let (s, m, weak) = (session.clone(), mpv.clone(), ui.as_weak());
    playback.on_back(move || {
        s.save_now();
        run(&m, &["stop"]);
        if let Some(ui) = weak.upgrade() {
            // The pages always show the pointer, however the player left it.
            set_cursor_visible(&ui, true);
            ui.window().set_fullscreen(false);
            ui.global::<Playback>().set_fullscreen(false);
            ui.set_playing(false);
            // Progress changed while watching.
            ui.invoke_refresh_library();
        }
    });
}

/// "Bookmark this moment": opening the dialog captures the position and
/// pauses; closing resumes if it was playing (as in the Tauri player).
fn wire_bookmarks(ui: &AppWindow, session: &Arc<Session>, mpv: &Arc<Mpv>) {
    let playback = ui.global::<Playback>();
    // (position being bookmarked, resume on close) — UI thread only.
    let draft = Rc::new(Cell::new((0.0_f64, false)));

    let show = |playback: &Playback, session: &Session| {
        let rows: Vec<BookmarkRow> = session
            .bookmarks()
            .into_iter()
            .map(|b| BookmarkRow {
                id: b.id.into(),
                time: tracks::clock(b.position_seconds).into(),
                label: b.label.unwrap_or_else(|| "Bookmark".into()).into(),
                position: b.position_seconds as f32,
            })
            .collect();
        playback.set_bookmarks(slint::ModelRc::new(slint::VecModel::from(rows)));
    };
    let close = {
        let (draft, m, weak) = (draft.clone(), mpv.clone(), ui.as_weak());
        move || {
            let (_, resume) = draft.take();
            if resume {
                let _ = m.set_property("pause", "no");
            }
            if let Some(ui) = weak.upgrade() {
                ui.global::<Playback>().set_open_menu("".into());
            }
        }
    };

    let (s, m, d, weak) = (session.clone(), mpv.clone(), draft.clone(), ui.as_weak());
    playback.on_open_bookmark(move || {
        let Some(ui) = weak.upgrade() else { return };
        let position = m.get_f64("time-pos").unwrap_or(0.0);
        let playing = m.get_property_string("pause").as_deref() == Some("no");
        if playing {
            let _ = m.set_property("pause", "yes");
        }
        d.set((position, playing));
        let playback = ui.global::<Playback>();
        playback.set_bookmark_time(tracks::clock(position).into());
        show(&playback, &s);
        playback.set_open_menu("bookmark".into());
    });

    let (s, d, done) = (session.clone(), draft.clone(), close.clone());
    playback.on_save_bookmark(move |label| {
        let label = label.trim();
        if let Err(e) = s.add_bookmark(d.get().0, (!label.is_empty()).then_some(label)) {
            tracing::warn!(error = %e, "add bookmark");
        }
        done();
    });

    let (m, done) = (mpv.clone(), close.clone());
    playback.on_jump_to_bookmark(move |position| {
        let _ = m.command(&["seek", &format!("{position:.3}"), "absolute"]);
        done();
    });

    playback.on_close_bookmark(close);

    let (s, weak) = (session.clone(), ui.as_weak());
    playback.on_delete_bookmark(move |id| {
        s.delete_bookmark(&id);
        if let Some(ui) = weak.upgrade() {
            show(&ui.global::<Playback>(), &s);
        }
    });
}

/// Which sections are expanded in the course panel (UI thread only).
#[derive(Default)]
struct PanelState {
    course: Option<String>,
    expanded: HashSet<String>,
    /// The lecture whose section was last opened automatically.
    auto_for: Option<String>,
}

/// The course panel (P) and resources (R).
fn wire_panel(ui: &AppWindow, session: &Arc<Session>) {
    let playback = ui.global::<Playback>();
    let state = Rc::new(RefCell::new(PanelState::default()));

    let refresh = {
        let (s, state, weak) = (session.clone(), state.clone(), ui.as_weak());
        move || {
            let Some(ui) = weak.upgrade() else { return };
            let playback = ui.global::<Playback>();
            let Some(course) = s.course() else {
                playback.set_panel_sections(course_panel::model(Vec::new()));
                playback.set_panel_resources(course_panel::model(Vec::new()));
                playback.set_resources_section("".into());
                playback.set_resources_count(0);
                return;
            };
            let current = s.lecture_id();
            let mut state = state.borrow_mut();
            if state.course.as_deref() != Some(course.id.as_str()) {
                *state = PanelState {
                    course: Some(course.id.clone()),
                    ..PanelState::default()
                };
            }
            // Open the playing lecture's section whenever the lecture changes.
            if state.auto_for != current {
                if let Some(section) = course_panel::current_section(&course, current.as_deref()) {
                    state.expanded.insert(section.id.clone());
                }
                state.auto_for = current.clone();
            }
            let attachments = s.attachments();
            playback.set_panel_sections(course_panel::model(course_panel::sections(
                &course,
                &attachments,
                s.resources_inline(),
                current.as_deref(),
                &state.expanded,
            )));
            let resources = course_panel::resources(&course, &attachments, current.as_deref());
            playback.set_resources_section(resources.section.into());
            playback.set_resources_count(resources.count as i32);
            playback.set_panel_resources(course_panel::model(resources.groups));
        }
    };

    let (st, update) = (state.clone(), refresh.clone());
    playback.on_toggle_section(move |id| {
        {
            let mut st = st.borrow_mut();
            if !st.expanded.remove(id.as_str()) {
                st.expanded.insert(id.to_string());
            }
        }
        update();
    });
    playback.on_refresh_panel(refresh.clone());

    let (s, weak) = (session.clone(), ui.as_weak());
    playback.on_toggle_resource(move |id, done| {
        s.set_resource_done(&id, done);
        refresh();
        // The end-of-lecture card lists them too.
        if let Some(ui) = weak.upgrade() {
            let playback = ui.global::<Playback>();
            if playback.get_open_menu() == "exercise" {
                playback.set_prompt_items(course_panel::model(
                    s.lecture_resources().iter().map(course_panel::resource_item).collect(),
                ));
            }
        }
    });

    let s = session.clone();
    playback.on_open_lecture(move |id| {
        // Resumes where it was left, like opening it from the library.
        if let Err(e) = s.open(&id) {
            tracing::warn!(error = %e, lecture = %id, "open lecture");
        }
    });
    playback.on_open_resource(|path| {
        if let Err(e) = open::that_detached(path.as_str()) {
            tracing::warn!(error = %e, %path, "open resource");
        }
    });
}

/// What the overlay shows, sampled from mpv on the event thread.
struct State {
    position: f64,
    duration: f64,
    paused: bool,
    volume: f64,
    muted: bool,
    speed: f64,
}

impl State {
    fn sample(mpv: &Mpv) -> Self {
        let flag = |name| mpv.get_property_string(name).as_deref() == Some("yes");
        State {
            position: mpv.get_f64("time-pos").unwrap_or(0.0),
            duration: mpv.get_f64("duration").unwrap_or(0.0),
            paused: flag("pause"),
            volume: mpv.get_f64("volume").unwrap_or(100.0),
            muted: flag("mute"),
            speed: mpv.get_f64("speed").unwrap_or(1.0),
        }
    }
}

/// The player's long-lived thread: drains mpv's events (they must be
/// drained), drives the session (end of file, periodic saves), keeps the
/// machine awake while playing, and pushes state to the overlay ~5×/s.
fn pump_events(mpv: &Mpv, session: &Session, stats_open: &AtomicBool, ui: slint::Weak<AppWindow>) {
    let mut shown_revision = u64::MAX;
    let mut shown_tracks = None;
    let mut awake = false;
    loop {
        let event = mpv.wait_event(0.2);
        if !event.is_null() {
            match unsafe { (*event).event_id } {
                MPV_EVENT_SHUTDOWN => break,
                MPV_EVENT_FILE_LOADED => {
                    tracing::info!(duration = ?mpv.get_f64("duration"), "file loaded");
                }
                MPV_EVENT_PLAYBACK_RESTART => {
                    let decoder = mpv.get_property_string("hwdec-current");
                    tracing::info!(decoder = decoder.as_deref().unwrap_or("software"), "decoding");
                }
                MPV_EVENT_END_FILE => {
                    let data = unsafe { (*event).data } as *const MpvEventEndFile;
                    // Only a real end of file — not `stop` or a replacing loadfile.
                    if !data.is_null() && unsafe { (*data).reason } == MPV_END_FILE_REASON_EOF {
                        session.on_eof();
                    }
                }
                _ => {}
            }
        }

        let s = State::sample(mpv);
        session.tick(s.position, s.duration, s.paused);

        let want_awake = !s.paused && session.is_loaded();
        if want_awake != awake {
            awake = want_awake;
            set_keep_awake(awake);
        }

        let (revision, now_playing) = session.now_playing();
        let now_playing = (revision != shown_revision).then(|| {
            shown_revision = revision;
            now_playing
        });

        // Re-read tracks only when their fingerprint moves: a file loaded, a
        // sidecar subtitle attached late, or a pick took effect.
        let signature = tracks::signature(mpv);
        let menus = (shown_tracks.as_ref() != Some(&signature)).then(|| {
            shown_tracks = Some(signature);
            Menus::from(&tracks::read(mpv))
        });

        let now = chrono::Local::now();
        let remaining = (s.duration - s.position).max(0.0) / s.speed.max(0.01);
        let ends = now + chrono::Duration::milliseconds((remaining * 1000.0) as i64);
        let clock = now.format("%H:%M").to_string();
        let ends_at = if s.duration > 0.0 {
            format!("Ends at {}", ends.format("%H:%M"))
        } else {
            String::new()
        };
        let stats = stats_open.load(Ordering::Relaxed).then(|| stats::read(mpv));
        let (sleep_mode, sleep_badge, sleep_minutes) = match session.sleep() {
            Sleep::Off => ("off", String::new(), 0),
            Sleep::EndOfLecture => ("lecture", String::new(), 0),
            Sleep::At { deadline, minutes } => {
                let left = deadline.saturating_duration_since(std::time::Instant::now());
                ("minutes", format!("{}m", left.as_secs().div_ceil(60).max(1)), minutes as i32)
            }
        };

        // Autoplay stopped at a lecture's resources: offer them.
        let waiting: Option<Vec<crate::ResourceItem>> = now_playing
            .as_ref()
            .filter(|np| np.resources_waiting)
            .map(|_| session.lecture_resources().iter().map(course_panel::resource_item).collect());

        let _ = ui.upgrade_in_event_loop(move |ui| {
            let playback = ui.global::<Playback>();
            if let Some(menus) = menus {
                menus.apply(&playback);
            }
            if let Some(items) = waiting {
                playback.set_prompt_items(course_panel::model(items));
                playback.set_open_menu("exercise".into());
            }
            if let Some(np) = now_playing {
                playback.set_title(np.title.into());
                playback.set_subtitle(np.section.into());
                playback.set_course_title(np.course.into());
                playback.set_course_id(np.course_id.into());
                playback.set_up_next(np.up_next.unwrap_or_default().into());
                playback.set_has_previous(np.has_previous);
                playback.set_has_next(np.has_next);
                playback.set_in_library(np.in_library);
                playback.set_speed_default(np.speed_default.into());
                if playback.get_panel_open() {
                    playback.invoke_refresh_panel();
                }
            }
            playback.set_position(s.position as f32);
            playback.set_duration(s.duration as f32);
            playback.set_paused(s.paused);
            playback.set_volume(s.volume as f32);
            playback.set_muted(s.muted);
            playback.set_clock(clock.into());
            playback.set_ends_at(ends_at.into());
            if let Some(groups) = stats {
                playback.set_stats(course_panel::model(stats::rows(groups)));
            }
            playback.set_sleep_mode(sleep_mode.into());
            playback.set_sleep_badge(sleep_badge.into());
            playback.set_sleep_minutes(sleep_minutes);
        });
    }
    set_keep_awake(false);
}

/// The overlay's menus, built on the event thread and applied on the UI one.
struct Menus {
    speed_label: String,
    subtitles_on: bool,
    speeds: Vec<MenuItem>,
    subtitles: Vec<MenuItem>,
    audio: Vec<MenuItem>,
    chapters: Vec<MenuItem>,
}

impl Menus {
    fn from(t: &tracks::Tracks) -> Self {
        Menus {
            speed_label: tracks::speed_label(t.speed),
            subtitles_on: t.sid.is_some(),
            speeds: tracks::speed_menu(t),
            subtitles: tracks::subtitle_menu(t),
            audio: tracks::audio_menu(t),
            chapters: tracks::chapter_menu(t),
        }
    }

    fn apply(self, playback: &Playback) {
        let model = |items: Vec<MenuItem>| slint::ModelRc::new(slint::VecModel::from(items));
        playback.set_speed_label(self.speed_label.into());
        playback.set_subtitles_on(self.subtitles_on);
        playback.set_speeds(model(self.speeds));
        playback.set_subtitles(model(self.subtitles));
        playback.set_audio_tracks(model(self.audio));
        playback.set_chapters(model(self.chapters));
    }
}

/// The window's X11 / Wayland display, which mpv's hardware-decoding interop
/// needs on Linux. Other platforms (and XCB-only windows) have none to give.
fn native_display(ui: &AppWindow) -> NativeDisplay {
    use slint::ComponentHandle;
    use raw_window_handle::{HasDisplayHandle, RawDisplayHandle};
    let handle = ui.window().window_handle();
    match handle.display_handle().map(|h| h.as_raw()) {
        Ok(RawDisplayHandle::Xlib(x)) => x.display.map_or(NativeDisplay::None, |d| NativeDisplay::X11(d.as_ptr())),
        Ok(RawDisplayHandle::Wayland(w)) => NativeDisplay::Wayland(w.display.as_ptr()),
        _ => NativeDisplay::None,
    }
}

/// Show or hide the pointer over the window.
fn set_cursor_visible(ui: &AppWindow, visible: bool) {
    use slint::winit_030::WinitWindowAccessor;
    ui.window().with_winit_window(|w| w.set_cursor_visible(visible));
}

/// Keep the machine and display awake while a video is actually playing.
/// mpv can't do this itself here: with vo=libmpv it has no window for its
/// stop-screensaver to act on. Windows scopes the request to the calling
/// thread, so this is only called from the long-lived event thread.
#[cfg(windows)]
fn set_keep_awake(on: bool) {
    use windows_sys::Win32::System::Power::{
        SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, ES_SYSTEM_REQUIRED,
    };
    let flags = if on {
        ES_CONTINUOUS | ES_SYSTEM_REQUIRED | ES_DISPLAY_REQUIRED
    } else {
        ES_CONTINUOUS
    };
    unsafe { SetThreadExecutionState(flags) };
    tracing::debug!(on, "sleep inhibit");
}

#[cfg(not(windows))]
fn set_keep_awake(_on: bool) {}
