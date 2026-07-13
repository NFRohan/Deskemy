# Deskemy — Offline Course Player

A **desktop-first** local app (Windows primary, Linux supported) that gives a
Udemy-style experience for downloaded courses: import folders of courses,
auto-structure them from the folder tree, and play them offline with resume,
chapters, subtitles, bookmarks, tags, full-text + subtitle search, and a stats
dashboard.

> **Status: v1 baseline complete.** All milestones **M0–M8** shipped, plus a
> round of post-v1 features (resources, tags, subtitle search, watch-time stats,
> auto-rescan) and a full pre-release code review with fixes. This document is
> the **current baseline** — what exists today — ahead of the next expansion.
> Schema is at **version 5**.

---

## Stack (as built)

- **Frontend:** SvelteKit + `adapter-static` (SPA, `ssr=false`), **Svelte 5
  runes**, TypeScript, **Tailwind v4** (`@theme` tokens, dark + light + system),
  `@lucide/svelte`, Inter (`@fontsource-variable/inter`).
- **Backend:** Rust — `rusqlite` (bundled SQLite + FTS5), scanner + importer,
  swappable media probing, an embedded player service, a filesystem watcher,
  exposed as **Tauri v2** commands.
- **Playback:** **libmpv** loaded at **runtime via FFI** (`libloading` →
  `libmpv-2.dll` / `libmpv.so`) — **not bundled**; Deskemy uses the user's
  installed mpv (discovered via `DESKEMY_LIBMPV`, exe dir, PATH, common install
  dirs) and prompts to install if missing. Embedded as a **native child window**
  (Win32 `--wid` HWND) sized to the video pane; z-ordered above WebView2. Driven
  by mpv properties, polled by a 200 ms pump; state pushed to the UI as
  `player:state` events.
- **Media probing:** behind a `MediaProber` trait — `MpvProber` (headless
  libmpv) with a `StubProber` fallback when libmpv is absent (import still works,
  minus durations). Swappable to an `FfprobeProber` later.
- **Key deps:** `uuid` (v7), `blake3`, `thiserror`, `tracing`, `walkdir`,
  `notify-debouncer-mini`, `base64`, `windows-sys`, `libloading`,
  `tauri-plugin-dialog`, `tauri-plugin-opener` (`protocol-asset` feature).

## Cross-cutting decisions

- **Desktop-first**, not Windows-locked. Platform window embedding is isolated
  in `player/`; everything else is portable.
- **UUIDv7 `TEXT` primary keys** everywhere (time-ordered, index-friendly,
  sync-safe). Never auto-increment ints.
- **Domain vs. persistence:** `domain.rs` holds serializable domain types sent
  to the UI as-is; `db/queries.rs` owns **all** SQL and maps rows directly into
  domain types (no rusqlite detail leaks upward).
- **One error type:** a single `DeskemyError` enum; every command returns
  `Result<T, DeskemyError>`, serialized to `{ kind, message }` for the UI.
- **Namespaced IPC:** commands grouped by domain (`library_*`, `course_*`,
  `player_*`, `bookmark_*`, `search_*`, `stats_*`, `config_*`, …).
- **Typed config:** a serde `AppConfig` persisted as `config.json` in app-data.
- **Logging from day one:** `tracing` + `tracing-subscriber`.
- **The "airspace" rule:** mpv's native surface can't be overlaid by HTML, so
  player controls **dock below** the video and side panels **shrink** it — the UI
  never floats over the video. (See `memory/player-compositing-decision`.)
- **`Co-Authored-By` trailer is intentionally omitted** from commits.

## Architecture — module layout (actual)

```
src-tauri/src/
  scanner/mod.rs        Scanner trait → FilesystemScanner: walk + classify → ScannedTree
  importer/mod.rs       structuring rules → persist; metadata reuse + data preservation on
       structure.rs     re-import; owns scan_status
  media/mod.rs          MediaMetadata + tracks + Chapter
       mpv_prober.rs     MpvProber (headless libmpv)   stub.rs  StubProber (fallback)
  mpv/mod.rs            runtime FFI wrapper over libmpv (libloading) + discovery
  player/mod.rs         PlayerState + PlayerService → MpvPlayer (child window, pump,
                        playlist, tracks, chapters, resume, watch-time telemetry)
  subtitles.rs          minimal SRT/VTT parser → (start_ms, text) cues
  thumbnails.rs         content-addressed thumbnail store (blake3) + magic-byte sniffing
  watcher.rs            notify filesystem watcher → debounced re-import (opt-in)
  hashing.rs            blake3 content hashing
  config.rs             typed AppConfig ↔ config.json
  db/mod.rs             schema + migrations (v1..v5) + open/configure
       queries.rs        every SQL statement; maps rows → domain
  domain.rs             serializable domain types (Course/Lecture/Section/… + DTOs)
  error.rs              DeskemyError (single crate-wide enum)
  state.rs              AppState (db, config, importer, player, watcher, data_dir)
  commands/mod.rs       thin namespaced Tauri handlers
       player.rs         player_* commands + lecture_get
  examples/             dev tools: dbcheck, statscheck, searchcheck, subcheck, probe_mpv,
                        import_sample

src/
  routes/               library (/) · course/[id] · watch/[lectureId] · search ·
                        bookmarks · favorites · stats · settings
  lib/api.ts            typed invoke wrappers      lib/types.ts   TS mirrors of domain
  lib/stores/app.svelte.ts   library store · ui state · theme
  lib/components/        Sidebar · TopBar · CourseCard · ProgressBar
```

---

## Feature inventory (shipped baseline)

### Library & import
- Register library **roots** (each immediate subfolder = a course) or **import a
  single folder** as one course. Files are **referenced in place**, never copied.
- **Auto-structuring** from the folder tree: subfolders → sections (loose root
  videos → "Introduction"); natural-sort ordering by leading number; title
  cleanup; sidecar subtitles matched to videos; resources attached to
  lecture/section; per-video probe for duration/codec/chapters/`playable`.
- Library grid with **Continue Watching** hero, recently-opened ordering, filter,
  sort (recent/alpha/progress), `scan_status` + `playable` (⚠ Corrupted) badges.
- **Remove course** from the library (DB only; files untouched) with a confirm
  modal, atomic delete.

### Player (embedded mpv)
- Native child-window embedding, rect-synced to the video pane (DPI-aware).
- Play/pause, seek, **speed**, volume/mute, prev/next, **resume** from saved
  position, **autoplay-next** (playlist model), fullscreen/immersive, keyboard
  shortcuts (space/arrows/f/m/n/esc) that don't get stolen by focused controls.
- **Subtitle + audio track picker** (embedded + external sidecar subs via mpv
  `track-list`; full sidecar filename shown), **chapter navigation** — all in a
  panel that pushes the video up (no airspace overlap).
- **Course-content sidebar** (Udemy-style tree) to jump around the course; back
  button + Esc to the course; progress written periodically + on completion (EOF
  or ≥90 %); manual complete toggle.
- **Deep-link seek**: `/watch/<id>?t=<sec>` jumps to a timestamp (used by
  bookmarks and subtitle search).

### Bookmarks
- Capture the current time-pos with an optional label; per-lecture list with
  jump-to + delete; a **global `/bookmarks` page** grouped by course that
  deep-links into the player.

### Thumbnails
- Set a course cover by **file upload** or **clipboard paste** (Ctrl+V), or
  **remove**, via a hover-edit → modal. Stored **content-addressed** in
  `app-data/thumbnails/` (blake3) and served through the Tauri **asset protocol**.
- **Resume-frame grab**: leaving the player captures an mpv screenshot of where
  you left off; the Continue Watching hero shows it.
- **Thumbnail-cache GC** (Settings) sweeps unreferenced files.

### Search
- **FTS5 over titles** — course / section / lecture / **attachment** (indexed on
  import; rebuilt on startup + on demand). Ranked, injection-safe query builder.
- **Subtitle full-text search**: sidecar SRT/VTT parsed into a `subtitle_index`
  FTS table; results show a snippet + timestamp and **jump to that moment**.
  Built via Settings → "Index subtitle text".

### Tags
- Add/remove **tags** on a course; **filter the library** by tag chips.

### Stats (watch telemetry)
- Per-day **`daily_activity`** telemetry: the player records real watch-time and
  completions. Dashboard: daily-goal ring, current + best **streak** (≥15 min/day),
  watch time, lectures/courses completed, overall-progress bar, a **GitHub-style
  watch heatmap**, this-week chart, active-days-this-month, last-7-day velocity,
  and a "currently focused" card (most-progressed course this week).

### Settings
- Theme (**dark / light / system**), default speed, autoplay-next, daily goal,
  **auto-rescan toggle** (opt-in), and maintenance actions: check for missing
  files (reconcile), rebuild search index, index subtitles, clean thumbnail cache.

### Auto-rescan (opt-in)
- A `notify` watcher over roots + standalone course folders → debounced **data-
  preserving re-import** on change (progress/bookmarks/tags/favorite/thumbnails
  survive, remapped by file path; unchanged files skip re-probing). Emits
  `library:changed` → the grid refreshes. **Off by default**; never re-imports
  the course currently playing.

---

## Data model (SQLite, schema v5, UUIDv7 TEXT keys)

```
library_roots(id, path UNIQUE, added_at)

courses(id, root_id, title, folder_path UNIQUE, thumbnail_path,
        resume_thumbnail_path,             -- v2: Continue-Watching frame
        total_duration, lecture_count, is_favorite,
        scan_status TEXT,                  -- Importing | Scanning | Ready | Missing | Error
        last_opened_at, last_lecture_id,   -- continue-watching resume pointer
        imported_at, last_scanned_at)

sections(id, course_id, title, position, folder_path)

lectures(id, course_id, section_id, title, file_path, position,
         duration, container, video_codec,
         playable,                         -- 0 = mpv can't open (corrupted) → UI ⚠
         file_size, mtime, content_hash)   -- (size,mtime) used for re-probe skip

chapters(id, lecture_id, idx, title, start_time)   -- cached from probe
subtitles(id, lecture_id, lang, label, file_path)  -- external sidecar subs
attachments(id, course_id, section_id, lecture_id, name, file_path, kind)

progress(lecture_id PK, position_seconds, completed, last_watched_at)
bookmarks(id, lecture_id, course_id, position_seconds, label, created_at)
course_tags(course_id, tag, PRIMARY KEY(course_id, tag))          -- v3
daily_activity(day PK, watch_seconds, lectures_completed)         -- v5: telemetry

search_index   USING fts5(kind, entity_id UNINDEXED, course_id UNINDEXED, title)
subtitle_index USING fts5(lecture_id UNINDEXED, course_id UNINDEXED,             -- v4
                          start_ms UNINDEXED, text)
```

Migrations run in a **single transaction** (atomic; safe to interrupt). FK
cascades on `courses`/`lectures`; FTS tables have no FK and are cleared
explicitly in `delete_course`. Config lives in `config.json`, not the DB.

## Config (`config.json`)

```rust
AppConfig { theme: "dark"|"light"|"system", default_speed: f64,
            autoplay_next: bool, daily_goal_minutes: i64,
            auto_rescan: bool, last_root: Option<String> }
```

## IPC commands (namespaced)

```
library_add_root · library_list_roots · library_remove_root · library_scan_root
        · library_import_course · library_list_courses · library_delete_course
        · library_reconcile
course_get · course_set_favorite · course_touch_opened · lecture_set_completed
        · course_attachments · course_tags · course_add_tag · course_remove_tag
        · course_set_thumbnail_file · course_set_thumbnail_bytes · course_clear_thumbnail
open_resource · thumbnails_gc
player_open · player_toggle_pause · player_set_paused · player_seek · player_set_speed
        · player_next · player_prev · player_tracks · player_set_subtitle · player_set_audio
        · player_set_chapter · player_set_volume · player_set_muted · player_set_rect
        · player_stop · player_grab_resume_frame · player_state · player_available · lecture_get
bookmark_add · bookmark_list · bookmark_delete · bookmark_list_all
search_query · search_reindex · subtitle_search · subtitles_reindex
stats_get
config_get · config_set
```

Events: `player:state` (PlayerState tick), `player:advanced` (autoplay),
`library:changed` (watcher).

---

## Milestones — all complete ✅

- **M0** Scaffold (Tauri + SvelteKit-TS + adapter-static + Tailwind + rusqlite + tracing).
- **M1** Backend core: schema/migrations, error, scanner, importer, prober, config, commands.
- **M1-IT** Importer integration test (hierarchy/ordering/resources; + re-import preservation).
- **M2** Library grid + course view (continue-watching, badges).
- **M3** Embedded mpv player (child window, resume, autoplay, shortcuts, progress).
- **M4** Subtitle/audio track picker + chapter navigation.
- **M5** Bookmarks (per-lecture + global page, jump-to).
- **M6** Thumbnails (manual upload/paste + resume-frame grab + content-hash cache + GC).
- **M7** Search (FTS5 titles + subtitle full-text with jump-to-timestamp).
- **M8** Settings/config UI, favorites, **theme (dark/light/system)**, missing-file
  reconciliation, remove-course, thumbnail GC, polish.
- **Post-v1** Resources UI (open pdfs/zips/code), Tags + filter, Stats dashboard +
  watch telemetry, Auto-rescan (opt-in), pre-release review + hardening.

---

## Known limitations & deferred work

These are understood, non-blocking, and good starting points for expansion:

- **Auto-rescan probe-under-lock:** re-import probes new files while holding the
  DB lock, so a large bulk-add can briefly pause the UI. Mitigated by making
  auto-rescan **opt-in** and skipping the active course; the real fix is a
  probe-outside-lock (two-phase import) refactor.
- **Rename ⇒ progress loss:** re-import remaps user data by **file path**, so a
  renamed file is treated as removed+new (its progress/bookmarks drop).
  `content_hash` exists in the schema but is not populated — hash-based rename
  detection is the fix.
- **Player teardown:** the mpv instance/child window aren't destroyed on app exit
  (benign; OS reclaims). No leak on repeated opens.
- **Durations imported pre-mpv:** courses imported before mpv was available have
  null durations; a rescan/re-import backfills them (reuse re-probes null-duration
  files).
- **Stats not yet built** (need new instrumentation, intentionally not faked):
  session tracking (avg/longest session, focus %), average playback speed, notes/
  highlights counts, transcript-search counts.
- **No CI type-check yet:** `svelte-check`/`tsc` isn't wired into a build gate.

---

## Future work / expansion candidates

- Two-phase import (probe outside the DB lock) → make auto-rescan freeze-free.
- Content-hash change/move detection → carry progress across renames; smarter
  incremental sync instead of delete+reinsert.
- Session-level telemetry → the deferred stats (sessions, focus %, avg speed).
- **Notes/highlights** (timestamped) and search history → richer study data.
- More scanners behind the `Scanner` trait (`ZipScanner`, `CloudScanner`).
- `FfprobeProber` alternative behind `MediaProber`.
- True HTML-over-video compositing (DirectComposition) if floating overlays are
  wanted (see the compositing decision memo).
- Android / LAN sync (UUIDv7 keys already make this safe).
- Packaged installer (`tauri build`) + a `svelte-check` CI gate.

## Prerequisites

- Node 22 / npm, WebView2 runtime (Windows), MSVC C++ Build Tools (VS 2022),
  Rust toolchain — all installed.
- **libmpv runtime** (`libmpv-2.dll` on Windows via an mpv install; `libmpv`
  package on Linux) — user-provided, discovered at runtime (not bundled).
