//! The video surface. libmpv renders each frame into a texture in Slint's own
//! OpenGL context, and an ordinary `Image` element shows that texture — so
//! anything declared after it in the `.slint` tree simply draws on top of the
//! video. No native child window, no rect reporting, on any platform.

use crate::{AppWindow, Playback};
use deskemy_core::importer::structure::clean_title;
use deskemy_core::mpv::{
    Mpv, MpvRenderContext, MPV_EVENT_FILE_LOADED, MPV_EVENT_PLAYBACK_RESTART, MPV_EVENT_SHUTDOWN,
    MPV_RENDER_UPDATE_FRAME,
};
use glow::HasContext;
use slint::{ComponentHandle, GraphicsAPI, RenderingState};
use std::ffi::c_void;
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;

/// A running player: the mpv core plus its event thread. The render side
/// lives in the window's rendering notifier (see [`Surface`]).
pub struct Player {
    mpv: Arc<Mpv>,
    events: Option<JoinHandle<()>>,
}

impl Player {
    /// Create an mpv core, hook it up to `ui`'s rendering, and queue `file`
    /// to start as soon as the GL render context exists.
    pub fn start(ui: &AppWindow, file: PathBuf) -> Result<Self, String> {
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
            ("keep-open", "yes"),
            ("idle", "yes"),
            ("config", "no"),
            ("terminal", "no"),
            ("osc", "no"),
            ("input-default-bindings", "no"),
            ("input-vo-keyboard", "no"),
        ] {
            if let Err(e) = mpv.set_option(name, value) {
                tracing::warn!(error = %e, "mpv option {name}={value} not supported");
            }
        }
        mpv.initialize().map_err(|e| e.to_string())?;

        let playback = ui.global::<Playback>();
        playback.set_title(display_name(&file).into());
        playback.set_subtitle(
            file.parent()
                .and_then(Path::file_name)
                .map(|s| clean_title(&s.to_string_lossy()))
                .unwrap_or_default()
                .into(),
        );
        wire_controls(ui, &mpv);

        let events = {
            let mpv = mpv.clone();
            let ui = ui.as_weak();
            std::thread::Builder::new()
                .name("mpv-events".into())
                .spawn(move || pump_events(&mpv, ui))
                .map_err(|e| e.to_string())?
        };

        let mut surface = Surface {
            render: None,
            target: None,
            gl: None,
            rendered: false,
            pending: Some(file),
            wake: Box::into_raw(Box::new(ui.as_weak())),
            ui: ui.as_weak(),
            mpv: mpv.clone(),
        };
        ui.window()
            .set_rendering_notifier(move |state, api| surface.on_rendering(state, api))
            .map_err(|e| format!("rendering notifier: {e:?}"))?;

        Ok(Player {
            mpv,
            events: Some(events),
        })
    }

    /// Stop playback and wait for the event thread to see mpv shut down.
    pub fn shutdown(mut self) {
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
    /// File to load once the render context exists (vo=libmpv needs it).
    pending: Option<PathBuf>,
    /// Handed to mpv's update callback; freed after the render context.
    wake: *mut slint::Weak<AppWindow>,
    ui: slint::Weak<AppWindow>,
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
        match unsafe { MpvRenderContext::new_gl(&self.mpv, *get_proc_address) } {
            Ok(render) => {
                render.set_update_callback(on_mpv_frame, self.wake as *mut c_void);
                self.render = Some(render);
            }
            Err(e) => {
                tracing::error!(error = %e, "mpv OpenGL render context");
                return;
            }
        }
        self.gl = Some(gl);
        tracing::info!("mpv rendering into Slint's OpenGL context");

        if let Some(file) = self.pending.take() {
            let path = file.to_string_lossy();
            if let Err(e) = self.mpv.command(&["loadfile", &path]) {
                tracing::error!(error = %e, file = %path, "loadfile");
            }
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
            if let Err(e) = unsafe { render.render_gl(target.fbo.0.get(), w, h) } {
                tracing::warn!(error = %e, "mpv render");
                return;
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

/// Saves the GL state Slint's renderer relies on and restores it on drop, so
/// mpv drawing into our framebuffer can't leak state into Slint's frame.
struct GlState<'a> {
    gl: &'a glow::Context,
    draw_fbo: i32,
    read_fbo: i32,
    viewport: [i32; 4],
    scissor: bool,
    blend: bool,
    program: i32,
    active_texture: i32,
    texture_2d: i32,
    vertex_array: i32,
    array_buffer: i32,
    unpack_alignment: i32,
}

impl<'a> GlState<'a> {
    fn save(gl: &'a glow::Context) -> Self {
        unsafe {
            let mut viewport = [0; 4];
            gl.get_parameter_i32_slice(glow::VIEWPORT, &mut viewport);
            GlState {
                gl,
                draw_fbo: gl.get_parameter_i32(glow::DRAW_FRAMEBUFFER_BINDING),
                read_fbo: gl.get_parameter_i32(glow::READ_FRAMEBUFFER_BINDING),
                viewport,
                scissor: gl.is_enabled(glow::SCISSOR_TEST),
                blend: gl.is_enabled(glow::BLEND),
                program: gl.get_parameter_i32(glow::CURRENT_PROGRAM),
                active_texture: gl.get_parameter_i32(glow::ACTIVE_TEXTURE),
                texture_2d: gl.get_parameter_i32(glow::TEXTURE_BINDING_2D),
                vertex_array: gl.get_parameter_i32(glow::VERTEX_ARRAY_BINDING),
                array_buffer: gl.get_parameter_i32(glow::ARRAY_BUFFER_BINDING),
                unpack_alignment: gl.get_parameter_i32(glow::UNPACK_ALIGNMENT),
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
            let toggle = |cap, on: bool| if on { gl.enable(cap) } else { gl.disable(cap) };
            toggle(glow::SCISSOR_TEST, self.scissor);
            toggle(glow::BLEND, self.blend);
            gl.use_program(name(self.program).map(glow::NativeProgram));
            gl.active_texture(self.active_texture as u32);
            gl.bind_texture(glow::TEXTURE_2D, name(self.texture_2d).map(glow::NativeTexture));
            gl.bind_vertex_array(name(self.vertex_array).map(glow::NativeVertexArray));
            gl.bind_buffer(glow::ARRAY_BUFFER, name(self.array_buffer).map(glow::NativeBuffer));
            gl.pixel_store_i32(glow::UNPACK_ALIGNMENT, self.unpack_alignment);
        }
    }
}

/// Route the overlay's actions to mpv.
fn wire_controls(ui: &AppWindow, mpv: &Arc<Mpv>) {
    let playback = ui.global::<Playback>();
    let run = |mpv: &Arc<Mpv>, args: &[&str]| {
        if let Err(e) = mpv.command(args) {
            tracing::warn!(error = %e, ?args, "mpv command");
        }
    };

    let m = mpv.clone();
    playback.on_toggle_pause(move || run(&m, &["cycle", "pause"]));
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

    let weak = ui.as_weak();
    playback.on_toggle_fullscreen(move || {
        if let Some(ui) = weak.upgrade() {
            let full = !ui.window().is_fullscreen();
            ui.window().set_fullscreen(full);
            ui.global::<Playback>().set_fullscreen(full);
        }
    });

    let m = mpv.clone();
    let weak = ui.as_weak();
    playback.on_back(move || {
        run(&m, &["stop"]);
        if let Some(ui) = weak.upgrade() {
            ui.window().set_fullscreen(false);
            ui.global::<Playback>().set_fullscreen(false);
            ui.set_playing(false);
        }
    });
}

/// Title for a bare file: its cleaned name, the way the library shows lectures.
fn display_name(file: &Path) -> String {
    file.file_name()
        .map(|n| clean_title(&n.to_string_lossy()))
        .unwrap_or_default()
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

/// Drain mpv's event queue (it must be drained) and push playback state to
/// the overlay about five times a second.
fn pump_events(mpv: &Mpv, ui: slint::Weak<AppWindow>) {
    let mut decoder = String::from("…");
    loop {
        let event = mpv.wait_event(0.2);
        if !event.is_null() {
            match unsafe { (*event).event_id } {
                MPV_EVENT_SHUTDOWN => break,
                MPV_EVENT_FILE_LOADED => {
                    tracing::info!(duration = ?mpv.get_f64("duration"), "file loaded");
                }
                MPV_EVENT_PLAYBACK_RESTART => {
                    decoder = mpv
                        .get_property_string("hwdec-current")
                        .unwrap_or_else(|| "software".into());
                    tracing::info!(%decoder, "decoding");
                }
                _ => {}
            }
        }

        let s = State::sample(mpv);
        let now = chrono::Local::now();
        let remaining = (s.duration - s.position).max(0.0) / s.speed.max(0.01);
        let ends = now + chrono::Duration::milliseconds((remaining * 1000.0) as i64);
        let clock = now.format("%H:%M").to_string();
        let ends_at = if s.duration > 0.0 {
            format!("Ends at {}", ends.format("%H:%M"))
        } else {
            String::new()
        };
        let stats = format!("Decoder: {decoder}");

        let _ = ui.upgrade_in_event_loop(move |ui| {
            let playback = ui.global::<Playback>();
            playback.set_position(s.position as f32);
            playback.set_duration(s.duration as f32);
            playback.set_paused(s.paused);
            playback.set_volume(s.volume as f32);
            playback.set_muted(s.muted);
            playback.set_clock(clock.into());
            playback.set_ends_at(ends_at.into());
            playback.set_stats(stats.into());
        });
    }
}
