<div align="center">
  <img src="app-icon.png" width="120" height="120" alt="Deskemy" />

  <h1>Deskemy</h1>

  <p><strong>An offline player for the video courses you already own.</strong></p>

  <p>
    <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0a7bbd" />
    <a href="https://github.com/NFRohan/Deskemy/releases"><img alt="Latest release" src="https://img.shields.io/github/v/release/NFRohan/Deskemy?color=8e10db&label=release" /></a>
    <a href="https://github.com/NFRohan/Deskemy/stargazers"><img alt="Stars" src="https://img.shields.io/github/stars/NFRohan/Deskemy?color=8e10db&label=stars" /></a>
    <img alt="License" src="https://img.shields.io/badge/license-MIT-2e7d32" />
    <img alt="Built with" src="https://img.shields.io/badge/Tauri%20v2%20%C2%B7%20Svelte%205%20%C2%B7%20Rust-111317" />
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
- Attaches Subtitles and resource files (PDFs, code, archives) to the
  lecture or section they belong to.
- Shows a preview of what will be imported — sections, lectures, resources,
  subtitles, total runtime — with live progress during the scan.
- References files in place; it never copies or moves your videos.

**Playback**
- Resume from your last position, autoplay-next, adjustable speed, and per-course
  preferences (speed, subtitle, audio track) that are remembered.
- Chapter navigation, and subtitle / audio-track selection.
- Extensive Youtube-style keyboard shortcuts.

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
- Watch-time stats: an activity heatmap, streaks, and a daily goal.
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
| `F` / `Esc` | Fullscreen / exit | | `?` | Show all shortcuts |

## Requirements

- **Windows 10 or 11.**
- The **WebView2** runtime — installed by the setup program if it's missing, and
  already present on most Windows 10/11 systems.

Everything else is bundled. Deskemy plays through **libmpv** (mpv's media
library, `libmpv-2.dll`), which ships inside both the installer and the portable
zip — no separate mpv install needed. If you'd rather use your own build, Deskemy
also picks up `libmpv-2.dll` from your `PATH` or from `DESKEMY_LIBMPV`.

## Install

1. Download the latest installer (`.exe` or `.msi`) from the releases page.
2. Run it.
3. Launch Deskemy → **Add Folder** → pick a course folder.

It's a per-user install (no admin required). Uninstalling from **Settings → Apps**
removes the program, its shortcuts, and its registry entry. Your library index
and settings under `%APPDATA%\com.spooksy.deskemy` are left in place so a
reinstall resumes where you left off — delete that folder for a clean slate.

### Portable (no install)

To run without installing, download the **portable zip**, extract it, and run
`Deskemy.exe`. A `.portable` marker beside the executable keeps all data (library,
settings, thumbnails) in a `data/` folder next to it, so nothing is written to
`%APPDATA%` or the registry. Delete the folder to remove it entirely. (The
WebView2 runtime still needs to be present on the system, as it is by default.)

## Community Translations

Maintained by the community as separate projects, so they may trail the latest
release. For translation-specific issues, please open them on that project's repo.

- **简体中文 (Simplified Chinese)** — [Deskemy-zh-CN](https://github.com/flipped0419/Deskemy-zh-CN) by [@flipped0419](https://github.com/flipped0419)

## Build from source

```bash
# Prerequisites: Node 22+, Rust (stable), MSVC C++ Build Tools, WebView2
npm install
npm run tauri dev      # run in development
npm run tauri build    # build an installer in src-tauri/target/release/bundle/
```

## Tech stack

| Layer | |
|---|---|
| **Frontend** | SvelteKit (`adapter-static` SPA) · Svelte 5 runes · TypeScript · Tailwind v4 |
| **Backend** | Rust · Tauri v2 · SQLite + FTS5 via `rusqlite` (bundled) |
| **Playback** | libmpv, loaded at runtime through FFI (`libloading`) — bundled with the app, with system/`DESKEMY_LIBMPV` fallback |
| **Storage** | Local SQLite database + a content-addressed thumbnail cache under the app data directory |

Import runs in two phases — probe, then persist — so media probing happens off
the database connection and scanning a large course doesn't block the UI.

**How it fits together**

```mermaid
flowchart TB
    subgraph win["Deskemy window · Tauri v2"]
        ui["SvelteKit UI<br/>Svelte 5 · transparent WebView2"]
        subgraph core["Rust core"]
            cmd["Commands + events"]
            imp["Two-phase importer<br/>scan → probe → persist"]
            ply["Player control (FFI)"]
        end
        dcomp["DirectComposition<br/>video surface"]
    end

    mpv["libmpv-2.dll<br/>bundled"]
    db[("SQLite + FTS5<br/>rusqlite, bundled")]
    thumb[("Thumbnail cache")]
    files[/"Your course folders<br/>referenced in place"/]

    ui <-->|IPC| cmd
    cmd --> imp
    cmd --> ply
    cmd --> db
    cmd --> thumb
    imp -->|probe| mpv
    imp --> files
    imp --> db
    ply --> mpv
    ply --> db
    mpv -->|renders| dcomp
    dcomp -.->|shows through| ui
```

## Privacy

No accounts, no telemetry, and no network requests for your content. Your
library, progress, bookmarks, and stats live only in a local database; the app
works fully offline.

## License

[MIT](LICENSE) © 2026 Nayeem Fardin.
