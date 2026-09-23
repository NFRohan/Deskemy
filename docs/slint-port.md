# Slint port — cross-platform Deskemy

**Branch:** `slint-port` · **Status:** Phase 1 (video spike) in progress

## Why

Drawing UI over mpv video in the Tauri app takes a Windows-only
DirectComposition compositor (or a child HWND), plus JS reporting the video
rect on every layout change. That is the blocker for Linux (GH #7) and macOS.

In Slint, mpv's OpenGL render API draws straight into a texture in Slint's own
GL context, shown by an ordinary `Image` element — so overlays are just
elements declared after it, on every platform, with no rect plumbing. Chosen
over Iced (no accessibility, no foreign-texture API, yearly breaking changes),
Flutter (Dart rewrite + FFI bridge) and DOM `<video>` (only covers
browser-playable codecs; users' libraries aren't all H.264).

## Layout

| Path | What |
|---|---|
| `crates/deskemy-core/` | Platform- and UI-agnostic core: db, import, config, backup, libmpv FFI. Shared by both apps. |
| `app/` | The Slint app (`deskemy-app`, binary `deskemy`). |
| `src-tauri/` | The shipping Tauri app, unchanged apart from depending on the core. Retired at the end. |

Not a Cargo workspace on purpose: that would move `src-tauri/target` and break
its `build.rs` and release flow. Each app has its own lockfile and target dir.

## Running (from `app/`)

```sh
cargo run                                   # the app
cargo run -- --play "<video file>"          # straight into playback (spike)
cargo run -- --snapshot out.png             # render one frame offscreen, no window
cargo run -- --snapshot-player out.png      # same, player overlay with sample state
```

- libmpv: `build.rs` copies `libmpv-2.dll` next to the exe from `app/vendor/`
  or `src-tauri/vendor/` (both gitignored).
- `DESKEMY_HWDEC=no` forces software decoding (default `auto-safe`).
- `DESKEMY_DATA_DIR=<dir>` points at a scratch library instead of the real one.
- `--snapshot` uses Slint's software renderer, so OpenGL content (video) is
  blank in it — use it for layout checks only.

## Phase 1 — video spike: does the approach hold?

- ☑ Extract the core into `crates/deskemy-core`
- ☑ Slint skeleton reading the library through the core; `--snapshot`
- ☑ Bind mpv's OpenGL render API (`MpvRenderContext::new_gl` / `render_gl`)
- ☑ Video surface: mpv → GL texture → `Image`, status bar drawn on top
- ☐ **Run it** — picture correct (not flipped, not stretched, letterboxed),
  overlay drawn over video, resizing works, closing exits cleanly
- ☐ Measure CPU/GPU at 1080p: `DESKEMY_HWDEC=no` vs default, and vs the Tauri
  app. Note which decoder `auto-safe` picks (shown in the overlay).
- ☐ Watch for Slint #12030 (rendering notifier forcing FemtoVG pipeline
  rebuilds every frame) — GPU/CPU cost with an idle overlay
- ☐ Text quality at 100% / 150% scaling, FemtoVG vs `renderer-skia-opengl`;
  pick the renderer

## Phase 2 — player parity

Port `src-tauri/src/player/mod.rs` behaviour onto the new surface.

- ☑ Plezy-style overlay (`ui/player.slint`): title / section / wall clock,
  thin seek bar with elapsed and −remaining, transport cluster, "Ends at",
  volume, fullscreen; auto-hides after 2.5 s idle (not while paused or
  dragging); click toggles pause, double-click toggles fullscreen
- ☐ Keyboard shortcuts (the Tauri app's full map)
- ☐ Lecture-aware playback: open by lecture id, progress saving + resume +
  completion (95%), playlist / previous / next / autoplay
- ☐ Speed, subtitles, audio tracks, chapters (buttons are placeholders),
  per-course prefs
- ☐ Bookmarks, keep-awake, stats overlay toggle

## Later phases

3. App shell: custom window chrome (`no-frame` + `WindowMoveArea`), sidebar, navigation.
4. Pages: library, course, search, history, bookmarks, favorites, stats, tracks, settings.
5. Import flow (preview + progress) and the filesystem watcher (callback instead of `tauri::Emitter`).
6. Platforms: Linux (EGL; pass the X11/Wayland display to mpv for vaapi), macOS (Apple's deprecated GL).
7. Packaging + updater; retire `src-tauri/`.

## Known risks

- Slint #12030: rendering notifier + FemtoVG pipeline churn (reported on the wgpu variant).
- FemtoVG text quality is "sometimes sub-optimal" per Slint; Skia is better but heavy to build on Windows.
- macOS OpenGL is deprecated; nobody has shipped mpv + Slint there yet.
- Accessibility gaps in virtualized `ListView`s — prefer plain `for` loops (our lists are small).
