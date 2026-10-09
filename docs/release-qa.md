# Release QA runbook

Run this for every release, on the exact files that will be published, after
`package.ps1` and before marking the release **latest**. [releasing.md](releasing.md)
says how to build and publish; this says how to know the build is fit to ship.

Deskemy has no Windows CI and no UI automation, so most of what protects users
is this checklist. Take its time seriously.

## How much to run

| Release | Run | Time |
|---|---|---|
| **Patch** (2.0.x: fixes, small touches) | §1–§5, §6 *Golden path*, §6 for every area the release touched, §9 | ~45 min |
| **Minor / major** (2.x.0, 3.0.0: new features) | Everything | ~2 h |
| **Rebuild of a held draft** (same version, new commits) | §1–§3 again, §6 for the new commits' areas, §5 *upgrade from the last published release* | ~20 min |

Whatever the tier, always run §1, §2, §3 and §5.1. Those are the steps that
catch a release that breaks for everyone.

## Ship / no-ship

| Severity | Examples | Ship? |
|---|---|---|
| **Blocker** | Crash or hang on launch; library, progress or settings lost; can't import or play; update fails, loops or installs the wrong kind; installer fails; a signature doesn't verify | **No.** Fix and rebuild |
| **Major** | A feature is broken with no workaround; a regression from the last release in something people use daily (resume, autoplay, mini player, search) | **No**, unless the bug is already in the published release and this one doesn't make it worse |
| **Minor** | Cosmetic issues, a rare edge case, something with an easy workaround | Ship. Open an issue and list it under "Known issues" in the release notes |

If you're unsure between two levels, pick the higher one. A bad update reaches
every user within a day through the auto-updater, and the only way back is
another release.

---

## 0. Prep

- [ ] `git status` is clean, and `main` holds what you're shipping.
- [ ] Note the previous release: `git describe --tags --abbrev=0`.
- [ ] List what changed: `git log --oneline <prev>..main`.
  - Next to each commit, write the §6 area or areas it touches.
  - Those areas get a full pass even for a patch release.
  - Commits that touch `mini.rs`, `caption.rs`, `video.rs`, `session.rs`, `db/`, `updates.rs`, `installer/` or `package.ps1` are high-risk. Test them even if the change looks trivial.
- [ ] Have these ready:
  - the previous release's setup, MSI and portable zip (from GitHub)
  - the 1.2.2 setup (the last Tauri build)
  - the QA courses: `powershell -File app/scripts/make-qa-course.ps1` (see *Test fixtures*)
- [ ] Use a scratch data folder for anything that doesn't need your real library: `$env:DESKEMY_DATA_DIR = "$env:TEMP\deskemy-qa"`. Never test destructive actions (delete, backup import, compact) on your real library.

## 1. Automated gate

Run these from a clean build of the release commit.

- [ ] Core tests: `cd crates/deskemy-core; cargo test`. All pass.
- [ ] App tests: `cd app; $env:DESKEMY_REQUIRE_LIBMPV = 1; cargo test --release`. All pass.
  - Without `DESKEMY_REQUIRE_LIBMPV`, 11 of the 12 player session tests skip themselves when libmpv can't be found, and the run still shows green.
  - With it set, a missing libmpv fails them instead. Fix that by putting `libmpv-2.dll` (from `app/vendor/`) where the tests can find it.
- [ ] Clippy: `cd app; cargo clippy --release`. No warnings.
- [ ] Linux CI is green for the release commit.
  - It runs on every push to `main` that touches `app/` or `crates/`: `gh run list --workflow slint-linux.yml --branch main -L 3`.
  - To run it by hand: `gh workflow run slint-linux.yml --ref main`, then `gh run watch`.
  - Linux doesn't ship, but a red Linux build usually means a `cfg(windows)` mistake that will bite later.

## 2. Snapshot sweep (offscreen UI check)

`--snapshot` renders a page offscreen, with no window on screen. Render every
page and compare it with the last release's renders. This catches layout
regressions, missing text and broken dialogs in a few minutes.

```powershell
$exe = "app\target\release\deskemy.exe"
$out = "$env:TEMP\deskemy-qa\snaps-$(Get-Date -Format yyyyMMdd)"
New-Item -ItemType Directory -Force $out | Out-Null
$env:DESKEMY_DATA_DIR = "$env:TEMP\deskemy-qa\data"   # a scratch library with the QA course imported
$pages = "", "favorites", "history", "bookmarks", "stats", "settings", "settings-import",
         "import-preview", "import-scanning", "search:intro", "tracks", "tracks-create",
         "track", "track-add", "track-edit", "track-delete",
         "course", "course-cover", "course-delete", "update:library", "update:settings-downloading"
foreach ($p in $pages) { & $exe --snapshot "$out\page-$($p -replace '[:]','_').png" $p }
$menus = "", "speed", "sub", "audio", "chapters", "sleep", "bookmark", "shortcuts", "stats",
         "content", "resources", "exercise", "speed-saved", "fullscreen", "mini", "mini-exercise", "light"
foreach ($m in $menus) { & $exe --snapshot-player "$out\player-$m.png" $m }
```

- [ ] Every file was written. A missing one means the page crashed; look in `<data>\logs\deskemy.log`.
- [ ] Look through each image:
  - no clipped or overflowing text
  - no overlapping elements
  - dialogs centred, with their dimmed backdrop
  - icons present, not boxes
  - light theme readable (`player-light.png`, and `settings` with Theme set to Light)
- [ ] Compare against the previous release's sweep, kept in the same folder layout. Anything that changed should be explained by a commit in §0.
- [ ] For pages with a fixed width, also render at the minimum window size:
  - `$env:DESKEMY_SNAPSHOT_WIDTH=900; $env:DESKEMY_SNAPSHOT_HEIGHT=600` (pages)
  - `$env:DESKEMY_SNAPSHOT_WIDTH=1920; $env:DESKEMY_SNAPSHOT_HEIGHT=1080` (pages and player)

Snapshots use Slint's software renderer, not the Skia/OpenGL renderer of the
real window. Text sharpness, video and window behaviour are not covered here;
§6 and §7 cover them.

## 3. Artifacts

In `app/target/packages/`:

- [ ] These files are present with the new version in their names:
  - `deskemy_<v>_x64-setup.exe` + `.sig`
  - `deskemy_<v>_x64_en-US.msi` + `.sig`
  - `Deskemy_<v>_x64-portable.zip`
  - `latest.json`
  - `SHA256SUMS.txt`
- [ ] Sizes are close to the last release's (about 47 MB setup, 62 MB MSI, 62 MB zip). A big jump or drop means something was added or left out.
- [ ] Both signatures verify. Both should print `OK: …`; see [releasing.md §3](releasing.md#3-check-the-signatures).
- [ ] `latest.json`:
  - [ ] `version` is the new version.
  - [ ] It has exactly two entries:
    - `windows-x86_64`: `format` `nsis`, `url` ending in the setup's name
    - `windows-x86_64-msi`: `format` `wix`, `url` ending in the MSI's name
  - [ ] Both URLs point at `releases/download/v<version>/`.
  - [ ] It has no Linux or macOS entry.
  - [ ] `notes` is the one-line summary, with no typos.
- [ ] `SHA256SUMS.txt` matches the files: `Get-FileHash -Algorithm SHA256 <file>` for each.
- [ ] Portable zip: unzip it into a scratch folder and check it holds:
  - `deskemy.exe`
  - `libmpv-2.dll`
  - the `.portable` marker
  - the `licenses` folder
- [ ] Setup and MSI properties: right-click → Properties → Details. File version and product name are right.
- [ ] Release notes `docs/release-notes/<v>.md`:
  - version number and file names correct
  - every user-visible change in §0 is mentioned
  - nothing listed that didn't ship
  - issue numbers and credits right

## 4. Test fixtures

Generate the QA courses with `powershell -File app/scripts/make-qa-course.ps1`.
It needs ffmpeg, writes to `%USERPROFILE%\Deskemy QA` (`-Out` to change,
`-Force` to regenerate), and takes about a minute for ~30 MB. When a bug comes
from a particular kind of folder, add that case to the script, so every later
release is tested against it.

**Deskemy QA Course** has:

- 3 numbered sections (`01 Getting Started`, `02 Formats`, `03 Edge Cases`) with numbered lectures (`001 …` to `012 …`)
- a lecture with a sidecar `.srt` subtitle (and one with two languages)
- an MKV with two audio tracks and an embedded subtitle
- a 6-minute MP4 with 4 chapters
- a 4:3 video and a vertical (9:16) video, which exercise the mini player's aspect handling
- section resources numbered between lectures (`003 Configuring Git.html`) and Udemy-style ones (`8.1 Slides.pdf`, which belongs with lecture 8)
- a course-wide resource at the root, and a `cover.jpg`
- one corrupt `.mp4` of random bytes (it should be flagged "Corrupted", not crash anything)
- a TypeScript `.ts` file and a small code project folder (neither should be counted as videos)
- non-ASCII and long names (`010 Über Café – 第1章 (final).mp4`, a ~130-character title), and a lecture two folders deep (`Extras\Even deeper\012 Nested Lecture.mp4`)
- a lecture shorter than 10 s, which tests completion and autoplay edge cases

Unusual words in the subtitles make search checks quick:

- `quokka` (001, English sidecar)
- `ornitorrinco` (002, Spanish sidecar)
- `axolotl` (006, embedded track)

The two audio tracks of 006 play different tones (440 and 880 Hz), so switching
between them is audible. **Deskemy QA Course 2** has two plain lectures, for
career tracks and library filters.

Also keep a **big library**: your real one, or a copy of its data folder. Use it
to check that launch, search and Stats stay quick.

## 5. Install and upgrade matrix

For fresh-install tests, use a clean machine: **Windows Sandbox** (Windows Pro),
a VM, or a second local Windows user account. These give an empty `%APPDATA%`
and `%LOCALAPPDATA%` without touching your own data. Windows Sandbox may lack
OpenGL acceleration; if the window doesn't draw there, use a spare user account
instead.

### 5.1 Upgrade from the last published release (always)

- [ ] Install the previous setup, add the QA course, and watch part of a lecture.
  - Also leave a bookmark, a favourite and a career track, and change one setting.
- [ ] Install the new setup over it.
- [ ] Settings → About shows the new version.
- [ ] The course, progress, resume position, bookmark, favourite, track and setting are all still there.
- [ ] The window opens in front, not behind other windows or minimized. This regressed in 2.0.

### 5.2 Fresh install, setup (always)

- [ ] On a clean machine or account: run the setup. It installs without asking for admin, and the app launches in front.
- [ ] Add the QA course, close the app, and open it again. **The course is still there.**
  - 2.0.0 shipped without creating the database on a fresh install.
  - Check that `%APPDATA%\com.spooksy.deskemy\deskemy.db` exists.
- [ ] Start Menu shortcut works. Uninstall from Apps & features removes the program.

### 5.3 MSI (minor and major releases, and any release that touches the installer)

- [ ] Fresh MSI install (asks for admin, goes to Program Files). Launches.
- [ ] Previous MSI → new MSI: upgrades in place. Only **one** Deskemy appears in Apps & features.
- [ ] 1.2.2 MSI → new MSI (per-machine, needs admin): the 1.x install is removed and the library carries over.

### 5.4 From 1.x (minor and major releases)

- [ ] 1.2.2 setup with a library → new setup.
  - The library migrates (schema v8).
  - Courses, progress, bookmarks, favourites and tracks are intact.
- [ ] Optional: 1.2.2's own updater with the new `latest.json` (see §8).

### 5.5 Portable (always)

- [ ] Unzip and run. Data goes into `data\` next to the exe; `%APPDATA%` is untouched.
- [ ] Update check: Settings → Check offers the update. **Update** opens the GitHub releases page; it doesn't run an installer.

## 6. Functional checks

Run *Golden path* every time. Then fully check each area that §0 marked as
touched. For minor and major releases, check all of them.

Use the installed build (§5), not `target\release`. The installed build is the
one users get.

### Golden path (every release, ~10 min)

1. [ ] Add Folder → QA course.
   - Preview counts look right: sections, lectures, resources, subtitles, runtime.
   - The corrupt file is flagged. Import.
2. [ ] Library shows the card with its cover, duration and "N Lectures · Not started".
3. [ ] Course page → **Start Course**. The first lecture plays with sound.
   - The control bar shows; it auto-hides after about 2.5 s and comes back when you move the mouse.
4. [ ] Seek with J/L and ←/→. Change the speed. Pick a subtitle track. Set the volume.
5. [ ] Watch about 30 s, then **Back**.
   - The library's "Continue Watching" shows this course.
   - Click it: the lecture **resumes where you left it**, not from 0.
6. [ ] Press End (to 99.9%). It completes, and autoplay moves to lecture 2 from 0:00.
7. [ ] Close the app with the window's ×. Reopen.
   - Progress, the resume position and the per-course speed are all kept.
8. [ ] Stats shows today's watch time; the heatmap cell for today is shaded.
9. [ ] Open `<data>\logs\deskemy.log`. It has no `ERROR`, `panicked` or `WARN` lines you can't explain.

### Library and course page

- [ ] Search box, sort (all 4 options), status pills and tag pills each filter correctly. "No courses match your filters." shows when nothing matches.
- [ ] Course page:
  - favourite toggles
  - tag add (Enter) and tag remove
  - cover upload, Ctrl+V paste and remove
  - lecture done tick and resource done tick
  - section expand/collapse
  - the "Next up" badge is on the right lecture
- [ ] "Keep videos and resources together" on and off:
  - On: resources sit in lecture order (`003 Configuring Git` between lectures 2 and 4; `8.1 Slides` with lecture 8).
  - Off: they're listed separately.
- [ ] Missing folder: rename the course folder.
  - Settings → Check for missing files flags it, and the course shows a missing-folder banner.
  - **Locate folder** with the wrong folder is refused; with the renamed one it works.
- [ ] Remove from library: the course is gone, and the files are still on disk.
- [ ] Re-import the same folder: the preview says Re-import, and progress is kept.

### Player

- [ ] Every shortcut in the cheat sheet (?) does what it says: Space/K, J/L, ←/→, ↑/↓, M, F, T, C, `<`/`>`, Home/End, 0–9, B, N/Shift+N, P/R, I, Esc.
- [ ] Esc closes things in order: bookmark dialog → menu → fullscreen → player.
- [ ] Mouse back button does the same, from over the video, the control bar and the title bar.
- [ ] Typing in the bookmark label doesn't trigger shortcuts (type "kjf m" into it).
- [ ] Menus:
  - speed, including "Saved for this course" / "Use default"
  - subtitles: external ones labelled `lang · filename`
  - audio (the two-track MKV)
  - chapters (the chaptered MP4; the button stays for the whole video)
- [ ] Sleep timer:
  - 15m and Custom (1 and 600) count down in the chip.
  - "End of lecture" stops autoplay once.
  - Turn off works.
- [ ] Bookmarks:
  - saving pauses the video, and closing the dialog resumes it
  - the saved list jumps to the bookmark
  - deleting works
  - the Bookmarks page shows them, and clicking one plays at that time
- [ ] Course panel: Content and Resources tabs, playing from the panel, ticking resources.
- [ ] Playback info (I) shows live numbers.
- [ ] "Pause on exercises and resources" on: at the end of a lecture with resources, "Before the next lecture" appears, with Stay here / Next lecture.
- [ ] Default speed:
  - Changing it in Settings applies to a course with no saved speed.
  - A course's own saved speed wins until "Use default".
- [ ] Fullscreen (F, double-click):
  - controls stay up unless "Auto-hide controls in fullscreen" is on
  - the wall clock shows
  - the taskbar is hidden behind it
- [ ] Hand-check text sharpness in the control bar over a bright video, at 100% and at 125/150% scaling.
- [ ] `--play "<some video>"` plays a bare file, with no bookmark button or panel.

### Mini player

- [ ] T from windowed, maximized and fullscreen.
  - Esc, T or double-click goes back to exactly the previous state: windowed, maximized or fullscreen.
- [ ] It's always on top. Dragging moves it with no snapping to edges.
- [ ] Resizing keeps the video's shape; the 4:3 and vertical lectures reshape it.
- [ ] Controls appear on hover; the progress line shows at rest.
- [ ] × closes the player. Closing and reopening the app restores the mini player where it was.
- [ ] The resources chip appears with "Pause on exercises and resources".

### Search, history, favourites, bookmarks, tracks, stats

- [ ] Search:
  - title hits play or open the right thing
  - after Settings → Index subtitle text, searching `quokka` / `ornitorrinco` finds the cue and plays at it
  - non-ASCII queries work
- [ ] History groups by Today / Yesterday / date. Resume plays from the saved spot.
- [ ] Favourites lists starred courses.
- [ ] Career tracks:
  - create (Enter submits), edit, delete
  - add courses (the picker stays open), reorder, remove
  - "Up next" badge
  - the library's track filter
- [ ] Stats:
  - goal ring follows Settings → Daily goal
  - streak counts days with 15+ minutes watched
  - heatmap tooltips show minutes
  - "This week" bars
  - Continue opens the course

### Settings and data safety

- [ ] Theme Dark / Light / System. With System, switching Windows' theme follows live.
- [ ] Every switch persists across a restart.
- [ ] Each maintenance button runs and reports: Check, Rebuild, Index, Compact, Clear, Clean.
  - Other buttons are dimmed while one runs.
- [ ] Auto-rescan on: drop a video into the QA course folder. It appears within a few seconds and doesn't disturb a lecture that's playing.
- [ ] **Backup round trip** (scratch data folder):
  - Export, then change things (delete a bookmark, untick a lecture).
  - Import & restart. Everything is back as it was in the backup.
  - A backup made by a newer version is refused with the "newer version" message.

## 7. Windows integration

- [ ] **Mixed-DPI monitors** (for example 100% + 150%, or a laptop with an external screen):
  - [ ] Drag the full window across and back several times. It doesn't jump, resize by itself or bounce back.
  - [ ] Same with the mini player.
  - [ ] Maximize on the second monitor: it maximizes there, not on the first.
  - [ ] Mini player on monitor 2 → back to full: with "Open the full player where the mini player is" on it opens on monitor 2; off, back where it was.
- [ ] **Snap**:
  - Dragging the title bar to the edges and corners snaps.
  - Win+arrows work.
  - Windows 11: hovering maximize shows Snap Layouts, and picking one places the window.
  - The maximize button's hover, click, and press-and-slide-off behave.
  - Resizing from the top edge works just above the maximize button.
- [ ] The maximize/restore glyph is right after Win+↑, double-clicking the title bar and snapping.
- [ ] Caption buttons do nothing under a dialog's dimmed backdrop.
- [ ] **Keep-awake**: with the display sleep set to 1 min, playing keeps the screen on; paused, it sleeps.
- [ ] Sleep/resume the PC mid-lecture. Playback recovers, and Stats doesn't count the time asleep.
- [ ] Two copies launched at once don't corrupt the library. There's no single-instance lock, so this is worth a look after database changes.

## 8. Update rehearsal (minor and major releases, and any change to `updates.rs`)

Follow [releasing.md §4](releasing.md#4-optional-rehearse-the-update) with a local
`latest.json`:

- [ ] Setup install of the previous release:
  - the banner appears about 3 s after launch
  - Update → Downloading N% → Restarting…
  - the new version launches in front
  - data intact
- [ ] MSI install of the previous release: the same, through `msiexec`, and it stays an MSI install afterwards.
- [ ] × on the banner hides it for the session; Settings still offers the update.
- [ ] A broken signature (edit one character in the local `latest.json`) is refused with an error. Nothing is installed.

## 9. Publish and watch

- [ ] Publish the draft and mark it **latest** (releasing.md §5).
- [ ] Within 5 minutes:
  - `https://github.com/NFRohan/Deskemy/releases/latest/download/latest.json` serves the new version.
  - Both installer URLs inside it download.
- [ ] An install of the previous release (left over from §5.1) is offered the update on its next launch.
- [ ] For 48 hours, watch GitHub issues and any posts you shared the release in.

### If a bad release gets out

1. **Stop it spreading.** Edit the previous good release and mark it **latest**.
   - `releases/latest/download/latest.json` then serves the old manifest, so nobody else is offered the bad one.
   - Don't delete the bad release's files while people may still be downloading them.
2. People already on the bad version can't be downgraded by the updater. **Fix forward**: release the fix as the next patch version, never a re-used version number.
3. If data is at risk (the database, progress), say so in the release notes and pin an issue with how to restore from a backup.

## Regression watchlist

Bugs that reached users or were caught late. Check each one every release.
When a new bug escapes, add it here along with the check that would have
caught it.

| Bug (version) | Check |
|---|---|
| No database on a fresh install; the library vanished on restart (2.0.0) | §5.2 restart check |
| Progress jumped back to the start (2.0.x) | Golden path steps 5–7 |
| First launch after installing came up behind other windows (2.0) | §5.1, §5.2 |
| Mini player jumped between mixed-DPI monitors (2.0.0, #8) | §7 mixed-DPI |
| Main window jumped between mixed-DPI monitors (2.0.1, #8) | §7 mixed-DPI |
| Chapters button disappeared mid-video (1.2.2, #6) | Player → chapters, watch 5+ min |
| Bookmark panel lost typing to shortcuts (2.0 UAT) | Player → type in the bookmark label |
| Creating a career track crashed (2.0 UAT) | Tracks → create |
| Default speed applied only to some courses (2.0 UAT) | Player → default speed |
| A course with long names overflowed its layout (2.0 UAT) | §2 sweep + the QA course's long names |
| Grayscale player text looked worse than subpixel (2.0.2 dev) | Player → text sharpness |
| Packaging deleted the previous installers (2.0 dev) | §3 files present |

## Sign-off

Copy this into the release's draft notes or a comment on it before publishing:

```
QA <version> — <date> — <who>
Tier: patch / minor / major
Gate: core ✓  app ✓ (libmpv ran)  clippy ✓  Linux CI ✓
Sweep: ✓ (diffs explained)   Artifacts + sigs: ✓
Installs: upgrade-from-<prev> ✓  fresh setup ✓  MSI ✓/n/a  1.x ✓/n/a  portable ✓
Golden path ✓   Areas: <list>   Windows integration ✓/n/a   Update rehearsal ✓/n/a
Known issues shipped: <none / issue links>
```
