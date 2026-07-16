# Deskemy — Roadmap

Forward-looking plan. For the shipped **baseline** (features, schema, commands,
known limitations) see [PLAN.md](PLAN.md). Theme through v1.x: **"the best
offline course player"** — deepen the core watch/learn loop, don't expand scope.

**1.0.1 shipped** (first public release). The next feature release — backup &
restore, an in-app auto-updater, plus daily-use quick wins — is planned in
[docs/v1.1-plan.md](docs/v1.1-plan.md).

Visual / theming work (custom themes from a color palette, etc.) is tracked
separately in [docs/visual-overhaul.md](docs/visual-overhaul.md) — **deferred**
until the core features land; the palette→semantic-role mapping is already proven.

Legend: ✅ done · 🔜 next · ⏳ planned · 🔑 key enabler · ❌ cut (with reason)

---

## Guiding decisions (don't relitigate without new info)

- **Stay on Tauri.** The "airspace" pain (mpv's native window can't be overlaid
  by HTML) is a two-surfaces problem, not a Tauri bug — it follows us to
  GPUI/Iced too. A framework migration = full UI rewrite + immature framework to
  gain on-video overlays our NOT-build list rejects. If floating-over-video ever
  becomes must-have, the surgical fix is libmpv render-API → D3D11 texture →
  DirectComposition visual behind a transparent WebView2 (contained, no rewrite).
- **Airspace rule for the player:** anything that would overlay the video gets
  **docked or shrinks the video**, never floated over it (controls dock below;
  panels/menus/cheat-sheet push the video up; sidebar shrinks it). Modals are
  fine on non-player screens (no mpv window there).
- **Mini Player, if ever revisited:** do the **audio-continues mini-bar** (pure
  HTML, keeps audio playing while browsing) — NOT a live-video PiP (native
  rectangle with airspace rough edges). Currently cut.
- **NOT building** (dilute the identity): ❌ AI chat · ❌ flashcards · ❌ notes ·
  ❌ transcript editor · ❌ recommendation engine · ❌ social · ❌ user accounts ·
  ❌ Plex/Jellyfin-scale media-server features · ❌ playback queue.

---

## v1.1 — Player UX  ✅ (shipped)

- ✅ **Keyboard shortcuts + `?` cheat sheet** — full YouTube-style set + Deskemy
  additions (N/⇧N, P content, R resources, B bookmark). Cheat sheet is a docked
  push-up panel (airspace).
- ✅ **Up Next** — next lecture (title + duration) in the control bar; click to skip.
- ✅ **Fullscreen** — `F` / Esc toggle real OS fullscreen with the sidebar/topbar
  hidden. The watch page pins to the viewport (`fixed inset-0`) so the themed body
  never leaks as a bar. The control bar stays docked and visible, including in
  fullscreen. (Auto-hide/vanishing-controls was built then **cut**: docked chrome
  reflows the native window — airspace, can't overlay — so hiding it made the
  video rubberband. Not worth it, and the bar looks good.)
  - **Accepted tradeoff — don't "fix" it:** a permanent docked bar makes the video
    pane wider than 16:9, so a 16:9 video letterboxes (~80px black side bars).
    That's mpv keeping aspect, not a bug. We keep the letterbox (true pixels) over
    zoom-to-fill (crops edges). Bars would only vanish if the bar could overlay
    the video, which airspace forbids.
- ✅ **Remember playback prefs per course** — speed / subtitle selection +
  visibility / audio track (schema v6 `course_prefs`).
- ⏳ **Sleep timer** — pause after N minutes / at end of lecture. (Small; not built yet.)
- ❌ **Mini Player / PiP** — cut: not enough value for a *learning* app + airspace
  friction. (Audio mini-bar is the fallback if revisited.)

## v1.2 — Better learning experience

- ✅ **Resources panel (right sidebar)** — in-player, **scoped to the current
  lecture's section**, grouped by lecture (avoids course-wide overload). `R`.
- ✅ **Playback history** — `/history` page grouped by day (Today / Yesterday /
  date), resume where you left off. Built from `progress.last_watched_at`, no new
  schema. (shipped)
- ✅ **Career Tracks** — user-created ordered course groupings with aggregate
  completion (Σ completed / Σ total lectures) and per-course ✓ / ◉ / ○ status.
  New `tracks` + `track_courses` tables (schema v7), `/tracks` + `/tracks/[id]`,
  sidebar entry. Player is unchanged — pure grouping. (shipped)
- ⏳ **Import preview** — dry-run shows detected counts (videos/sections/resources/
  subtitles/duration) → confirm, instead of a blocking "Importing…". 🔑 needs
  two-phase import.

## v1.3 — Navigation & discovery

- ✅ **Global shortcut overlay** — the `?` cheat sheet (Playback/Navigation/Panels).
- ⏳ **Better library filters** — recently watched / never started / finished /
  in progress / favorites / tags / career track. Mostly client-side.
- ❌ **Filmstrip timeline** — cut: coolest but most expensive (needs a 2nd headless
  mpv for background sprite-gen); low value for a learning app.

## v1.4 — Import & scanner

- 🔑 **Two-phase import (probe outside the DB lock)** — THE pivotal refactor.
  Unlocks import preview + import progress, and makes auto-rescan freeze-free
  (removes the current probe-under-lock pause). Do this before preview/progress.
- ⏳ **Import progress (real-time)** — stream scanning progress (N videos / PDFs /
  ZIPs, sections detected). 🔑 needs two-phase import.
- ⏳ **Rename detection (`content_hash`)** — populate blake3 hash (already wired),
  match by hash on re-import so renamed files keep progress/bookmarks. Closes the
  last data-loss gap (see PLAN.md limitations). Pairs with the import refactor.
- ⏳ **Smarter importer heuristics** — loose videos, nested sections, mixed
  resource folders, numbering variants / missing numbers, better title cleanup.
  No Udemy-specific scanner — just better detection. + property tests.

## v1.5 — Multi-device

- ❌ **CRDT progress/bookmarks/settings sync (desktop ↔ laptop, no cloud)** — cut
  for now (optional; large). UUIDv7 keys already keep it safe if revisited.

## Engineering (parallel, non-user-facing)

- 🔑 **Two-phase import / probe outside DB lock** (see v1.4 — highest leverage).
- ⏳ **Property tests** — natural sorting, importer, scanner, resource classification.
- ⏳ **SQLite tuning** — low priority; the import bottleneck is mpv probing, not
  SQLite (WAL is on, inserts already batched in a tx). Add a benchmark + targeted
  indexes only if it justifies itself.
- ⏳ **CI** — cargo fmt/clippy/test + `svelte-check` + eslint. **Deferred** (user
  is focused on UX right now); cheap and protects everything when picked up. Note:
  a `svelte-check`-only type bug slipped past `npm run build` once (caught by review).

---

## Recommended order (post player-polish sprint)

1. **v1.2 learning batch:** Playback history → Career Tracks.
2. **Two-phase import** (🔑) → then Import preview + Import progress fall out cheaply.
3. **Rename detection** (`content_hash`) — pairs with #2.
4. Better library filters · Sleep timer (quick wins, slot in anywhere).
5. Later/optional: importer heuristics + property tests · SQLite tuning (if a
   benchmark justifies) · CI · CRDT sync.

Loose small items to fold in when convenient: **Sleep timer** (v1.1), **better
library filters** (v1.3).
