# Linux port

What stands between Deskemy and a Linux release (GH #7): what already works,
the bottlenecks, the issues to fix, and how each is checked. The work happens
on the `linux-port` branch; nothing merges to `main` until the Windows checks
below pass. [roadmap.md](roadmap.md) keeps the summary; this is the detail.

## Ground rules

- **Windows must not regress.** Windows is the shipping platform and works
  well. Each change is one of:
  - **Linux-only:** behind `cfg(target_os = "linux")` or `cfg(not(windows))`.
    Windows builds don't compile it.
  - **Shared:** it changes behaviour on Windows too (marked **Windows: yes**
    below). Real-mpv tests run on Windows locally and on Linux in CI, and
    each change gets a hand check on Windows before it merges.
- **Small slices.** One issue per commit, each with tests where mpv or the
  rule can be tested.
- **Merge back:** shared fixes that are worth having on Windows anyway (the
  seek items) can merge to `main` on their own, after a Windows test build.
  Linux-only work merges when the first Linux beta is ready.
- **No release** until the Fedora VM pass and a test on at least one real
  machine (not a VM) are done.

## Status

- **Done on the branch (draft PR #12):** issues 1 to 12. CI builds and tests
  on Ubuntu 24.04 and builds the AppImage, .deb and .rpm as an artifact.
- **Next:** the [Fedora VM pass](#fedora-vm-pass) with CI's AppImage and a
  source build, then a real machine; then milestone 4 (the beta).
- **Windows:** issues 1, 3 and 4 change it (seeks and the seek bar); they
  need a Windows test build and a hand check before anything merges.

## Where things stand

- **CI is green on Ubuntu 24.04** (`.github/workflows/slint-linux.yml`). It
  builds the app and runs the core tests and the app tests, including the
  ones that play real lectures through a headless mpv 0.37 (end of lecture,
  `keep-open`, subtitles). It renders the UI offscreen with the software
  renderer, so **no video has been drawn on Linux in CI**.
- **Already portable:**
  - the data folder (`~/.local/share/<identifier>`)
  - the libmpv names (`libmpv.so.2`, then `.so.1`, `.so`)
  - the frameless window: resize edges and title-bar drag go through winit,
    on both X11 and Wayland
  - file dialogs: rfd 0.17's default backend is the XDG desktop portal (not
    GTK), with the #10 message when no file chooser is available
  - the one-copy-per-library lock (a file lock)
  - opening resources with the default app
- **Windows-only code is fenced off:** keep-awake, Snap Layouts, the MSI
  check, the mini player's Win32 placement and drag, long paths.
- **A community port runs.** @l0udsil3nc3's fork
  ([Deskemy-linux](https://github.com/l0udsil3nc3/Deskemy-linux),
  `linux-polish`, built from 2.0.2) ships an AppImage built and used on
  Fedora 44 with mpv 0.41 and OpenGL 4.6. It shows the core path works on a
  real desktop: Slint's OpenGL context, mpv drawing into it, the tests
  passing. We don't take its code, but its findings are credited where they
  shaped a fix below.

## Bottlenecks

The risks that decide how hard this is. Most of the work is ordinary; these
are where it can go wrong.

1. **Video through the GPU, across drivers.** mpv renders into Slint's OpenGL
   context, and we pass the X11 or Wayland display so mpv can use VA-API
   hardware decoding. It works on Mesa with OpenGL 4.6 (the fork). Unknown so
   far:
   - NVIDIA's proprietary driver, where VA-API needs `nvidia-vaapi-driver`
     and often falls back to software decoding
   - older OpenGL versions
   - hybrid laptops

   A VM's virtual GPU checks correctness only, not performance or hardware
   decoding. **Needs real machines,** ours or testers'.
2. **Wayland's window model.** Wayland lets an app neither keep its window on
   top nor place it, so the mini player becomes an ordinary small window.
   winit also has no drag & drop on Wayland (only its X11 backend reports
   dropped files), so drag & drop import doesn't work there. Plan: run under
   XWayland by default (issue 6).
3. **Codecs and libraries.** A distro's own libmpv brings that distro's
   ffmpeg. Stock Fedora ships a patent-stripped ffmpeg (`ffmpeg-free`), so
   H.264 courses may not play without RPM Fusion; confirm on the VM. Fedora's
   AMD and Intel hardware decoding may also need RPM Fusion's drivers. Plan:
   the AppImage bundles a libmpv and ffmpeg with full codecs (issue 9),
   as Windows ships `libmpv-2.dll`.
4. **Distro spread.** A binary runs only where glibc is at least as new as
   the one it was built against. The fork builds on Fedora 44, which rules
   out Ubuntu 24.04 and Debian 12. Plan: build on Ubuntu 24.04 first (covers
   Fedora 40+, Ubuntu 24.04+, Mint 22, Debian 13), and an older base with our
   own mpv build later.

## Issues to fix

In the order they'd be done. **Windows: no** means the change is Linux-only
and can't affect the Windows build.

### 1. Relative seeks land on a keyframe (Windows: yes)

- **Problem:** the skip buttons, arrows and `j` / `l` send
  `seek <n> relative`, and mpv lands relative seeks on a keyframe. With long
  keyframe gaps a +10 s skip can land short or barely move, so one click
  sometimes takes two. Seen on Linux by the fork; Windows does it too.
- **Fix:** `seek <n> relative+exact`. Absolute seeks (bar, digits, bookmarks)
  are exact already, by mpv's default.
- **Tests (real mpv):** forward and backward × playing and paused ×
  relative and absolute, each checking `time-pos` lands within 0.1 s of the
  target once the seek finishes. The fork reports two more problems with mpv
  0.41: backward absolute seeks overshooting, and backward seeks while paused
  being dropped. Both are unusual for mpv, so these tests check them on
  Windows and in Linux CI before anything works around them. Any workaround
  must not unpause, even briefly (that leaks audio).

### 2. Resuming fails on mpv older than 0.38 (Windows: no change in practice)

- **Problem:** resuming loads with `loadfile <url> replace 0 start=…`. The
  index argument (`0`) is new in mpv 0.38; 0.37 (Ubuntu 24.04) rejects the
  command with error -4, so a lecture with a saved position **doesn't open
  at all**. The test `no_position_or_leaving_the_player_never_wipes_progress`
  is ignored on Linux for this.
- **Not the fix:** setting `start` as a property before `loadfile`, as the
  roadmap suggested. The fork found that 0.37 ignores it and newer versions
  apply it to the file already playing.
- **Fix:** send the 0.38 form; on error -4 only, send the older form
  (`loadfile <url> replace start=…`) and remember which one this mpv takes,
  so later resumes send one command and log no error. Credit: the fallback
  idea is @l0udsil3nc3's.
- **Windows:** the bundled libmpv is newer than 0.38, so the first form
  always works and nothing changes.
- **Tests:** un-ignore the test on Linux; CI's mpv 0.37 exercises the
  fallback.

### 3. The seek bar moves in steps (Windows: yes)

- **Problem:** the player's thread pushes the position to the UI 5× a second,
  so the bar visibly steps on a 60 Hz screen.
- **Not the fix:** waking the thread 30× a second (the fork). That's six
  times the thread traffic in every lecture, for one number.
- **Fix:** keep the 5× updates, and send with each one the playback speed and
  whether it's playing. The bar works out its own position on every frame
  it draws, from the last reading and the time since (Slint's
  `animation-tick()`), capped at the duration. While a video plays the
  window redraws every video frame anyway, so this costs nothing extra.
  The mini player's progress line uses the same value.
- **Check:** the bar glides at 1× and 2×, stops dead on pause, and never runs
  past the end or backwards between readings.

### 4. The seek bar jumps back after a seek (Windows: yes)

- **Problem:** after a click, the next reading can come before the seek has
  happened, so the bar flicks back to the old position, then forward.
- **Fix:** on any seek (click, keys, digits, bookmarks), the bar goes to the
  target at once, and readings are ignored until mpv says the seek is done
  (`MPV_EVENT_PLAYBACK_RESTART`, already in our bindings). A short timeout
  is only a safety net. The fork's version guesses from "near the target
  and moving", with a 2 s deadline; the event is exact.
- **Check:** click far back, far ahead, and while paused: no flicker. Tests:
  the hold clears on the event, and clears on the timeout when no event
  comes.

### 5. Keep-awake (Windows: no)

- **Problem:** not implemented outside Windows, so the screen dims and locks
  mid-lecture. mpv's own `stop-screensaver` only works for mpv's own window,
  not when it draws into ours.
- **Fix:** while playing, ask the desktop to inhibit idle over D-Bus,
  through the portal's `org.freedesktop.portal.Inhibit` (works in sandboxes
  too) or `org.freedesktop.ScreenSaver.Inhibit`. Release it on pause, at the
  end of a lecture and when the player closes, the same moments as the
  Windows `set_keep_awake`. Needs a D-Bus crate (zbus) as a Linux-only
  dependency.
- **Check:** with a 1-minute screen-off setting, a playing lecture keeps the
  screen on and a paused one lets it dim.

### 6. XWayland by default (Windows: no)

- **Problem:** see bottleneck 2. On native Wayland the mini player can't
  stay on top or be placed, and drag & drop doesn't work.
- **Fix:** start Slint with winit's X11 backend
  (`BackendSelector::with_winit_event_loop_builder` + `with_x11()`) when an
  X server is available, which every mainstream Wayland desktop provides
  through XWayland. That brings back always-on-top, placement, drag & drop
  and window restore. A setting or `DESKEMY_WAYLAND=1` opts into native
  Wayland. It's chosen in code, not by clearing `WAYLAND_DISPLAY`, which the
  apps Deskemy opens (the PDF app, the browser) would inherit.
- **Cost:** with fractional scaling, XWayland apps can look soft on older
  GNOME; GNOME 47+ and KDE handle it better. Compare on the VM.
- **Check:** the mini player stays on top and goes where it's put; drag &
  drop imports; resizing, fullscreen and mouse back work, on both backends.

### 7. A second launch doesn't bring the window forward (Windows: no)

- **Problem:** outside Windows the second copy only prints "already running"
  and exits, so a click on the menu entry seems to do nothing.
- **Fix:** the second launch tells the first one to show itself, through a
  local socket in the data folder; the first restores and focuses its own
  window. Under X11 that works directly. On native Wayland, focusing needs
  an activation token, so the window may only flash in the dock; acceptable.
- **Check:** launch twice with the window minimised, behind another app, and
  on another workspace.

### 8. The PDF viewer has no PDFium on Linux (Windows: no)

- **Problem:** without `libpdfium.so` beside the binary, PDFs open in the
  outside app (the fallback works, so this isn't a blocker).
- **Fix:** a `get-pdfium.sh` that fetches the pinned build (the same
  chromium/7881 non-V8 build as Windows, Linux x64), checks its SHA-256 and
  stages it like `get-pdfium.ps1`. The AppImage ships it.
- **Check:** a PDF opens in Deskemy, from the course page, the Resources
  panel and the end card.

### 9. The AppImage, built in CI (Windows: no)

The fork's recipe (the binary's libraries plus libmpv's, minus glibc,
libstdc++ and the GPU drivers) is the right approach. Ours differs where it
matters:

- **No `LD_LIBRARY_PATH`.** The fork's launcher sets it for the whole
  process, and every app Deskemy starts inherits it: opening a resource
  (`open::that_detached` → `xdg-open`) would start the host's PDF app or
  browser with our bundled libraries, which tends to crash them. Instead:
  - the binary and the bundled libraries find each other through their
    rpath (`$ORIGIN/../lib`, set with patchelf)
  - libmpv's search adds `../lib` beside the exe (it's loaded at run time,
    so the rpath doesn't cover it)
- **Built in CI**, in the Linux workflow, on Ubuntu 24.04 (see
  bottleneck 4), not by hand as root. Ubuntu's libmpv brings a full-codec
  ffmpeg.
- **The static AppImage runtime**, so it runs without libfuse2 installed
  (many distros no longer ship it).
- **Leave to the host only what the AppImage community's excludelist does**
  (glibc, the GPU stack, the core X11 / Wayland / sound-server client
  libraries, fonts), plus libva: host GPU drivers don't load into an older
  libva. Everything else is bundled. A first cut that left every `libX*` to
  the host failed on a fresh Fedora Workstation, which has no libXss or
  libXpresent.
- **Desktop integration:** a `.desktop` entry and icons at 128, 256 and
  512 px.
- **CI uploads it as a build artifact** (not a release) until the beta.

### 10. Updates for the AppImage (Windows: no)

- **Fix:** `package.ps1` signs CI's AppImage with the release key (the key
  never goes to CI), and `latest.json` gets a `linux-x86_64` entry with
  format `appimage`. The app downloads and verifies it like the installers;
  after shutting down it copies it beside the running AppImage and renames
  it over it (never half-written; the name, and so any shortcut, stays),
  then relaunches. Only a copy running as an AppImage (`$APPIMAGE` set) in
  a folder it can write to does this; one built from source, a read-only
  folder, or a future Flatpak gets the release page instead. A failed swap
  leaves the old AppImage, and Deskemy relaunches either way.
- **Releasing:** a Linux section in [releasing.md](releasing.md), and Linux
  checks in [release-qa.md](release-qa.md).
- **Check:** rehearse with `DESKEMY_UPDATE_ENDPOINT`, as on Windows.

### 11. Docs and loose ends

- Correct the roadmap's Linux list: the `start` property route doesn't work
  (issue 2), and rfd's default is the portal, not GTK.
- The Linux CI no longer needs `libgtk-3-dev` (the AppImage job builds
  without it).
- Text: the Skia patch for ClearType text is Windows-only; Linux gets
  greyscale antialiasing, which is normal there. Nothing to do.

### 12. .deb and .rpm (Windows: no)

- **Problem:** an AppImage doesn't install. Downloaded, it isn't executable,
  so a double-click opens it with whatever claims the file type: on Fedora,
  GNOME Disks' image writer, offering to write it to a drive. Made
  executable, it runs but isn't in the app menu.
- **Fix:** `package-deb-rpm.sh` packs the AppImage's AppDir with
  [nfpm](https://nfpm.goreleaser.com) (pinned, checked by its hash): the
  binary and the bundled libraries in `/opt/deskemy`, a `deskemy` command,
  the menu entry and icon in `/usr`. Same bundle as the AppImage, so no
  distro mpv and no codec problem.
  - **Dependencies** are worked out from the bundle: the libraries it links
    to and doesn't carry (the AppImage's host list, as far as they're used;
    not what those link to in turn), and the newest glibc symbol version it
    asks for. The .deb's come from `dpkg-shlibdeps` on the build machine
    (Ubuntu 24.04), kept to those libraries' packages: the oldest version
    each needs by the symbols used, and alternatives (any JACK). The .rpm
    requires their sonames (`libX11.so.6()(64bit)`), which Fedora's and
    openSUSE's packages both provide, and glibc's symbol version. An older
    distro refuses the package instead of installing an app that can't
    start.
  - **AppStream metainfo** (`app/linux/io.github.NFRohan.Deskemy.metainfo.xml`,
    in all three): once installed, software centres show Deskemy by name
    with its icon, screenshots and releases. Before installing, GNOME
    Software's page for a package file shows a generic icon whatever the
    package holds: it reads only the package's header and file list, and
    icons come from AppStream catalogs (a distro's repositories, installed
    apps). Only a repository of our own would change that (see below).
  - **Updates:** a system package can't replace itself, and has no
    `$APPIMAGE`, so it gets the release page (issue 10): the new package
    opens in the software installer over the old one. No APT / DNF
    repository of our own: its signing key and hosting would be a security
    responsibility of their own, for every user's system updates.
- **Check:** install, run and remove in clean Ubuntu 24.04, Debian 13,
  Fedora and openSUSE Tumbleweed containers; Ubuntu 22.04 and Debian 12
  refuse it. Then the VM pass from the .rpm.

## Release formats

- **AppImage first.** One file that runs on most distros, bundles the codecs
  and PDFium, and updates itself.
- **Flatpak (Flathub) next,** once the AppImage is stable. Linux users often
  prefer it, the platform provides full codecs, and Flathub handles updates.
  The work is the sandbox: folders picked through the portal come back as
  temporary paths, which would break saved library paths, so Deskemy needs
  home and removable-drive access, and Flathub's reviewers will ask why.
- **.deb and .rpm beside it** (issue 12): the AppImage's bundle, installed,
  in the app menu, updated from the release page. They'd bring back the
  codec problem only if they used the distro's mpv; they don't.
- **Not now:** Snap. An AUR package is for the community if they want one.

## Fedora VM pass

Before and after the fixes, on the branch:

1. **Enable 3D acceleration in the VM** (virtio-gpu with OpenGL). Without it,
   OpenGL is software-only: it still works, but slowly.
2. **Build from source:**
   - Install the build packages: libmpv's development files,
     `fontconfig-devel`, `libxkbcommon-devel` and `wayland-devel`. The libmpv
     package name is to confirm (`mpv-libs-devel` is likely).
   - Install Rust with rustup.
   - Run `cargo build --release` in `app/`.
3. **Playback:**
   - Play an H.264 course. If it fails, that's bottleneck 3.
   - The log names the OpenGL renderer and the decoder.
   - `DESKEMY_HWDEC=no` forces software decoding, to compare.
4. **Wayland vs X11:** run once on native Wayland and once under XWayland.
   Each time, compare:
   - the mini player
   - drag & drop
   - resize, fullscreen and the mouse back button
   - sharpness with fractional scaling
5. **The rest:**
   - Add Folder, Locate folder, a cover image, backup export / import
   - opening a resource, the PDF viewer
   - a second launch
   - the screen dimming during a lecture
6. **The AppImage** from CI, once issue 9 lands: the same checks, plus
   opening a resource. The host's PDF app must start normally.

## Milestones

1. **Seek fixes (1, 3, 4) and resume (2).** Shared code; Windows test build
   and hand check; can merge to `main` on their own, for 2.1.1 if wanted.
2. **Linux behaviour (5, 6, 7, 8).** Linux-only; CI plus the VM pass.
3. **AppImage and updates (9, 10).** CI artifact, VM pass, then a real
   machine.
4. **Linux beta:** an AppImage, .deb and .rpm published beside a Windows
   release, marked as a beta in its notes, with `latest.json` updating the
   AppImage.
5. **Flatpak.**
