# Releasing (Windows)

The Slint app (`app/`) is packaged with [cargo-packager](https://github.com/crabnebula-dev/cargo-packager)
(`cargo install cargo-packager --locked`); its config is `[package.metadata.packager]`
in `app/Cargo.toml`. The installer matches the Tauri app's — per-user NSIS into
`%LOCALAPPDATA%\Deskemy`, product "Deskemy", publisher "Spooksy", binary
`deskemy.exe` — so it updates an existing install (Tauri or Slint) in place.

## 1. Version

Set `version` in `app/Cargo.toml`. It's shown in Settings → About, compared by
the updater, and written into the installer and `latest.json`.

## 2. Build, sign, package

Signing uses the same minisign key as the Tauri releases (the one in
`TAURI_SIGNING_PRIVATE_KEY`); the app trusts its public key (`updates::PUBKEY`,
the same as `src-tauri/tauri.conf.json`).

```powershell
$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY = "<the key, or a path to the key file>"
$env:CARGO_PACKAGER_SIGN_PRIVATE_KEY_PASSWORD = "<its password>"
powershell -File app/scripts/package.ps1 -Notes "What's new in this release"
```

Into `app/target/packages/`:

| File | |
|---|---|
| `deskemy_<v>_x64-setup.exe` | the installer |
| `deskemy_<v>_x64-setup.exe.sig` | its signature |
| `Deskemy_<v>_x64-portable.zip` | portable build (`.portable` marker; data in `data/` beside it) |
| `latest.json` | the update manifest |
| `SHA256SUMS.txt` | checksums of the installer and zip |

Without a key the installer and zip still build, but there's no `.sig` or
`latest.json` — so no auto-update.

## 3. Check the signature

Before publishing, confirm the installer verifies against the key the app
trusts (a wrong or re-generated key would strand every user):

```powershell
$env:DESKEMY_DATA_DIR = "$env:TEMP\deskemy-scratch"   # never touches your library
app\target\release\deskemy.exe --verify-update app\target\packages\deskemy_<v>_x64-setup.exe app\target\packages\deskemy_<v>_x64-setup.exe.sig
```

It prints `OK: … is signed with the release key.`, or `NOT VERIFIED: …` and
exits 1.

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

This really installs the build — do it on a machine where that's fine.

## 5. Publish

Create a GitHub release tagged `v<version>` with the installer, the portable
zip, `SHA256SUMS.txt` and `latest.json` (the `.sig` is inside `latest.json`; attaching it is
optional). Marking it **latest** is what updates people: both the Tauri 1.x
updater and this app read
`https://github.com/NFRohan/Deskemy/releases/latest/download/latest.json`.
Don't mark a release "latest" until it's ready for everyone.

## 2.0.0 notes (Tauri → Slint)

- Tauri 1.2.x installs update through their own updater: it downloads the new
  NSIS installer and runs it passively; it replaces the files in
  `%LOCALAPPDATA%\Deskemy` and relaunches the Slint build.
- The first launch migrates the database to schema v8 (resource ticks). An older
  build still opens it, ignoring the new table; a backup exported from 2.0 won't
  import into 1.x.
- No MSI in 2.0 (the updater installs the NSIS build). Anyone who installed a
  1.x MSI keeps that separate install; they should uninstall it and use the
  NSIS installer.
- No WebView2 needed any more.
