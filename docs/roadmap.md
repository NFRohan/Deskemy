# Roadmap

What's next after the Slint port reached parity with the Tauri app. The port's
own history and checklist live in [slint-port.md](slint-port.md).

## Order of work

1. **Perfect the Windows rebuild first.** A new frontend surfaces bugs users
   would otherwise find for us: keep testing real use (real libraries, odd
   course folders, long sessions) and fixing what turns up before anything
   else. Open items are in [Windows polish](#windows-polish).
2. **Installer and updater, then retire `src-tauri/`.** Users are still on the
   Tauri build; they move over once the Slint one installs and updates itself.
3. **Linux** (GH #7) — the reason for the port. Then macOS.
4. **New features**, from the [ideas](#feature-ideas) below.

## Windows polish

Known follow-ups:

- [ ] Real-window checks not yet done: Add Folder (native picker, progress),
      auto-rescan (drop a video into a course folder), track dialogs,
      Settings maintenance buttons, backup export / import with its restart.
- [ ] Numbered article pages (Udemy's `003 Configuring Git.html`) sit at the end
      of their section; in "keep videos and resources together" they could sit
      in lecture order, between the lectures around their number.
- [ ] Courses imported before the scanner fixes (TypeScript `.ts` files counted
      as videos, code projects listed file by file) need a re-import to pick
      them up.
- [ ] Mouse back only works over the video, not over the control bar or title
      bar.
- [x] Controls hide when the pointer leaves the window (paused or playing).

## Installer and updater

- [x] Installer (cargo-packager, NSIS, matching the Tauri install so it updates
      in place) and portable zip — `app/scripts/package.ps1`, see
      [releasing.md](releasing.md).
- [x] In-app updates against the signed `latest.json` (the Tauri updater's, plus
      `format`), banner + Settings → About, nothing downloads until confirmed.
- [ ] Hand-test 2.0.0 for a couple of days, then release (the user's call).
- [ ] Rehearse a real update with the release key (releasing.md §3–4).
- [ ] Then remove `src-tauri/` and the Svelte frontend.

## Linux

- Push the branch so `.github/workflows/slint-linux.yml` builds and tests on
  Ubuntu.
- Run it: playback with `hwdec=auto-safe` (VA-API), on X11 and Wayland.
- Keep-awake during playback (freedesktop ScreenSaver inhibit; macOS:
  IOPMAssertion) — Windows only today.
- rfd's backend: GTK 3 (default) or the XDG portal.
- Packaging: AppImage and/or Flatpak.

## Feature ideas

Roughly by value to someone working through a course:

- **Transcript panel** — the lecture's subtitles as a scrolling transcript:
  click a line to jump, the current line highlighted, search within the
  lecture. Subtitle files are already parsed and indexed.
- **Follow-along mini player** — a small always-on-top window beside an editor
  or terminal for coding courses: play/pause, ±10s, next.
- **Timestamped notes** — bookmarks with a body: notes per lecture, jump to the
  moment, export a course's notes as Markdown.
- **Drag & drop / batch import** — drop a course folder on the window to import
  it; drop a folder of courses to import them all (the core already has
  library roots).
- **Study plan** — pick a finish date for a course; it works out the daily
  minutes and shows "today: 2 lectures, 25 min" on the library, building on the
  daily goal and streaks.
- **A–B loop** — repeat a stretch of a lecture, optionally slower (mpv's
  `ab-loop-a` / `ab-loop-b`).
- **Command palette** — Ctrl+K to jump to any course or lecture, or run an
  action.
