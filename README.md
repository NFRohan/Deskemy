<div align="center">
  <img src="app-icon.png" width="120" height="120" alt="Deskemy" />

  <h1>Deskemy</h1>

  <p><strong>An offline, Udemy-style player for the video courses you already own.</strong></p>

  <p>
    <img alt="Platform" src="https://img.shields.io/badge/platform-Windows%2010%20%2F%2011-0a7bbd" />
    <img alt="Version" src="https://img.shields.io/badge/version-1.0.0-8e10db" />
    <img alt="Built with" src="https://img.shields.io/badge/Tauri%20v2%20%C2%B7%20Svelte%205%20%C2%B7%20Rust-111317" />
    <img alt="Offline" src="https://img.shields.io/badge/offline-no%20account%20%C2%B7%20no%20telemetry-2e7d32" />
  </p>
</div>

---

Deskemy turns a folder of downloaded course videos into a real learning
library. Point it at a course, and it auto-detects the sections and lectures,
cleans up the messy filenames, attaches the subtitles and resource files, and
gives you a focused player built for *studying* — resume, chapters, bookmarks,
notes-adjacent tools, search, and progress tracking.

It's **local-first by design**: your video files are only ever *referenced* in
place — never copied, moved, or uploaded — and everything Deskemy knows lives in
a single SQLite database on your machine.

## Highlights

<table>
<tr>
<td width="50%" valign="top">

**Library that organizes itself**
- Import a folder → sections, ordered lectures, cleaned titles
- Sidecar subtitles and resources attached automatically
- A **dry-run preview** of what will be imported, before you commit
- Live progress while it scans (`Probing 42 / 210`)

**A player made for learning**
- Resume where you left off · autoplay-next · playback speed
- Chapters, subtitle **and** audio-track pickers
- Per-course playback preferences, remembered
- Fullscreen that behaves natively — even from a maximized window

</td>
<td width="50%" valign="top">

**Find and revisit anything**
- Full-text **search** over titles — and, optionally, the *spoken words*
  in your subtitles, jumping straight to the moment
- **Bookmarks** with timestamps · **tags** · favorites · **history**
- **Career Tracks** — ordered course paths with aggregate completion

**Stays out of your way**
- **Rename-safe**: move or rename a file and its progress/bookmarks follow
  it (matched by content, not path)
- Watch-time **stats** — heatmap, streaks, daily goal
- Optional folder auto-rescan; a Storage panel to reclaim space

</td>
</tr>
</table>

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

- **Windows 10 or 11.** The codebase is desktop-first and portable, but the
  packaged release targets Windows.
- **[mpv](https://mpv.io/)** — Deskemy plays through your own installed mpv
  (`libmpv-2.dll`) instead of bundling a media engine, so playback quality and
  format support match mpv exactly. Install it via [Scoop](https://scoop.sh/)
  (`scoop install mpv`) or from [mpv.io](https://mpv.io/); Deskemy finds it
  automatically and prompts you if it can't.
- The **WebView2** runtime — installed by the setup program if it's missing.

## Install

1. Download the latest installer (`.exe` or `.msi`) from the releases page.
2. Run it. If mpv isn't already installed, grab it (see above).
3. Launch Deskemy → **Add Folder** → pick a course folder.

The installer is a per-user install (no admin needed). Uninstalling from
**Settings → Apps** removes the program, its shortcuts, and its registry entry
cleanly. Your library index and settings under `%APPDATA%\com.spooksy.deskemy`
are kept on purpose, so a reinstall picks up right where you left off — delete
that folder yourself if you want a completely fresh start.

### Portable (no install)

Prefer to try it without installing anything? Download the **portable zip**,
extract it anywhere, and run `Deskemy.exe`. A `.portable` marker next to the
executable tells Deskemy to keep **everything** — library, settings,
thumbnails — in a `data/` folder right beside it, so nothing is written to
`%APPDATA%` or the registry. Delete the folder and it's gone without a trace.
(You still need mpv and the WebView2 runtime available on the system.)

> Moving to another PC? Copy the `%APPDATA%\com.spooksy.deskemy` folder (or the
> portable `data/` folder). Your course files are referenced by path, so keep
> them where they are — or re-point the library after moving.

## Build from source

```bash
# Prerequisites: Node 22+, Rust (stable), MSVC C++ Build Tools, WebView2
npm install
npm run tauri dev      # run in development
npm run tauri build    # produce an optimized installer in
                       # src-tauri/target/release/bundle/
```

## How it's built

| Layer | Stack |
|---|---|
| **Frontend** | SvelteKit (`adapter-static` SPA) · Svelte 5 runes · TypeScript · Tailwind v4 |
| **Backend** | Rust · Tauri v2 · bundled SQLite + FTS5 (`rusqlite`) · a filesystem scanner/importer and a watcher |
| **Playback** | **libmpv** loaded at runtime via FFI (`libloading`) — not bundled; discovered from your system |
| **Identity** | UUIDv7 keys throughout, so data stays sync-safe if multi-device is ever added |

The import pipeline probes media **off the database lock** (two-phase import), so
a large course scans without freezing the UI, and re-imports preserve your
progress, bookmarks, tags, and thumbnails.

## Privacy

There are no accounts, no telemetry, and no network calls for your content.
Your library, progress, bookmarks, and stats exist only in a local database —
the app works fully offline.

## License

Released under the **MIT License**.
