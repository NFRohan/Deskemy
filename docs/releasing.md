# Releasing (Windows)

Releases are Windows-only for now. CI builds the Linux AppImage, .deb and
.rpm (`.github/workflows/slint-linux.yml`, the `deskemy-linux` artifact,
unsigned) but publishes nothing. It ships with a release once the Linux app
has been reviewed and tested ([linux-port.md](linux-port.md)); then see
[Linux](#linux). Until then `latest.json` has no Linux
entry, so no Linux copy is offered an update.

The Slint app (`app/`) is packaged with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(`cargo install cargo-packager --locked`); its config is `[package.metadata.packager]`
in `app/Cargo.toml`. Both installers match the Tauri app's, so each updates an
existing install of its kind (Tauri or Slint) in place:

- **Setup (NSIS)**: per-user into `%LOCALAPPDATA%\Deskemy`, product "Deskemy",
  publisher "Spooksy", binary `deskemy.exe`.
- **MSI (WiX)**: per-machine into `Program Files\Deskemy`, from
  `app/installer/main.wxs`. That's cargo-packager's template with the Tauri
  MSI's upgrade code `{12314604-E9F6-5440-BB59-4983D4418E47}` pinned
  (cargo-packager's own would differ, and the MSI would install beside the old
  one instead of replacing it) and Tauri's `AUTOLAUNCHAPP` relaunch. Never
  change that code.

## 1. Version

Set `version` in `app/Cargo.toml`. It's shown in Settings → About, compared by
the updater, and written into the installer and `latest.json`.

## 2. Build, sign, package

Signing uses the same minisign key as the Tauri 1.x releases; the app trusts
its public key (`updates::PUBKEY`, the one 1.x's `tauri.conf.json` carried),
so 1.x installs accept 2.x updates too.

```powershell
$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY = "<the key, or a path to the key file>"
$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD = "<its password>"
powershell -File app/scripts/package.ps1 -Notes "What's new in this release"
```

`-Notes` is the one-line summary that goes into `latest.json` (what an
update prompt can show); the full notes, for the GitHub release, live in
`docs/release-notes/<version>.md`.

Into `app/target/packages/`:

| File | |
|---|---|
| `deskemy_<v>_x64-setup.exe` | the per-user installer |
| `deskemy_<v>_x64_en-US.msi` | the per-machine installer |
| `*.sig` | their signatures |
| `Deskemy_<v>_x64-portable.zip` | portable build (`.portable` marker; data in `data/` beside it) |
| `latest.json` | the update manifest |
| `SHA256SUMS.txt` | checksums of the installers and zip |

`latest.json` has two entries: `windows-x86_64-msi` (the MSI) for MSI installs
and `windows-x86_64` (the setup) for the rest. The Tauri 1.x updater picks by
the bundle type stamped into its exe; this app asks Windows Installer whether
an MSI owns its folder. Neither falls back to the other kind.

Without a key the installers and zip still build, but there are no `.sig`s or
`latest.json`, so no auto-update.

## 3. Check the signatures

Before publishing, confirm both installers verify against the key the app
trusts (a wrong or re-generated key would strand every user):

```powershell
$env:DESKEMY_DATA_DIR = "$env:TEMP\deskemy-scratch"   # never touches your library
# "| Write-Output": deskemy.exe is a GUI program, so PowerShell would neither
# wait for it nor show what it prints.
app\target\release\deskemy.exe --verify-update app\target\packages\deskemy_<v>_x64-setup.exe app\target\packages\deskemy_<v>_x64-setup.exe.sig | Write-Output
app\target\release\deskemy.exe --verify-update app\target\packages\deskemy_<v>_x64_en-US.msi app\target\packages\deskemy_<v>_x64_en-US.msi.sig | Write-Output
```

It prints `OK: … is signed with the release key.`, or `NOT VERIFIED: …` and
exits 1. No output at all means the `| Write-Output` was left off. That's not
a pass.

## 4. Optional: rehearse the update

`DESKEMY_UPDATE_ENDPOINT` points the app at another manifest. To rehearse the
whole flow (check → banner → download → verify → restart → install) without
publishing, serve `app/target/packages` locally with a copy of `latest.json`
whose `url` points at the local installer and whose `version` is higher than
the installed build, then start the installed app with the variable set:

```powershell
cd app\target\packages; python -m http.server 8765   # in one terminal
$env:DESKEMY_UPDATE_ENDPOINT = "http://127.0.0.1:8765/latest-local.json"
& "$env:LOCALAPPDATA\Deskemy\deskemy.exe"
```

This really installs the build — do it on a machine where that's fine. To
rehearse the MSI path, install the previous MSI first (per-machine; remove it
after from Apps & features).

## 5. Publish

Not before [release-qa.md](release-qa.md) has been run on these exact files
and signed off: it's the go / no-go for every release.

Create a GitHub release tagged `v<version>` with both installers, the portable
zip, `SHA256SUMS.txt` and `latest.json` (the `.sig`s are inside `latest.json`;
attaching them is optional). Keep the file names as built: `latest.json`
points at them. Marking it **latest** is what updates people: both the Tauri 1.x
updater and this app read
`https://github.com/NFRohan/Deskemy/releases/latest/download/latest.json`.
Don't mark a release "latest" until it's ready for everyone.

## Linux

The AppImage, .deb and .rpm are built by CI, not on Windows; the AppImage
is signed here with the same key: the key never goes to CI.

1. **Fetch them** from the Linux workflow's run on the release commit, into
   the packages folder, before step 2:

   ```powershell
   gh run download <run id> -n deskemy-linux -D app/target/packages
   ```

   They're `Deskemy_<v>_x86_64.AppImage`, `deskemy_<v>_amd64.deb` and
   `deskemy_<v>_x86_64.rpm`, their version from the same commit's
   `app/Cargo.toml`. `package.ps1` only picks up this version's.
2. **Step 2 as usual.** `package.ps1` signs the AppImage, adds a
   `linux-x86_64` entry (format `appimage`) to `latest.json` and lists all
   three in `SHA256SUMS.txt`.
3. **Check its signature** as the installers' (step 3):
   `deskemy.exe --verify-update` takes any file.
4. **Publish** with the release: the three beside the Windows files (the
   AppImage's `.sig` is in `latest.json`).

A copy running from the AppImage updates itself: it downloads the new one,
puts it in place of the running file (same name) and relaunches from it.
One installed from the .deb or .rpm is pointed at the release page; the new
package installs over it.

## 2.0.0 notes (Tauri → Slint)

- Tauri 1.2.x installs update through their own updater, each with its own
  kind. A setup install downloads the new setup and runs it passively, which
  replaces the files in `%LOCALAPPDATA%\Deskemy`. An MSI install (its exe is
  stamped `MSI`) reads `windows-x86_64-msi` and runs
  `msiexec /i … /passive AUTOLAUNCHAPP=True`, a major upgrade that removes
  1.x from Program Files first. Both relaunch into the Slint build.
- The first launch migrates the database to schema v8 (resource ticks). An older
  build still opens it, ignoring the new table; a backup exported from 2.0 won't
  import into 1.x.
- No WebView2 needed any more.
