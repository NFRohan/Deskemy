//! Phase-1 spike for the C1 compositing path (see docs/player-compositing.md).
//!
//! Proves the gating unknown: can a DirectComposition visual painted on the main
//! window HWND show *through* a transparent, windowed-hosted WebView2? This paints
//! a solid magenta swapchain behind the webview — no mpv yet. If the magenta shows
//! where the page is transparent and stays glued during resizes, C1 is viable in
//! our own code; if not, we need visual hosting (a wry patch).
#![cfg(windows)]

use crate::error::{DeskemyError, Result};
use tauri::{AppHandle, Manager};
use windows::core::Interface;
use windows::Win32::Foundation::{HMODULE, HWND};
use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11RenderTargetView, ID3D11Texture2D,
    D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION,
};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice, IDCompositionDevice, IDCompositionTarget, IDCompositionVisual,
};
use windows::Win32::Graphics::Dxgi::Common::{
    DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC,
};
use windows::Win32::Graphics::Dxgi::{
    IDXGIDevice, IDXGIFactory2, IDXGISwapChain1, DXGI_PRESENT, DXGI_SCALING_STRETCH,
    DXGI_SWAP_CHAIN_DESC1, DXGI_SWAP_EFFECT_FLIP_SEQUENTIAL, DXGI_USAGE_RENDER_TARGET_OUTPUT,
};

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
