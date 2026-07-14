# Deskemy — Roadmap

Forward-looking plan. For the shipped **baseline** (features, schema, commands,
known limitations) see [PLAN.md](PLAN.md). Theme through v1.x: **"the best
offline course player"** — deepen the core watch/learn loop, don't expand scope.

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
- ✅ **Vanishing controls / Focus Mode** — in fullscreen the controls auto-hide
  after ~2.5s and the video reflows to true 100% height (no leftover strip);
  any key brings them back, Esc exits fullscreen. Mouse-over-video can't reveal
  them (airspace) — keyboard-driven by design. (Reflow, not fade-over.)
- ✅ **Remember playback prefs per course** — speed / subtitle selection +
  visibility / audio track (schema v6 `course_prefs`).
- ⏳ **Sleep timer** — pause after N minutes / at end of lecture. (Small; not built yet.)
- ❌ **Mini Player / PiP** — cut: not enough value for a *learning* app + airspace
  friction. (Audio mini-bar is the fallback if revisited.)

## v1.2 — Better learning experience

- ✅ **Resources panel (right sidebar)** — in-player, **scoped to the current
  lecture's section**, grouped by lecture (avoids course-wide overload). `R`.
- ⏳ **Playback history** — YouTube-style history page (grouped by day, resume
  where you left off). Buildable from existing `progress.last_watched_at` — no
  new schema for a v1.
- ⏳ **Career Tracks** — user-created ordered course checklists (e.g. "Platform
  Engineering": ✓ Linux ▶ Docker □ K8s …). New `tracks` + `track_courses` tables,
  a page + sidebar entry. Self-contained; high UX/effort ratio.
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
