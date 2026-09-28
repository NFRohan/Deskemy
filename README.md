<div align="center">
  <img src="app-icon.png" width="120" height="120" alt="Deskemy" />

  <h1>Deskemy</h1>

  <p><strong>An offline player for the video courses you already own.</strong></p>

  <p>
    <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0a7bbd" />
    <a href="https://github.com/NFRohan/Deskemy/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/NFRohan/Deskemy?color=8e10db&label=release" /></a>
    <a href="https://github.com/NFRohan/Deskemy/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/NFRohan/Deskemy?color=8e10db&label=stars" /></a>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-2e7d32" />
    <img alt="Built with" src="https://img.shields.io/badge/Slint%20%C2%B7%20Rust%20%C2%B7%20libmpv-111317" />
  </p>

  <p>
    <a href="https://github.com/NFRohan/Deskemy/releases/latest"><img alt="Download for Windows" src="https://img.shields.io/badge/Download%20for%20Windows-8e10db?style=for-the-badge&logo=windows&logoColor=white" /></a>
  </p>
</div>

---

Deskemy is a local, offline player for downloaded video courses. Point it at a
folder of course videos and it organizes them into a browsable library with
playback, progress tracking, search, and study tools. Your video files stay
where they are — they're referenced in place, never copied or uploaded — and
everything Deskemy records lives in a single local SQLite database.

<div align="center">
  <img src="Images/Deskemy%20Home.png" alt="Deskemy library — Continue Watching and the course grid" width="100%" />
</div>

<details>
<summary><b>More screenshots</b> — course view, player, fullscreen, shortcuts, search, history</summary>
<br/>
<table>
  <tr>
    <td><img src="Images/Deskemy%20Course%20View.png" alt="Course view" /></td>
    <td><img src="Images/Deskemy%20Player%20Windowed.png" alt="Player (windowed)" /></td>
  </tr>
  <tr>
    <td><img src="Images/Deskemy%20player%20Fullscreen.png" alt="Fullscreen player" /></td>
    <td><img src="Images/Deskemy%20Keyboard%20shortcut%20Cheatsheet.png" alt="Keyboard shortcuts cheat sheet" /></td>
  </tr>
  <tr>
    <td><img src="Images/Deskemy%20Search.png" alt="Search" /></td>
    <td><img src="Images/Deskemy%20history.png" alt="Watch history" /></td>
  </tr>
</table>
</details>

## Features

**Library & import**
- Structures a course folder into sections and ordered lectures, cleaning
  numeric prefixes and extensions out of the titles.
- Attaches subtitles and resource files (PDFs, code, archives) to the
  lecture or section they belong to. Code projects (a folder with
  `package.json`, `node_modules` and the like) come in as one resource, and
  TypeScript `.ts` files aren't mistaken for videos.
- Shows a preview of what will be imported — sections, lectures, resources,
  subtitles, total runtime — with live progress during the scan.
- References files in place; it never copies or moves your videos. Adding a
  course's folder again updates it and keeps your progress.

**Playback**
- Resume from your last position and autoplay-next. A default speed for every
  course, or a course's own; subtitle and audio-track choices are remembered
  per course.
- **Mini player**: press `T` to shrink the player to a small always-on-top
  window beside your editor or terminal — it keeps the video's shape and opens
  where you left it.
- Resources next to their lectures, in the curriculum and the player's course
  panel, ticked off as you go; optionally, autoplay stops at a lecture with
  exercises to offer them.
- Chapter navigation, a sleep timer, and subtitle / audio-track selection.
- Extensive YouTube-style keyboard shortcuts.

<div align="center">
  <img src="Images/Deskemy%20Player%20with%20panels.png" alt="The player with the course-contents panel open" width="90%" />
</div>

**Organize & revisit**
- Continue Watching on the home screen, resuming the exact lecture you were on.
- Timestamped bookmarks, tags, favorites, and a watch history.
- Career Tracks — ordered groups of courses with aggregate completion.

**Search**
- Full-text search across course, section, and lecture titles.
- Optional subtitle search over the words spoken in your subtitle files, jumping
  straight to the matching timestamp.

**Progress & maintenance**
- Watch-time stats: an activity heatmap (hover a day for what you watched),
  streaks, and a daily goal.
- Rename-safe: a moved or renamed file keeps its progress and bookmarks, matched
  by content rather than path.
- Optional folder auto-rescan, and a storage panel for reclaiming disk space.

<div align="center">
  <img src="Images/Deskemy%20Stats.png" alt="The stats dashboard — streaks, watch-time, and an activity heatmap" width="90%" />
</div>

## Keyboard shortcuts

| Key | Action | | Key | Action |
|---|---|---|---|---|
| `Space` / `K` | Play / pause | | `C` | Toggle subtitles |
| `J` / `L` | Skip back / forward 10s | | `,` / `.` | Slower / faster |
| `←` / `→` | Skip back / forward 5s | | `N` / `⇧N` | Next / previous lecture |
| `↑` / `↓` | Volume up / down | | `P` / `R` | Course contents / Resources |
| `M` | Mute | | `B` | Bookmark this moment |
| `F` / `Esc` | Fullscreen / exit | | `T` | Mini player, always on top |
| `I` | Playback info | | `?` | Show all shortcuts |

## Requirements

- **Windows 10 or 11**, with a GPU that supports OpenGL 3.3 (any integrated GPU
  from the last decade). No WebView2 needed.
- Linux is in progress (the app is built and tested on Linux in CI, but not
  released yet); macOS after that.

Everything is bundled. Deskemy plays through **libmpv** (mpv's media
library, `libmpv-2.dll`), which ships inside the installers and the portable
zip — no separate mpv install needed. If you'd rather use your own build, Deskemy
also picks up `libmpv-2.dll` from your `PATH` or from `DESKEMY_LIBMPV`.

## Install

1. Download the latest installer (`deskemy_<version>_x64-setup.exe`) from the releases page.
2. Run it.
3. Launch Deskemy → **Add Folder** → pick a course folder.

It's a per-user install (no admin required). For a per-machine install in
Program Files (admin, for all users), use the MSI (`deskemy_<version>_x64_en-US.msi`)
instead. Either way it keeps itself up to date: when a new release is out,
Deskemy offers it (nothing downloads until you click **Update**), and
**Settings → About** has a Check button. Coming from Deskemy 1.x, the update
installs over it and keeps your library. Uninstalling from **Settings → Apps**
removes the program, its shortcuts, and its registry entry. Your library index
and settings under `%APPDATA%\com.spooksy.deskemy` are left in place so a
reinstall resumes where you left off — delete that folder for a clean slate.

### Portable (no install)

To run without installing, download the **portable zip**, extract it, and run
`deskemy.exe`. A `.portable` marker beside the executable keeps all data (library,
settings, thumbnails) in a `data/` folder next to it, so nothing is written to
`%APPDATA%` or the registry. Delete the folder to remove it entirely. (A portable
copy can't update itself; Deskemy points you at the release page instead.)

## Community Translations

Maintained by the community as separate projects, so they may trail the latest
release. For translation-specific issues, please open them on that project's repo.

- **简体中文 (Simplified Chinese)** — [Deskemy-zh-CN](https://github.com/flipped0419/Deskemy-zh-CN) by [@flipped0419](https://github.com/flipped0419)

## Build from source

```bash
# Prerequisites: Rust (stable), MSVC C++ Build Tools, and libmpv-2.dll in
# app/vendor/ (the "libmpv" build of mpv; the build copies it beside the exe)
cd app
cargo run              # run in development
cargo test             # tests (a headless mpv runs the playback ones)
```

Releases are packaged with cargo-packager — see [docs/releasing.md](docs/releasing.md).
The previous Tauri + Svelte app still lives in `src-tauri/` and `src/` until it's
retired.

## Tech stack

| Layer | |
|---|---|
| **UI** | [Slint](https://slint.dev) (Rust) · Skia renderer, patched for ClearType-style text |
| **Core** | Rust (`crates/deskemy-core`) · SQLite + FTS5 via `rusqlite` (bundled) |
| **Playback** | libmpv, loaded at runtime through FFI (`libloading`), rendering straight into the UI's OpenGL context — bundled with the app, with system/`DESKEMY_LIBMPV` fallback |
| **Storage** | Local SQLite database + a content-addressed thumbnail cache under the app data directory |

Import runs in two phases — probe, then persist — so media probing happens off
the database connection and scanning a large course doesn't block the UI.

**How it fits together**

```mermaid
flowchart TB
    subgraph win["Deskemy window · Slint"]
        ui["Slint UI<br/>Skia over OpenGL"]
        subgraph core["Rust core"]
            cmd["Pages + session"]
            imp["Two-phase importer<br/>scan → probe → persist"]
            ply["Player control (FFI)"]
        end
    end

    mpv["libmpv-2.dll<br/>bundled"]
    db[("SQLite + FTS5<br/>rusqlite, bundled")]
    thumb[("Thumbnail cache")]
    files[/"Your course folders<br/>referenced in place"/]

    ui <-->|callbacks| cmd
    cmd --> imp
    cmd --> ply
    cmd --> db
    cmd --> thumb
    imp -->|probe| mpv
    imp --> files
    imp --> db
    ply --> mpv
    ply --> db
    mpv -->|renders frames into the UI's GL context| ui
```

## Privacy

No accounts, no telemetry, and no network requests for your content. Your
library, progress, bookmarks, and stats live only in a local database; the app
works fully offline. The one request it makes is the update check: it reads
the latest release's manifest from GitHub, and downloads nothing unless you
choose to update.

## License

[MIT](LICENSE) © 2026 Nayeem Fardin.
