# Roadmap

Where Deskemy goes after the Slint rebuild. The port's own history and
checklist live in [slint-port.md](slint-port.md).

## Where things stand

- **2.0.0** (2026-09-29) — the Slint rebuild: no WebView2, mini player,
  resources with their lectures, signed self-updating setup / MSI / portable.
- **2.0.1** (2026-10-05) — the library database is created on a fresh install
  (2.0.0 never wrote one there); mini player across monitors of different
  scaling (#8), no Snap while dragging it, and leaving it on another monitor.
- **2.0.2** (2026-10-10) — progress always kept, the main window across
  mixed-DPI monitors (#8), Snap Layouts, resources in lecture order, mouse
  back anywhere in the player, chapters kept (#6). First release run through
  [release-qa.md](release-qa.md); Windows 11 checks still to do.
- **2.1.0** (2026-10-21) — the PDF viewer, drag & drop import (Add Folder
  tells a folder of courses apart), manual resource pairing, the subtitle
  language kept by language (#11), lectures kept on their last frame at the
  end, scroll kept on the course page and library; with the 2.0.3 fixes
  (#10, long paths, single copy, subtitles indexed on import).
- **2.1.1** (2026-10-24) — Windows: resource popup timing (a corner card
  before a lecture ends), bookmarks on the seek bar, search grouped by kind
  and as you type, sections in the player's title bar, exact skips and a
  seek bar that glides. Built on the linux-port branch; Linux itself is held
  for more testing.

## Order of work

1. **Keep the Windows build solid.** Real users find what testing doesn't:
   fix what they report first ([Open issues](#open-issues),
   [Windows polish](#windows-polish)).
2. ~~**2.0.3 fixes**~~ — found by 2.0.2's QA run and #10 ([2.0.3](#203));
   released with 2.1.0 rather than on their own.
3. ~~**2.1**~~ — released as 2.1.0: drag & drop import, manual resource pairing, the PDF viewer
   ([2.1](#21)). Versions follow semver: fixes and small touches are patch
   releases; new features are the next minor.
4. ~~Retire the Tauri app~~ — done: removed, 1.x docs in `docs/archive/`.
5. **Linux** (GH #7) — the reason for the port. Then macOS.
6. **Bigger features** ([Feature ideas](#feature-ideas)).

## 2.0.3

- [x] **Add Folder does nothing when the folder dialog can't open (#10)** —
      rfd returns `None` for both "cancelled" and "never opened". Log every
      `None`; on Linux, check the XDG desktop portal is reachable first and
      show an error when it isn't. Same for the other pickers (locate folder,
      cover, backup export / import).
- [x] **Paths over 260 characters** can't be opened (flagged unplayable on
      import; found by the QA course in a deep folder). Long-path-aware
      paths for mpv (`\\?\` prefix) and the exe manifest's `longPathAware`.
- [x] **Course header at the minimum width (900px)**: "N resources done"
      runs into the total duration.
- [x] **Index subtitle text on import** (today: Settings → Index subtitle
      text, by hand).
- [x] **A single-instance lock**: two copies at once can lose a write (seen
      with overlapping snapshot imports). A second launch brings the first
      window forward instead.

## 2.0.2 (released)

- [x] **Main window across monitors of different scaling (#8)** — the mini
      player's WM_DPICHANGED fix (Windows' suggested rectangle) for the full
      window, while it's dragged.
- [x] **Snap Layouts** on the drawn maximize button (Windows 11).
- [x] **Mini player: where the full window comes back** — a setting: "When
      leaving the mini player: open on the monitor it's on (2.0.1's
      behaviour) / go back to where the window was". Both rectangles already
      exist in `mini.rs` (the saved one, and `carry_over`'s); the setting picks.
- [x] **Chapters button (#6)** — hardened (an empty read keeps the file's
      list; logged); reported on 1.2.2: the button vanishes after
      a few minutes of a chaptered MP4. Check whether 2.0 does the same (the
      menus rebuild from mpv's track list; a transient empty chapter list
      would hide it), and keep the last non-empty list for the same file.

## 2.1

In this order: the smallest first; pairing before the viewer, which opens
what's paired with the lecture.

- [x] **Drag & drop import** — drop a course folder, a folder of courses or
      several on the library. One course goes to the usual preview; several to
      a list to tick, imported one after another (hide / stop). Add Folder
      tells a folder of courses apart the same way. Each course is watched on
      its own: a dropped folder of courses isn't kept as a library root (no
      UI to list or remove roots yet), so new courses added to it later aren't
      picked up. No drops on Wayland (winit has none there).
- [x] **Manual resource pairing** — a resource row's link button (course
      page) pairs it with any lecture of the course, with no lecture, or back
      to automatic. Schema v9: `resource_pairings`, by file path, so it
      survives re-imports, auto-rescan and Locate folder; a pairing whose
      resource or lecture is gone is dropped at the next re-import (and
      ignored until then). It wins over the importer's guess and the "keep
      together" number placement everywhere: course page, the player's
      Resources panel and the end-of-lecture pause. Not yet: pairing from the
      player, dragging a resource onto a lecture.
- [x] **Simple built-in PDF viewer** — a resource that's a PDF opens in
      Deskemy (course page, the player's Resources panel and the
      end-of-lecture card): pages, scroll, zoom, page n of m, keys, "Open in
      your PDF app". Over the player it pauses first (and leaves fullscreen
      or the mini player). PDFium (`pdfium.dll`, build 7881, the non-V8 one:
      no JavaScript), via `pdfium-render`, on a thread of its own; only pages
      near the view are rendered, at their shown width. Without the DLL,
      PDFs open outside as before. Linux: needs packaging (`libpdfium.so`
      beside the binary); until then PDFs open outside there.
- [ ] **Resources prompt timing** (dcsm8) — to discuss once the above are
      done. With "Pause on exercises and
      resources", a heads-up that the lecture has resources some time before
      its end, besides the pause at the end. Needs design first: what the
      heads-up looks like (a chip? the card early?), when it's too early to be
      useful, and how it behaves in the mini player and when seeking past it.

## Open issues

- **#5 Additional features** — per-lecture space for your own resources /
  links ("like a classroom app"); AI summaries per video (NotebookLM-style).
  The first overlaps [Timestamped notes](#feature-ideas); the second needs
  thought (offline-first, no accounts) — reply before committing to either.
- **#6 Chapters button disappears** — hardened in [2.0.2](#202). Waiting on the reporter
  for details; 1.2.2 couldn't be reproduced.
- **#7 Linux** — see [Linux](#linux).
- **#8 Mini player across monitors** — the mini player fixed in 2.0.1
  (confirmed); the main window in 2.0.2. The reporter will say when they've
  tried it; close then.
- **#10 Add Folder silent when the dialog can't open** (Linux) — see
  [2.0.3](#203).
- **#11 Subtitle language not kept from lecture to lecture** — fixed for
  2.1: the choice was saved as mpv's track number, which differs per file
  (embedded tracks, then each lecture's own sidecars), so the next lecture
  showed another language, or none where that number didn't exist. Now
  it's saved by language (and title), and found in each file's tracks; a
  lecture without it shows none. 2.0's saved numbers turn into a language
  the first time the course plays. Schema v10. Reply on the issue when 2.1
  is out. (Audio tracks are still remembered by number: courses rarely have
  more than one.)

## Windows polish

- [x] The first run after installing came up behind other windows (not
      minimized: the log showed visible, `show=1`, not foreground, launched
      by the already-exited setup — Windows' foreground lock). It's brought
      forward once at startup unless Explorer launched it; logged.
- [x] Mouse back works anywhere in the player window (a window-level
      filter, not per surface).
- [x] Numbered section resources sit in lecture order with "keep videos and
      resources together": `003 Configuring Git.html` between `002` and `004`,
      Udemy's per-lecture `16.1 …` with lecture 16.
- [x] Track status icons name the status on hover.
- [x] Real-window checks: Add Folder, auto-rescan, track dialogs, Settings
      maintenance buttons, backup export / import (hand-tested on 2.0.2).
- [x] A real 1.2.2 MSI → 2.x MSI upgrade (hand-tested).
- Dropped: offering to re-import courses added before the 2.0 scanner fixes
  (Add Folder on the same folder already re-imports and keeps progress);
  the work-in-progress logos (a designer will make one).
- The rest moved to [2.0.3](#203).

## Retire the Tauri app

- [x] Removed `src-tauri/`, `src/` (Svelte), `static/` and the Node tooling;
      libmpv is staged from `app/vendor/` only.
- [x] The core crate dropped the queries only the Tauri app used (their
      columns stay, for older libraries and backups).
- [x] README / docs updated; 1.x planning docs in `docs/archive/`.

## Linux

The plan, bottlenecks and issues: [linux-port.md](linux-port.md) (work on
the `linux-port` branch).

On the branch, waiting on the Fedora VM pass and a real machine:

- [x] Build and test on Ubuntu 24.04 in CI (`.github/workflows/slint-linux.yml`),
  real videos included; the AppImage built there as an artifact.
- [x] Seeks land, and the seek bar glides and holds a seek's target (shared
  with Windows).
- [x] **Resume on mpv before 0.38** (found by CI): the index argument
  `loadfile` gained in 0.38 made 0.37 (Ubuntu 24.04) refuse a resume. On
  that refusal the older form is used from then on.
- [x] Keep-awake during playback (org.freedesktop.ScreenSaver).
- [x] X11 (XWayland) by default, for the mini player (on top, placed) and
  drag & drop; `DESKEMY_WAYLAND=1` for native Wayland. The Windows-only
  mini player parts (work area, SetWindowPos, DWM cloaking, the app-driven
  drag, WM_DPICHANGED) stay behind `cfg(windows)` in `mini.rs`.
- [x] A second launch brings the running window forward.
- [x] PDFium (`libpdfium.so`) for the PDF viewer.
- [x] The AppImage (libmpv, codecs and PDFium bundled), signed by
  `package.ps1` with the release key, updating itself.
- [ ] Run it: playback with `hwdec=auto-safe` (VA-API), on X11 and Wayland,
  on a VM and a real machine (NVIDIA too). See linux-port.md's VM pass.
- [ ] Flatpak, after the AppImage.

File dialogs use rfd's default backend, the XDG desktop portal (no GTK).

## Feature ideas

Priority (2.1): manual resource pairing, the PDF viewer, drag & drop import.
Later: the transcript panel (needs a lot of design). Held: study plan,
command palette, A–B loop.

- [x] **Manual resource pairing** — in [2.1](#21). Later, maybe: from the
  player's Resources panel too, and by dragging a resource onto a lecture.
- [x] **Simple built-in PDF viewer** (dcsm8) — in [2.1](#21). Later, maybe:
  find in the PDF, text selection, remembering the page per PDF.
- **Transcript panel** (later: needs a lot of design) — the lecture's
  subtitles as a scrolling transcript: click a line to jump, the current line
  highlighted, search within the lecture. Subtitle files are already parsed
  and indexed.
- **Notes on bookmarks** — bookmarks are already a moment plus a short label;
  give them an optional longer note, show it in the Bookmarks page and the
  player's list, and export a course's bookmarks and notes as Markdown.
  (Covers #5's per-lecture space.)
- [x] **Drag & drop / batch import** — in [2.1](#21). Later, maybe: keep a
  dropped folder of courses as a library root, so courses added to it import
  themselves (needs a place in Settings to list and remove roots).
- **Study plan** (held) — pick a finish date for a course; it works out the daily
  minutes and shows "today: 2 lectures, 25 min" on the library, building on the
  daily goal and streaks.
- **A–B loop** — repeat a stretch of a lecture, optionally slower (mpv's
  `ab-loop-a` / `ab-loop-b`). On hold: rarely used — until someone asks.
- **Command palette** (held) — Ctrl+K to jump to any course or lecture, or run an
  action.
- [x] **Follow-along mini player** — shipped in 2.0.
