# Deskemy — Offline Course Player

A **desktop-first** local app (Windows primary, Linux supported) that gives a
Udemy-style experience for downloaded courses: import a folder of courses,
auto-structure them from the folder tree, and play them offline with resume,
chapters, subtitles, bookmarks, and search.

## Stack

- **Frontend:** SvelteKit + `adapter-static` (SPA), Svelte 5 runes, TypeScript,
  Tailwind v4 (dark-first), lucide icons.
- **Backend:** Rust — `rusqlite` (bundled SQLite + FTS5), scanner + importer,
  swappable media probing, a player service, exposed as Tauri v2 commands.
- **Playback:** **libmpv** (`libmpv2` crate) as the **sole playback backend** —
  no transcoding pipeline in v1. Behind a `PlayerService` trait, embedded as a
  native child window (HWND on Windows / X11 window-id on Linux) sized to the
  video pane. Driven by mpv properties (`time-pos`, `speed`, `sid`, `chapter`,
  `pause`); mpv events surfaced as a typed `PlayerEvent`. (Plays almost every
  format; corrupt / encrypted / exotic files can still fail → `playable` flag.)
- **Media probing:** behind a `MediaProber` trait; `MpvProber` (libmpv headless)
  for v1 → a concrete `MediaMetadata`. Swappable to an `FfprobeProber` later
  without touching callers. No ffmpeg/ffprobe sidecars in v1.

## Cross-cutting decisions

- **Desktop-first**, not Windows-locked. Platform-specific window embedding is
  isolated inside `PlayerService`; everything else is portable.
- **UUIDv7** `TEXT` primary keys everywhere (time-ordered, index-friendly,
  sync-safe for future Android / LAN sync). Never auto-increment ints.
- **Domain models ≠ persistence models.** Each feature module owns its domain
  type (`Lecture`, `Course`…); `db/models.rs` owns the `*Row` SQLite shapes and
  the conversions. The persistence layer never leaks upward.
- **One error type.** A single `DeskemyError` enum; every command returns
  `Result<T, DeskemyError>` — never stringly-typed errors.
- **Namespaced IPC.** Commands are grouped by domain (`library_*`, `course_*`,
  `player_*`, `bookmark_*`, `search_*`, `config_*`), never bare `play`/`seek`.
- **Typed config**: a serde `AppConfig` persisted as `config.json` in app-data.
- **Logging from day one**: `tracing` + `tracing-subscriber`.

## Confirmed product decisions

- Library **root auto-scan**: register a root folder; each immediate subfolder = a course.
- **Reference files in place** — never copy. DB stores paths only.
- **Custom player controls docked below** the video pane (mpv's native surface
  can't be overlaid by HTML — "airspace"; docking beneath avoids it).
- Search = **FTS5 over titles** (course / section / lecture / attachment, each
  indexed separately). Subtitle full-text deferred.
- v1 player: resume + autoplay-next (playlist model), speed + keyboard shortcuts,
  subtitle track picker (embedded + external), chapters, **bookmarks**.
  Timestamped notes deferred.
- Netflix-style continue-watching + recently-opened.
- blake3 change/move detection.

## Architecture — clean separations

```
scanner/   Scanner trait → FilesystemScanner: walk + classify → ScannedTree
           (no DB, no rules; future Zip/Network/Cloud scanners slot in here)
importer/  models.rs (domain) + structuring rules → persist; owns scan_status
media/     metadata.rs (MediaMetadata + tracks + Chapter) + MediaProber → MpvProber
player/    models.rs (PlayerState) + PlayerService → MpvPlayer (mpv + Playlist),
           emits PlayerEvent
config/    typed AppConfig (serde) ↔ config.json
db/        models.rs (*Row types) + schema + migrations + queries
error/     DeskemyError (single crate-wide error enum)
commands/  thin, namespaced Tauri handlers over the above
```

## Domain types

### MediaMetadata (the single prober return shape)

```rust
pub struct MediaMetadata {
    pub duration: Duration,
    pub playable: bool,
    pub video_tracks: Vec<VideoTrack>,
    pub audio_tracks: Vec<AudioTrack>,
    pub subtitle_tracks: Vec<SubtitleTrack>,   // embedded
    pub chapters: Vec<Chapter>,
    pub container: String,
    pub video_codec: String,
    pub thumbnail: Option<PathBuf>,
}
```
Every `MediaProber` impl returns this exact object.

### PlayerState (single object the frontend subscribes to)

```rust
pub struct PlayerState {
    pub lecture_id: Uuid,
    pub position: Duration,
    pub duration: Duration,
    pub paused: bool,
    pub speed: f64,
    pub subtitle_track: Option<i64>,
    pub chapter: usize,
}
```
`PlayerEvent`s mutate this; Svelte subscribes to one store, not scattered values.

### PlayerService + Playlist

`MpvPlayer` owns the embedded mpv instance and a **Playlist** (ordered lectures
of the current course + a cursor). Autoplay-next = advance the cursor, not an
ad-hoc load — smooth transitions, consistent progress writes. Platform window
embedding lives here and nowhere else.

### PlayerEvent (typed — no stringly-typed events)

```
PlayerReady        mpv instance up, ready to load
PlayerOpened       a file finished loading (duration/tracks/chapters known)
Playing            playback (re)started            (+ counterpart to Paused)
Paused
TimeChanged        position tick
Ended              current item reached EOF
TrackChanged       audio/video track switched
SubtitleChanged    subtitle track (sid) switched
ChapterChanged     current chapter changed
SpeedChanged
PlaylistAdvanced   autoplay moved to next lecture  (+ for playlist model)
Error              typed error payload
```
(The bottom-marked entries extend the requested list; the rest are as specified.)

## Error model

```rust
pub enum DeskemyError {
    Import(..),      // scanning / structuring
    Probe(..),       // MediaProber failures
    Player(..),      // mpv / embedding
    Database(..),    // rusqlite
    Config(..),
    Io(..),
}
```
Commands return `Result<T, DeskemyError>`; serialized to a typed payload for the UI.

## IPC command namespaces

```
library_add_root · library_scan · library_list_courses · library_remove_root
course_get · course_set_favorite · course_touch_opened
player_open · player_pause · player_resume · player_seek · player_set_speed
        · player_set_subtitle · player_set_chapter · player_next · player_prev
bookmark_add · bookmark_list · bookmark_delete
search_query
config_get · config_set
```

## Data model (SQLite, UUIDv7 TEXT keys)

```
library_roots(id, path UNIQUE, added_at)

courses(id, root_id, title, folder_path UNIQUE, thumbnail_path,
        total_duration, lecture_count, is_favorite,
        scan_status TEXT,        -- Importing | Scanning | Ready | Missing | Error
        last_opened_at, last_lecture_id,   -- continue-watching resume pointer
        imported_at, last_scanned_at)

sections(id, course_id, title, position, folder_path)

lectures(id, course_id, section_id, title, file_path, position,
         duration, container, video_codec,
         playable BOOLEAN,       -- false if mpv can't open (corrupted) → UI ⚠
         file_size, mtime, content_hash)   -- change/move detection

chapters(id, lecture_id, idx, title, start_time)   -- cached from probe

subtitles(id, lecture_id, lang, label, file_path)  -- external subs only
attachments(id, course_id, section_id, lecture_id, name, file_path, kind)

progress(lecture_id PK, position_seconds, completed, last_watched_at)
bookmarks(id, lecture_id, course_id, position_seconds, label, created_at)

search_index USING fts5(kind, entity_id UNINDEXED, course_id UNINDEXED, title)
             -- kind ∈ course | section | lecture | attachment
```

Course progress derived from `progress`. Continue-watching = most recent
`last_watched_at`; recently-opened = `courses.last_opened_at`.
Config lives in `config.json`, not the DB.

## Thumbnail cache

Thumbnails live in `app-data/thumbnail_cache/` keyed by content identity, not by
source filename: `<content_hash>.jpg` (lecture frame) / `<course_uuid>.jpg`
(course cover). `courses.thumbnail_path` stores the cache path. Content-hash keys
make invalidation trivial — a changed file yields a new key, orphans get swept.

## Change detection (hashing)

- Quick key `(file_size, mtime)` → skip unchanged files instantly on re-scan.
- **blake3** content hash (bounded sample for very large files) → stable identity
  to detect moved/renamed files and carry over progress + bookmarks.

## Auto-structuring algorithm (in importer/)

1. Consume `ScannedTree` (already classified: video / subtitle / attachment / image).
2. Immediate subfolders = **sections**; videos loose in root → implicit
   "Introduction". Flat folder → single section.
3. **Ordering:** parse leading integer (`01`, `1.`, `1 -`, `Section 1`);
   natural-sort fallback. Same rule for sections and lectures.
4. **Title cleanup:** strip numeric prefix + separator + extension for display;
   keep raw for sort tiebreak.
5. **Subtitles:** match by video basename (`name.srt`, `name.en.vtt`); parse lang.
6. **Attachments:** basename match → lecture; files under
   `resources/attachments/code/assets` → nearest lecture or section-level.
7. **Probe:** `MediaProber` per video → `MediaMetadata` (duration/chapters/codec/
   playable/tracks).
8. **Thumbnail:** prefer a cover/thumbnail/poster image; else mpv screenshot →
   cache by content hash.
9. Set `courses.scan_status` across the lifecycle (Importing → Scanning → Ready,
   or Missing/Error).

## Project layout

```
src/routes/       library · course/[id] · watch/[lectureId] · search · settings
src/lib/          api.ts (typed invoke wrappers) · stores · components
src-tauri/src/    scanner/ · importer/ · media/ · player/ · config/ · db/ · error/ · commands/
src-tauri/binaries/  libmpv-2.dll (+ generated MSVC import lib)
```

## Milestones

- **M0** Scaffold: create-tauri-app (SvelteKit-TS) + adapter-static + Tailwind +
  rusqlite + `tracing`; empty `tauri dev` window runs.
- **M1** Backend core: DB schema/migrations (UUIDv7), `error/`, `Scanner`/
  `FilesystemScanner` (unit-tested), `importer/` structuring (unit-tested),
  `MediaProber`/`MpvProber` → `MediaMetadata`, typed config, namespaced commands.
- **M1-IT** Importer integration test: sample library folder → scan → SQLite →
  assert full course/section/lecture hierarchy + ordering. (Catches the most regressions.)
- **M2** Library grid + course view: sections, chapters, continue-watching,
  recently-opened, `scan_status` + `playable` (⚠ Corrupted) badges.
- **M3** `PlayerService`/`MpvPlayer`: embed via native child window, HWND rect-sync,
  `PlayerEvent` → `PlayerState`, Playlist, play/pause/seek, resume, speed,
  shortcuts, autoplay-next, progress writes.
- **M4** Subtitle + track picker (embedded + external via mpv `track-list`);
  chapter navigation.
- **M5** Bookmarks (capture current `time-pos` + label; jump-to; list per lecture).
- **M6** Thumbnails (mpv screenshot + image-file detection + content-hash cache).
- **M7** Search (FTS5 populate for course/section/lecture/attachment + UI).
- **M8** Settings/config UI, favorites, theme, missing/moved-file reconciliation,
  polish.

## Future work (post-v1, architecture already accommodates)

- **Watch-folder** via `notify` → incremental rescan on filesystem changes.
- **More scanners** behind the `Scanner` trait: `ZipScanner`, `NetworkScanner`,
  `CloudScanner`.
- **`FfprobeProber`** alternative behind `MediaProber`.
- Subtitle full-text search; timestamped notes; tags/categories; stats/streaks.
- Android / LAN sync (UUIDv7 keys already make this safe).

## Prerequisites

- Node 22 / npm — installed.
- WebView2 runtime (Windows) — installed.
- MSVC C++ Build Tools (VS 2022) — installed.
- Rust toolchain (rustup) — **installing** (user-managed).
- libmpv dev build (`libmpv-2.dll` + import lib on Windows; `libmpv` package on
  Linux) — to acquire at M3.
