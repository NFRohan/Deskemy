//! Phase-1 spike for the C1 compositing path (see docs/player-compositing.md).
//!
//! Proves the gating unknown: can a DirectComposition visual painted on the main
//! window HWND show *through* a transparent, windowed-hosted WebView2? This paints
//! a solid magenta swapchain behind the webview — no mpv yet. If the magenta shows
//! where the page is transparent and stays glued during resizes, C1 is viable in
//! our own code; if not, we need visual hosting (a wry patch).
#![cfg(windows)]

use crate::error::{DeskemyError, Result};
use crate::mpv::MpvRenderContext;
use std::ffi::CStr;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::JoinHandle;
use tauri::{AppHandle, Manager};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView, ID3D11Texture2D,
    D3D11_CPU_ACCESS_WRITE, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_WRITE, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_IGNORE, DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM,
    DXGI_FORMAT_UNKNOWN, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_CHAIN_FLAG, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
    DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

/// mpv software output format matching a BGRA8 D3D texture (opaque; X byte).
const SW_FORMAT: &CStr = c"bgr0";

// ─────────────────────────────── Path decision ─────────────────────────────
// Whether this run uses the compositing player (mpv beneath a transparent
// WebView, DOM overlays float over the video) or the native `wid` child-window
// player (video on top, panels dock). Decided once, cached, read by both the UI
// (`compositor_enabled`) and the lazily-created player so they always agree.

static ACTIVE: OnceLock<bool> = OnceLock::new();

/// Decide once (and cache) whether the compositing player path is active.
///
/// Default is **on**: Windows, when a GPU/DirectComposition probe succeeds — so
/// the video pane can host DOM overlays (the floating cheat sheet, transparent
/// pane, etc.). Machines where the probe fails (no BGRA-capable D3D11 device or
/// DirectComposition unavailable — old GPUs, some VMs / RDP sessions) fall back
/// to the native `wid` child-window player, which always works. Override with
/// `DESKEMY_COMPOSITOR`: `0`/`off`/`false`/`no` forces the wid player; any other
/// value (`1`, `force`, …) forces compositing and skips the probe (that's what
/// `DESKEMY_COMPOSITOR=1 npm run tauri dev` does).
///
/// Warmed from `setup()` on the main thread; safe to call again — it's cached.
pub fn decide() -> bool {
    *ACTIVE.get_or_init(|| match std::env::var("DESKEMY_COMPOSITOR").ok().as_deref() {
        Some("0" | "off" | "false" | "no") => {
            tracing::info!("compositor: forced off via DESKEMY_COMPOSITOR → wid player");
            false
        }
        Some(_) => {
            tracing::info!("compositor: forced on via DESKEMY_COMPOSITOR (probe skipped)");
            true
        }
        None => match unsafe { probe() } {
            Ok(()) => {
                tracing::info!("compositor: GPU/DComp probe passed → compositing player");
                true
            }
            Err(e) => {
                tracing::info!(error = ?e, "compositor: probe failed → wid player fallback");
                false
            }
        },
    })
}

/// The cached decision (false until `decide` has run).
pub fn is_active() -> bool {
    ACTIVE.get().copied().unwrap_or(false)
}

/// Side-effect-free feasibility probe: build the same D3D11 + DirectComposition
/// objects the compositor needs, then drop them. Unlike `feasibility_test` it
/// attaches nothing to the window and leaks nothing — it only answers "would the
/// GPU path work here?". These three calls are what actually fail on unsupported
/// setups; the mpv render context is guaranteed by our bundled libmpv.
unsafe fn probe() -> windows::core::Result<()> {
    let mut device: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )?;
    let device = device.unwrap();
    let dxgi_device: IDXGIDevice = device.cast()?;
    let adapter = dxgi_device.GetAdapter()?;
    let factory: IDXGIFactory2 = adapter.GetParent()?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 16,
        Height: 16,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Scaling: DXGI_SCALING_STRETCH,
        ..Default::default()
    };
    let _swapchain: IDXGISwapChain1 =
        factory.CreateSwapChainForComposition(&device, &desc, None)?;
    let _dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
    Ok(())
}

/// Run the solid-color compositing test against the main window.
pub fn feasibility_test(app: &AppHandle) -> Result<()> {
    let win = app
        .get_webview_window("main")
        .ok_or_else(|| DeskemyError::Player("no main window".into()))?;
    let raw = win
        .hwnd()
        .map_err(|e| DeskemyError::Player(format!("hwnd: {e}")))?;
    // HWND wraps a raw pointer (!Send); ferry the address across as an isize.
    let hwnd_addr = raw.0 as isize;

    // DirectComposition is single-threaded and must run on the UI thread (same as
    // the window). Set it up there and report the result back.
    let (tx, rx) = std::sync::mpsc::channel();
    app.run_on_main_thread(move || {
        let hwnd = HWND(hwnd_addr as *mut _);
        let _ = tx.send(unsafe { setup(hwnd) });
    })
    .map_err(|e| DeskemyError::Player(format!("run_on_main_thread: {e}")))?;
    rx.recv()
        .map_err(|e| DeskemyError::Player(format!("channel: {e}")))?
        .map_err(|e| DeskemyError::Player(format!("compositor: {e:?}")))
}

unsafe fn setup(hwnd: HWND) -> windows::core::Result<()> {
    // D3D11 device (BGRA support required for composition).
    let mut device: Option<ID3D11Device> = None;
    D3D11CreateDevice(
        None,
        D3D_DRIVER_TYPE_HARDWARE,
        HMODULE::default(),
        D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        None,
        D3D11_SDK_VERSION,
        Some(&mut device),
        None,
        None,
    )?;
    let device = device.unwrap();
    let dxgi_device: IDXGIDevice = device.cast()?;

    // A composition swapchain (not bound to a HWND) — same shape we'll use for mpv.
    let adapter = dxgi_device.GetAdapter()?;
    let factory: IDXGIFactory2 = adapter.GetParent()?;
    let desc = DXGI_SWAP_CHAIN_DESC1 {
        Width: 600,
        Height: 400,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
        BufferCount: 2,
        SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
        AlphaMode: DXGI_ALPHA_MODE_PREMULTIPLIED,
        Scaling: DXGI_SCALING_STRETCH,
        ..Default::default()
    };
    let swapchain: IDXGISwapChain1 = factory.CreateSwapChainForComposition(&device, &desc, None)?;

    // Clear the backbuffer to (premultiplied) magenta and present it.
    let backbuffer: ID3D11Texture2D = swapchain.GetBuffer(0)?;
    let mut rtv: Option<ID3D11RenderTargetView> = None;
    device.CreateRenderTargetView(&backbuffer, None, Some(&mut rtv))?;
    let rtv = rtv.unwrap();
    let ctx = device.GetImmediateContext()?;
    let color = [1.0f32, 0.0, 1.0, 1.0];
    ctx.ClearRenderTargetView(&rtv, &color);
    swapchain.Present(0, DXGI_PRESENT(0)).ok()?;

    // DComp: device → target for the HWND → visual holding the swapchain → commit.
    let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
    let target: IDCompositionTarget = dcomp.CreateTargetForHwnd(hwnd, false)?;
    let visual: IDCompositionVisual = dcomp.CreateVisual()?;
    visual.SetContent(&swapchain)?;
    target.SetRoot(&visual)?;
    dcomp.Commit()?;

    // Spike: leak the COM objects so the composition survives past this call.
    std::mem::forget((device, dxgi_device, swapchain, rtv, dcomp, target, visual));
    Ok(())
}

// ─────────────────────────────── Compositor ───────────────────────────────
// Drives mpv's software render into a DirectComposition visual beneath the
// transparent webview, so the DOM and video share one surface (no airspace).

#[derive(Clone, Copy, Default)]
struct Rect {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
}

struct State {
    dirty: bool,      // mpv has a new frame to draw
    rect: Rect,       // target pane rect (device px)
    insets: (i32, i32, i32, i32), // pane margins from the window edges (l,t,r,b)
    rect_dirty: bool, // rect changed → resize/reposition
    running: bool,
}

struct Shared {
    m: Mutex<State>,
    cv: Condvar,
}

/// Owns the compositor render thread. Dropping it stops the thread, frees the
/// mpv render context, and tears down the composition tree.
pub struct Compositor {
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
}

impl Compositor {
    /// Spawn the render thread. `hwnd` is the main window; `rctx` an already-
    /// created software render context bound to the active mpv handle; the rect
    /// is the initial pane in device px.
    pub fn new(hwnd: isize, rctx: MpvRenderContext, x: i32, y: i32, w: i32, h: i32) -> Compositor {
        let shared = Arc::new(Shared {
            m: Mutex::new(State {
                dirty: true,
                rect: Rect {
                    x,
                    y,
                    w: w.max(1),
                    h: h.max(1),
                },
                insets: (0, 0, 0, 0),
                rect_dirty: true,
                running: true,
            }),
            cv: Condvar::new(),
        });
        let s2 = shared.clone();
        let thread = std::thread::Builder::new()
            .name("compositor".into())
            .spawn(move || render_loop(hwnd, rctx, s2))
            .ok();
        Compositor { shared, thread }
    }

    /// Set the pane rect and its margins from the window edges (all device px);
    /// wakes the render thread. Reported from JS when the layout changes.
    pub fn set_rect(&self, x: i32, y: i32, w: i32, h: i32, insets: (i32, i32, i32, i32)) {
        let mut st = self.shared.m.lock().unwrap_or_else(|e| e.into_inner());
        st.rect = Rect {
            x,
            y,
            w: w.max(1),
            h: h.max(1),
        };
        st.insets = insets;
        st.rect_dirty = true;
        self.shared.cv.notify_one();
    }

    /// Recompute the pane from the last insets for a new window size (device px).
    /// Driven natively from `WindowEvent::Resized` so fullscreen/edge-drag resizes
    /// track the window without the JS round-trip.
    pub fn resize(&self, win_w: i32, win_h: i32) {
        let mut st = self.shared.m.lock().unwrap_or_else(|e| e.into_inner());
        let (il, it, ir, ib) = st.insets;
        st.rect = Rect {
            x: il,
            y: it,
            w: (win_w - il - ir).max(1),
            h: (win_h - it - ib).max(1),
        };
        st.rect_dirty = true;
        self.shared.cv.notify_one();
    }
}

impl Drop for Compositor {
    fn drop(&mut self) {
        {
            let mut st = self.shared.m.lock().unwrap_or_else(|e| e.into_inner());
            st.running = false;
            self.shared.cv.notify_one();
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// mpv update callback (called from an arbitrary thread) — flag a frame + wake.
extern "C" fn on_mpv_update(data: *mut std::ffi::c_void) {
    // SAFETY: `data` is a live `Arc<Shared>` for the render thread's lifetime.
    let shared = unsafe { &*(data as *const Shared) };
    let mut st = shared.m.lock().unwrap_or_else(|e| e.into_inner());
    st.dirty = true;
    shared.cv.notify_one();
}

fn render_loop(hwnd: isize, rctx: MpvRenderContext, shared: Arc<Shared>) {
    let init = shared.m.lock().unwrap_or_else(|e| e.into_inner()).rect;
    let mut gpu = match unsafe { Gpu::new(hwnd, init.w, init.h) } {
        Ok(g) => g,
        Err(e) => {
            tracing::error!(error = ?e, "compositor GPU init failed");
            return;
        }
    };
    // Frames arrive via this callback; hand it a borrowed Shared pointer.
    rctx.set_update_callback(on_mpv_update, Arc::as_ptr(&shared) as *mut _);
    let mut cur = init;

    loop {
        let (render, resize, stop) = {
            let mut st = shared.m.lock().unwrap_or_else(|e| e.into_inner());
            while st.running && !st.dirty && !st.rect_dirty {
                st = shared.cv.wait(st).unwrap_or_else(|e| e.into_inner());
            }
            let stop = !st.running;
            let render = std::mem::replace(&mut st.dirty, false);
            let resize = st.rect_dirty.then(|| {
                st.rect_dirty = false;
                st.rect
            });
            (render, resize, stop)
        };
        if stop {
            break;
        }
        if let Some(r) = resize {
            if r.w != cur.w || r.h != cur.h {
                if let Err(e) = unsafe { gpu.resize(r.w, r.h) } {
                    tracing::warn!(error = ?e, "compositor resize failed");
                }
            }
            if let Err(e) = unsafe { gpu.reposition(r.x, r.y) } {
                tracing::warn!(error = ?e, "compositor reposition failed");
            }
            cur = r;
        }
        if render || resize.is_some() {
            if let Err(e) = unsafe { gpu.render_frame(&rctx) } {
                tracing::warn!(error = ?e, "compositor render failed");
            }
        }
    }
    // gpu + rctx dropped here; freeing rctx unregisters the update callback.
}

struct Gpu {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    swapchain: IDXGISwapChain1,
    staging: ID3D11Texture2D,
    dcomp: IDCompositionDevice,
    _target: IDCompositionTarget,
    visual: IDCompositionVisual,
    w: i32,
    h: i32,
}

impl Gpu {
    unsafe fn new(hwnd: isize, w: i32, h: i32) -> windows::core::Result<Gpu> {
        let (w, h) = (w.max(1), h.max(1));
        let mut device: Option<ID3D11Device> = None;
        let mut context: Option<ID3D11DeviceContext> = None;
        D3D11CreateDevice(
            None,
            D3D_DRIVER_TYPE_HARDWARE,
            HMODULE::default(),
            D3D11_CREATE_DEVICE_BGRA_SUPPORT,
            None,
            D3D11_SDK_VERSION,
            Some(&mut device),
            None,
            Some(&mut context),
        )?;
        let device = device.unwrap();
        let context = context.unwrap();
        let dxgi_device: IDXGIDevice = device.cast()?;
        let adapter = dxgi_device.GetAdapter()?;
        let factory: IDXGIFactory2 = adapter.GetParent()?;

        let desc = DXGI_SWAP_CHAIN_DESC1 {
            Width: w as u32,
            Height: h as u32,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC {
                Count: 1,
                Quality: 0,
            },
            BufferUsage: DXGI_USAGE_RENDER_TARGET_OUTPUT,
            BufferCount: 2,
            SwapEffect: DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL,
            AlphaMode: DXGI_ALPHA_MODE_IGNORE,
            Scaling: DXGI_SCALING_STRETCH,
            ..Default::default()
        };
        let swapchain: IDXGISwapChain1 =
            factory.CreateSwapChainForComposition(&device, &desc, None)?;
        let staging = create_staging(&device, w, h)?;

        let dcomp: IDCompositionDevice = DCompositionCreateDevice(&dxgi_device)?;
        let target = dcomp.CreateTargetForHwnd(HWND(hwnd as *mut _), false)?;
        let visual = dcomp.CreateVisual()?;
        visual.SetContent(&swapchain)?;
        target.SetRoot(&visual)?;
        dcomp.Commit()?;

        Ok(Gpu {
            device,
            context,
            swapchain,
            staging,
            dcomp,
            _target: target,
            visual,
            w,
            h,
        })
    }

    unsafe fn resize(&mut self, w: i32, h: i32) -> windows::core::Result<()> {
        let (w, h) = (w.max(1), h.max(1));
        self.swapchain
            .ResizeBuffers(0, w as u32, h as u32, DXGI_FORMAT_UNKNOWN, DXGI_SWAP_CHAIN_FLAG(0))?;
        self.staging = create_staging(&self.device, w, h)?;
        self.w = w;
        self.h = h;
        Ok(())
    }

    unsafe fn reposition(&self, x: i32, y: i32) -> windows::core::Result<()> {
        self.visual.SetOffsetX2(x as f32)?;
        self.visual.SetOffsetY2(y as f32)?;
        self.dcomp.Commit()?;
        Ok(())
    }

    unsafe fn render_frame(&self, rctx: &MpvRenderContext) -> windows::core::Result<()> {
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        self.context
            .Map(&self.staging, 0, D3D11_MAP_WRITE, 0, Some(&mut mapped))?;
        // mpv writes the frame straight into the mapped staging memory.
        let _ = rctx.render_sw(
            mapped.pData,
            mapped.RowPitch as usize,
            self.w,
            self.h,
            SW_FORMAT,
        );
        self.context.Unmap(&self.staging, 0);
        let backbuffer: ID3D11Texture2D = self.swapchain.GetBuffer(0)?;
        self.context.CopyResource(&backbuffer, &self.staging);
        self.swapchain.Present(0, DXGI_PRESENT(0)).ok()?;
        Ok(())
    }
}

unsafe fn create_staging(
    device: &ID3D11Device,
    w: i32,
    h: i32,
) -> windows::core::Result<ID3D11Texture2D> {
    let desc = D3D11_TEXTURE2D_DESC {
        Width: w as u32,
        Height: h as u32,
        MipLevels: 1,
        ArraySize: 1,
        Format: DXGI_FORMAT_B8G8R8A8_UNORM,
        SampleDesc: DXGI_SAMPLE_DESC {
            Count: 1,
            Quality: 0,
        },
        Usage: D3D11_USAGE_STAGING,
        BindFlags: 0,
        CPUAccessFlags: D3D11_CPU_ACCESS_WRITE.0 as u32,
        MiscFlags: 0,
    };
    let mut tex: Option<ID3D11Texture2D> = None;
    device.CreateTexture2D(&desc, None, Some(&mut tex))?;
    Ok(tex.unwrap())
}
