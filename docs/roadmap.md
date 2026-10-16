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

## Order of work

1. **Keep the Windows build solid.** Real users find what testing doesn't:
   fix what they report first ([Open issues](#open-issues),
   [Windows polish](#windows-polish)).
2. **2.0.3** — fixes found by 2.0.2's QA run and #10 ([2.0.3](#203)).
3. **2.1** — drag & drop import, manual resource pairing, the PDF viewer
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
- [ ] **Course header at the minimum width (900px)**: "N resources done"
      runs into the total duration.
- [ ] **Index subtitle text on import** (today: Settings → Index subtitle
      text, by hand).
- [ ] **A single-instance lock**: two copies at once can lose a write (seen
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

- [ ] **Drag & drop import** — see [Feature ideas](#feature-ideas).
- [ ] **Manual resource pairing** — see [Feature ideas](#feature-ideas).
      A new table (schema v9); needs a short design pass on re-imports.
- [ ] **Simple built-in PDF viewer** — see [Feature ideas](#feature-ideas).
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

- Build and test on Ubuntu: `.github/workflows/slint-linux.yml` (CI only — it
  publishes nothing; releases stay Windows-only until Linux is reviewed).
- Run it: playback with `hwdec=auto-safe` (VA-API), on X11 and Wayland.
- **Resume needs mpv 0.38** (found by CI): a lecture with a saved position
  loads with `loadfile <url> replace 0 start=…`, and the index argument
  (`0`) is new in 0.38. Ubuntu 24.04 has 0.37, so resuming fails there
  (mpv error -4). Use the named-argument form, or set `start` as a property
  before `loadfile` (and reset it for loads without one). Then un-ignore
  `no_position_or_leaving_the_player_never_wipes_progress` on Linux.
- Keep-awake during playback (freedesktop ScreenSaver inhibit; macOS:
  IOPMAssertion) — Windows only today.
- rfd's backend: GTK 3 (default) or the XDG portal.
- Mini player: the portable path (Slint's position/size, `always-on-top`,
  the window system's own drag) should do on X11. Wayland lets an app neither
  place its window nor keep it on top, so there it's a small window wherever
  it lands. The Windows-only parts (work area, SetWindowPos, DWM cloaking,
  the app-driven drag, WM_DPICHANGED) are behind `cfg(windows)` in `mini.rs`.
- Packaging: AppImage and/or Flatpak; the updater's Linux entry in
  `latest.json`.

## Feature ideas

Priority (2.1): manual resource pairing, the PDF viewer, drag & drop import.
Later: the transcript panel (needs a lot of design). Held: study plan,
command palette, A–B loop.

- **Manual resource pairing** — attach a resource (a PDF, a link, a file) to
  a lecture by hand when the importer guessed wrong or the file has no
  numbering: pick a lecture from the resource's menu (or drag it onto one);
  stored per course, kept across re-imports, and "reset to automatic".
- **Simple built-in PDF viewer** (dcsm8) — read a lecture's PDFs inside
  Deskemy: pages, scroll, zoom, page number, "Open in your PDF app". No
  annotation or editing. Likely PDFium via `pdfium-render` (~5 MB DLL);
  needs a Linux check.
- **Transcript panel** (later: needs a lot of design) — the lecture's
  subtitles as a scrolling transcript: click a line to jump, the current line
  highlighted, search within the lecture. Subtitle files are already parsed
  and indexed.
- **Notes on bookmarks** — bookmarks are already a moment plus a short label;
  give them an optional longer note, show it in the Bookmarks page and the
  player's list, and export a course's bookmarks and notes as Markdown.
  (Covers #5's per-lecture space.)
- **Drag & drop / batch import** — drop a course folder on the window to import
  it; drop a folder of courses to import them all (the core already has
  library roots).
- **Study plan** (held) — pick a finish date for a course; it works out the daily
  minutes and shows "today: 2 lectures, 25 min" on the library, building on the
  daily goal and streaks.
- **A–B loop** — repeat a stretch of a lecture, optionally slower (mpv's
  `ab-loop-a` / `ab-loop-b`). On hold: rarely used — until someone asks.
- **Command palette** (held) — Ctrl+K to jump to any course or lecture, or run an
  action.
- [x] **Follow-along mini player** — shipped in 2.0.
