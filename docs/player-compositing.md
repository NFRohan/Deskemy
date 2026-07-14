# Player compositing (C1) — killing airspace

Branch: `player-compositing`. Goal: composite mpv's video **into the same surface
as the WebView** so DOM and video resize atomically — eliminating the airspace
lag/bounce for good, and (bonus) unlocking real overlays over the video.

Status legend: ☐ todo · ◐ in progress · ☑ done · ✗ blocked

---

## Why we're here

Today mpv renders into a `STATIC` **child HWND** (`--wid`) that we raise above
the WebView2 sibling (`move_child` → `HWND_TOP`, see `player/mod.rs`). Two
separate surfaces (WebView2's HWND + mpv's HWND) that the OS never composites
together, so their resizes can't be atomic:

- WebView pane resizes on the GPU compositor instantly.
- mpv HWND is repositioned via JS → IPC → main thread → `SetWindowPos`, 1–2+
  frames behind → the video visibly chases/bounces on fullscreen + panel toggles.

Mitigations tried and rejected: auto-hide reflow (rubberband), hide-during-resize
mask (whole-video blackout — "infinitely worse"). The root fix is to stop having
two surfaces.

## Target architecture (C1)

```
        ┌─ main window (transparent, DWM per-pixel alpha) ─┐
        │  DComp visual tree:                              │
        │    [top]    WebView2  (transparent bg)           │  ← DOM / controls
        │    [below]  mpv video (D3D11 texture)            │  ← composited, not a HWND
        └──────────────────────────────────────────────────┘
```

- mpv uses the **render API** (`mpv_render_context`, OpenGL via ANGLE→D3D11, or
  software→texture) to render into a **D3D11 texture we own** instead of a HWND.
- That texture is the content of a **DirectComposition visual** placed **beneath**
  the WebView2, on the same window. The page is transparent where the video shows.
- Both are composited by DWM/DirectComposition → resize is one atomic operation.
  No `SetWindowPos` race, no bounce. The video pane in the DOM just needs a
  transparent hole; the compositor keeps the video glued to it.

## Stack facts (recon)

- tauri **2.11.5**, wry **0.55.1**, webview2-com **0.38.2**, `windows` **0.61.3**
  (already transitively present — add as direct dep with DComp/D3D11/DXGI feats).
- Window: `decorations:false`, not currently `transparent`.
- Tauri `WebviewWindow::with_webview(|w| …)` → on Windows exposes
  `w.controller()` (`ICoreWebView2Controller`) and the HWND. Enough to set the
  transparent background color; **not** enough to switch hosting mode.
- mpv wrapper is runtime FFI via `libloading` (`mpv/mod.rs`) — render-API symbols
  are just more symbols to load. Current child-window code is
  `create_child`/`move_child`/`show_child` in `player/mod.rs` (Windows cfg).

## THE gating unknown

wry hosts WebView2 **windowed** (child HWND), not **visual/composition** hosting.
Visual hosting (`CreateCoreWebView2CompositionController`) is the canonical way to
layer WebView2 with other DComp content — and wry doesn't do it.

**Q: will a DComp visual on the parent HWND show through a transparent windowed
WebView2?**
- If **yes** → C1 works entirely in our code. 🎉
- If **no** → we need visual hosting = patch/fork wry (`[patch.crates-io]`) or
  host our own WebView2 for the player window. Big scope jump → stop and re-decide.

We answer this with a **dumb solid-color layer** before any mpv work.

---

## Phased plan (each phase is a testable gate)

### Phase 1 — Compositing feasibility spike  ◐
Prove a composited layer can show through the WebView, no mpv involved.
- ☐ Add `windows` crate dep (Graphics_DirectComposition, Direct3D11, Dxgi,
  Direct2D/Foundation_Numerics as needed).
- ☐ `transparent: true` on the window (verify the opaque UI still looks normal;
  everything has solid bg except where we opt out).
- ☐ `compositor.rs`: D3D11 device → `DCompositionCreateDevice` → target for the
  main HWND (`DCompositionCreateTargetForHwnd`, topmost=false) → one visual with a
  solid-color surface → commit. Behind a `compositor_test` command.
- ☐ Frontend: make the watch video pane transparent (`bg-transparent`, punch a
  hole) and trigger the test.
- **GATE:** does the color show through the pane and stay glued during
  fullscreen/panel resizes? → decides in-code vs wry-fork.

### Phase 2 — mpv render context → texture  ☐
Independent of compositing; can develop in parallel once Phase 1 is green.
- ☐ Load render-API symbols in `mpv/mod.rs` (`mpv_render_context_create`,
  `_render`, `_set_update_callback`, `_free`, `_report_swap`).
- ☐ Stand up a GL context (ANGLE → D3D11 so the GL FBO is a shareable D3D
  texture) or software readback as a fallback.
- ☐ Render frames into a D3D11 texture; drive the render loop off mpv's update
  callback + a present cadence.

### Phase 3 — Wire texture → DComp visual  ☐
- ☐ Feed the mpv texture as the DComp visual content (swapchain or surface).
- ☐ Replace the child-HWND path: retire `create_child`/`move_child`; mpv no
  longer owns a window. Keep the old path behind a config flag until stable.
- ☐ Position/size the visual from the pane rect (still reported from JS, but now
  it only moves a compositor visual — cheap and can be made atomic).

### Phase 4 — Parity + cleanup  ☐
- ☐ Subtitles, letterbox bg, aspect, HDR/hwdec sanity, multi-monitor DPI.
- ☐ Resize is atomic → delete the reportRect delays, the fullscreen fill hacks,
  and revisit whether controls can now *overlay* the video (airspace gone).
- ☐ Linux path (separate: GL/EGL + a wayland/x11 subsurface or the same render
  API into a GtkGLArea) or keep `wid` embedding on Linux behind cfg.
- ☐ Perf: no extra copies, vsync, power use.

## Fallbacks (if Phase 1 gate fails)

1. **Patch wry for composition hosting** — `[patch.crates-io.wry]` → our fork that
   creates the WebView2 via `CreateCoreWebView2CompositionController` and hands us
   the visual to parent under. Known-good, but a fork to maintain.
2. **Two-window** — transparent WebView window + a video window behind, both moved
   from one Rust resize handler. Sidesteps in-window layering; adds two-window
   focus/z-order/sync fiddliness. Not really "one surface."
3. **Abandon C1, ship B1+A1** — Rust-driven `WindowEvent::Resized` reposition +
   instant (un-animated) panels. ~95% of the win for ~5% of the effort. The
   pragmatic retreat.

## Guardrails

- Keep the working `wid` player as the default until the new path reaches parity;
  gate the compositor behind a flag. Never leave `main` (or this branch's tip)
  with a broken player.
- Windows-first. Linux stays on `wid` behind cfg until Phase 4.
- Each phase must build green and be user-testable in isolation.
