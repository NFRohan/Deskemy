//! In-app updates, as the Tauri app does them: check on launch, show a banner
//! and a line in Settings → About, and only download / install when the user
//! opts in. Reads the same signed `latest.json` release asset as the Tauri
//! app's updater (plus the `format` field cargo-packager's updater needs),
//! verified with the same key.
//!
//! The installer is downloaded and verified in the background, then the app
//! shuts down cleanly (saving playback) and `main` hands over to it; it
//! relaunches Deskemy when done. A portable copy can't replace its own files,
//! so it's pointed at the release page instead.
//!
//! An MSI install updates with the MSI (the manifest's `windows-x86_64-msi`,
//! as the Tauri updater does), the NSIS one with the setup: each replaces its
//! own kind in place, where the other would install a second copy beside it.

use crate::{AppWindow, Updates};
use cargo_packager_updater::{semver::Version, Config, Update, UpdateFormat, UpdaterBuilder, WindowsConfig};
use slint::ComponentHandle;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

/// The release asset both the Tauri app and this one read.
pub const ENDPOINT: &str = "https://github.com/NFRohan/Deskemy/releases/latest/download/latest.json";
/// The minisign public key releases are signed with (the Tauri app's).
pub const PUBKEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDM1NEM0QUJGNTNGQjdENTMKUldSVGZmdFR2MHBNTmZ4aUZYOFNxOXZKT0pTS0ZUbTdmUElvZkNGOFZuNzI1SFRjYTBWTzFqTnYK";
pub const RELEASES_URL: &str = "https://github.com/NFRohan/Deskemy/releases/latest";
/// NSIS: passive install (a progress bar, no questions), then relaunch.
pub const INSTALLER_ARGS: [&str; 2] = ["/P", "/R"];
/// msiexec, as the Tauri updater runs it: passive, then relaunch
/// (`AUTOLAUNCHAPP`, see installer/main.wxs).
pub const MSI_ARGS: [&str; 3] = ["/passive", "/promptrestart", "AUTOLAUNCHAPP=True"];
/// The Tauri MSI's upgrade code, which ours keeps (installer/main.wxs).
pub const MSI_UPGRADE_CODE: &str = "{12314604-E9F6-5440-BB59-4983D4418E47}";
/// The manifest entry MSI installs read.
pub const MSI_TARGET: &str = "windows-x86_64-msi";

/// Where downloaded installers wait; swept on the next start.
pub fn download_dir() -> PathBuf {
    std::env::temp_dir().join("deskemy-update")
}

/// Delete installers a previous update left behind. Best-effort: one still
/// running is locked and stays until the next start.
pub fn sweep() {
    let dir = download_dir();
    if dir.exists() && std::fs::remove_dir_all(&dir).is_ok() {
        tracing::info!("swept the previous update's installer");
    }
}

/// The update endpoint: the release asset, or DESKEMY_UPDATE_ENDPOINT (to
/// try the flow against a test manifest).
fn endpoint() -> String {
    std::env::var("DESKEMY_UPDATE_ENDPOINT").unwrap_or_else(|_| ENDPOINT.to_string())
}

/// Ask the endpoint for a version newer than this build. None when up to date.
pub fn check() -> Result<Option<Update>, String> {
    check_with(&endpoint(), PUBKEY, env!("CARGO_PKG_VERSION"), target())
}

/// The manifest entry for this install: `windows-x86_64-msi` for an MSI
/// install, else the platform's default (`windows-x86_64`, the setup). An MSI
/// install never falls back to the setup: that would install a second copy.
pub fn target() -> Option<&'static str> {
    static MSI: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    MSI.get_or_init(installed_by_msi).then_some(MSI_TARGET)
}

/// Whether the MSI installed this copy: a Deskemy MSI product (by upgrade
/// code) is installed in the exe's folder.
#[cfg(windows)]
pub fn installed_by_msi() -> bool {
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiEnumRelatedProductsW, MsiGetProductInfoW, INSTALLPROPERTY_INSTALLLOCATION,
    };
    const ERROR_MORE_DATA: u32 = 234;
    let Some(dir) = std::env::current_exe().ok().and_then(|exe| exe.parent().map(Path::to_path_buf)) else {
        return false;
    };
    let code: Vec<u16> = MSI_UPGRADE_CODE.encode_utf16().chain([0]).collect();
    let products = (0..).map_while(|i| {
        let mut product = [0u16; 39]; // a GUID and its terminator
        // SAFETY: `code` is NUL-terminated; `product` holds the 39 chars the API writes.
        let found = unsafe { MsiEnumRelatedProductsW(code.as_ptr(), 0, i, product.as_mut_ptr()) } == 0;
        found.then_some(product)
    });
    let location = |product: &[u16; 39]| -> Option<String> {
        let mut buf = vec![0u16; 260];
        loop {
            let mut len = buf.len() as u32;
            // SAFETY: `product` is NUL-terminated; `len` is `buf`'s size in chars.
            let rc = unsafe {
                MsiGetProductInfoW(product.as_ptr(), INSTALLPROPERTY_INSTALLLOCATION, buf.as_mut_ptr(), &mut len)
            };
            match rc {
                0 => return Some(String::from_utf16_lossy(&buf[..len as usize])),
                ERROR_MORE_DATA => buf.resize(len as usize + 1, 0),
                _ => return None,
            }
        }
    };
    let found = products.filter_map(|p| location(&p)).any(|loc| !loc.is_empty() && same_dir(Path::new(&loc), &dir));
    tracing::info!(msi = found, "install kind");
    found
}

#[cfg(not(windows))]
pub fn installed_by_msi() -> bool {
    false
}

/// Two folders are the same, ignoring case and a trailing separator.
fn same_dir(a: &Path, b: &Path) -> bool {
    let norm = |p: &Path| {
        let p = std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        p.to_string_lossy().trim_end_matches(['\\', '/']).to_lowercase()
    };
    norm(a) == norm(b)
}

/// `check` against a given endpoint and key, as `current`, reading the
/// manifest's `target` entry (None: the platform's default).
pub fn check_with(endpoint: &str, pubkey: &str, current: &str, target: Option<&str>) -> Result<Option<Update>, String> {
    let url = endpoint.parse().map_err(|e| format!("update endpoint: {e}"))?;
    let current = Version::parse(current).map_err(|e| e.to_string())?;
    let config = Config {
        endpoints: vec![url],
        pubkey: pubkey.to_string(),
        windows: Some(WindowsConfig { installer_args: None, install_mode: None }),
    };
    let mut builder = UpdaterBuilder::new(current, config).timeout(std::time::Duration::from_secs(30));
    if let Some(target) = target {
        builder = builder.target(target);
    }
    let updater = builder.build().map_err(|e| e.to_string())?;
    updater.check().map_err(|e| e.to_string())
}

/// Download (verifying the signature) and save the installer.
pub fn download(update: &Update, progress: impl Fn(u64, Option<u64>)) -> Result<PathBuf, String> {
    if !matches!(update.format, UpdateFormat::Nsis | UpdateFormat::Wix) {
        return Err(format!("this build installs .exe or .msi updates, not {}", update.format));
    }
    let received = std::cell::Cell::new(0u64);
    let bytes = update
        .download_extended(
            |chunk, total| {
                received.set(received.get() + chunk as u64);
                progress(received.get(), total);
            },
            || {},
        )
        .map_err(|e| e.to_string())?;
    let dir = download_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join(installer_name(&update.version, update.format));
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    Ok(path)
}

/// "Deskemy_2.1.0_x64-setup.exe" / "Deskemy_2.1.0_x64_en-US.msi" — as the
/// packager names them.
pub fn installer_name(version: &str, format: UpdateFormat) -> String {
    let version = version.trim_start_matches('v');
    match format {
        UpdateFormat::Wix => format!("Deskemy_{version}_x64_en-US.msi"),
        _ => format!("Deskemy_{version}_x64-setup.exe"),
    }
}

/// Verify `data` against a signature in the updater's form (base64-wrapped
/// minisign), with a public key in the same form.
pub fn verify(data: &[u8], signature: &str, pubkey: &str) -> Result<(), String> {
    use base64::Engine;
    let text = |b64: &str| -> Result<String, String> {
        let bytes = base64::engine::general_purpose::STANDARD.decode(b64.trim()).map_err(|e| e.to_string())?;
        String::from_utf8(bytes).map_err(|e| e.to_string())
    };
    let key = minisign_verify::PublicKey::decode(&text(pubkey)?).map_err(|e| format!("public key: {e}"))?;
    let sig = minisign_verify::Signature::decode(&text(signature)?).map_err(|e| format!("signature: {e}"))?;
    key.verify(data, &sig, true).map_err(|e| e.to_string())
}

/// `deskemy --verify-update <installer> <installer.sig>`: check a signed
/// installer against the key this build trusts, before publishing it.
pub fn verify_file(installer: &Path, signature: &Path) -> Result<(), String> {
    let data = std::fs::read(installer).map_err(|e| format!("{}: {e}", installer.display()))?;
    let sig = std::fs::read_to_string(signature).map_err(|e| format!("{}: {e}", signature.display()))?;
    verify(&data, &sig, PUBKEY)
}

/// A download's progress, 0–100 (0 while the size is unknown).
pub fn percent(received: u64, total: Option<u64>) -> i32 {
    match total {
        Some(total) if total > 0 => ((received.min(total) * 100) / total) as i32,
        _ => 0,
    }
}

/// Run a downloaded installer. Called by `main` after the app has shut down.
pub fn run_installer(path: &Path) -> std::io::Result<()> {
    let is_msi = path.extension().is_some_and(|e| e.eq_ignore_ascii_case("msi"));
    let mut command = if is_msi {
        let mut msiexec = std::process::Command::new("msiexec.exe");
        msiexec.arg("/i").arg(path).args(MSI_ARGS);
        msiexec
    } else {
        let mut setup = std::process::Command::new(path);
        setup.args(INSTALLER_ARGS);
        setup
    };
    command.spawn().map(|_| ())
}

/// The update flow: the update found by the last check.
pub struct Updater {
    update: Arc<Mutex<Option<Update>>>,
}

impl Updater {
    pub fn new() -> Rc<Self> {
        Rc::new(Updater { update: Arc::new(Mutex::new(None)) })
    }

    /// A verified installer to run once the app has shut down.
    pub fn pending_installer(&self) -> Option<PathBuf> {
        PENDING.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Check in the background. `quiet` (the launch check) only speaks up when
    /// an update exists; a manual check also reports "up to date" and errors.
    pub fn check(&self, ui: &AppWindow, quiet: bool) {
        let updates = ui.global::<Updates>();
        if matches!(updates.get_status().as_str(), "checking" | "downloading") {
            return;
        }
        if !quiet {
            updates.set_status("checking".into());
        }
        let (slot, weak) = (self.update.clone(), ui.as_weak());
        std::thread::spawn(move || {
            let result = check();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let updates = ui.global::<Updates>();
                match result {
                    Ok(Some(update)) => {
                        tracing::info!(version = %update.version, "update available");
                        updates.set_version(update.version.trim_start_matches('v').into());
                        updates.set_status("available".into());
                        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(update);
                    }
                    Ok(None) if !quiet => updates.set_status("latest".into()),
                    Err(e) if !quiet => {
                        tracing::warn!(error = %e, "update check");
                        updates.set_error(e.into());
                        updates.set_status("error".into());
                    }
                    Ok(None) => updates.set_status("".into()),
                    Err(e) => {
                        tracing::info!(error = %e, "launch update check");
                        updates.set_status("".into());
                    }
                }
            });
        });
    }

    /// Download and verify the update, then restart into its installer.
    pub fn install(&self, ui: &AppWindow) {
        if deskemy_core::paths::portable_data_dir().is_some() {
            if let Err(e) = open::that_detached(RELEASES_URL) {
                tracing::warn!(error = %e, "open the release page");
            }
            return;
        }
        let Some(update) = self.update.lock().unwrap_or_else(|e| e.into_inner()).clone() else { return };
        let updates = ui.global::<Updates>();
        updates.set_status("downloading".into());
        updates.set_progress(0);
        let weak = ui.as_weak();
        std::thread::spawn(move || {
            let progress_weak = weak.clone();
            let result = download(&update, move |received, total| {
                let (weak, pct) = (progress_weak.clone(), percent(received, total));
                let _ = slint::invoke_from_event_loop(move || {
                    if let Some(ui) = weak.upgrade() {
                        ui.global::<Updates>().set_progress(pct);
                    }
                });
            });
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = weak.upgrade() else { return };
                let updates = ui.global::<Updates>();
                match result {
                    Ok(path) => {
                        tracing::info!(installer = %path.display(), "update downloaded and verified");
                        updates.set_status("restarting".into());
                        *PENDING.lock().unwrap_or_else(|e| e.into_inner()) = Some(path);
                        // Hiding the only window ends the event loop; `main` saves
                        // and shuts down, then runs the installer.
                        let _ = ui.hide();
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "update download");
                        updates.set_error(e.into());
                        updates.set_status("error".into());
                    }
                }
            });
        });
    }
}

/// The verified installer to run after shutdown (set on the UI thread).
static PENDING: Mutex<Option<PathBuf>> = Mutex::new(None);

#[cfg(test)]
mod tests {
    use super::*;
    use cargo_packager_updater::RemoteRelease;

    /// The manifest the release publishes: Tauri's latest.json plus `format`.
    const MANIFEST: &str = r#"{
        "version": "2.0.0",
        "notes": "Deskemy 2.0",
        "pub_date": "2026-10-01T12:00:00Z",
        "platforms": {
            "windows-x86_64": {
                "signature": "c2lnbmF0dXJl",
                "url": "https://github.com/NFRohan/Deskemy/releases/download/v2.0.0/Deskemy_2.0.0_x64-setup.exe",
                "format": "nsis"
            },
            "windows-x86_64-msi": {
                "signature": "c2lnbmF0dXJl",
                "url": "https://github.com/NFRohan/Deskemy/releases/download/v2.0.0/Deskemy_2.0.0_x64_en-US.msi",
                "format": "wix"
            }
        }
    }"#;

    #[test]
    fn reads_the_release_manifest() {
        let release: RemoteRelease = serde_json::from_str(MANIFEST).unwrap();
        assert_eq!(release.version.to_string(), "2.0.0");
        assert!(release.download_url("windows-x86_64").unwrap().as_str().ends_with("Deskemy_2.0.0_x64-setup.exe"));
        assert_eq!(release.format("windows-x86_64").unwrap().to_string(), "nsis");
        assert!(release.download_url(MSI_TARGET).unwrap().as_str().ends_with("Deskemy_2.0.0_x64_en-US.msi"));
        assert_eq!(release.format(MSI_TARGET).unwrap().to_string(), "wix");
        assert!(release.download_url("linux-x86_64").is_err());
    }

    #[test]
    fn the_key_is_a_minisign_public_key() {
        use base64::Engine;
        let text = base64::engine::general_purpose::STANDARD.decode(PUBKEY).unwrap();
        assert!(String::from_utf8(text).unwrap().starts_with("untrusted comment: minisign public key"));
    }

    /// Serve responses over HTTP on localhost: `routes` gets the base URL and
    /// returns (path, body) pairs; each connection gets the matching body.
    fn serve(routes: impl FnOnce(&str) -> Vec<(&'static str, Vec<u8>)>) -> String {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let responses = routes(&base);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(&stream);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                // Drain the headers.
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).map_or(true, |n| n <= 2) {
                        break;
                    }
                }
                let path = request.split_whitespace().nth(1).unwrap_or("/").to_string();
                let body = responses.iter().find(|(p, _)| *p == path).map(|(_, b)| b.clone()).unwrap_or_default();
                let mut out = &stream;
                let _ = write!(out, "HTTP/1.1 200 OK
Content-Length: {}
Connection: close

", body.len());
                let _ = out.write_all(&body);
            }
        });
        base
    }

    /// A throwaway minisign key: (public key, signature of `data`), both in the
    /// updater's base64-wrapped form.
    fn sign(data: &[u8]) -> (String, String) {
        use base64::Engine;
        let b64 = |s: String| base64::engine::general_purpose::STANDARD.encode(s);
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let pk = keys.pk.to_box().unwrap().to_string();
        let sig = minisign::sign(Some(&keys.pk), &keys.sk, std::io::Cursor::new(data), None, None).unwrap();
        (b64(pk), b64(sig.to_string()))
    }

    fn manifest(base: &str, version: &str, signature: &str) -> Vec<u8> {
        format!(
            r#"{{"version":"{version}","platforms":{{"windows-x86_64":{{"signature":"{signature}","url":"{base}/setup.exe","format":"nsis"}},"windows-x86_64-msi":{{"signature":"{signature}","url":"{base}/deskemy.msi","format":"wix"}}}}}}"#
        )
        .into_bytes()
    }

    #[test]
    fn checks_downloads_and_verifies_an_update() {
        let installer = b"MZ pretend installer".to_vec();
        let (pubkey, signature) = sign(&installer);
        let base = serve(|base| {
            vec![("/latest.json", manifest(base, "9.9.9", &signature)), ("/setup.exe", installer.clone())]
        });
        let endpoint = format!("{base}/latest.json");

        // Newer than this build: offered, downloaded, verified, saved.
        let update = check_with(&endpoint, &pubkey, "2.0.0", None).unwrap().expect("an update");
        assert_eq!(update.version, "9.9.9");
        let seen = std::cell::Cell::new(0);
        let path = download(&update, |received, _| seen.set(received)).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), installer);
        assert_eq!(path.file_name().unwrap(), "Deskemy_9.9.9_x64-setup.exe");
        assert_eq!(seen.get(), installer.len() as u64);
        let _ = std::fs::remove_file(&path);

        // Already on it (or newer): nothing to offer.
        assert!(check_with(&endpoint, &pubkey, "9.9.9", None).unwrap().is_none());
    }

    #[test]
    fn an_msi_install_gets_the_msi() {
        let msi = b"pretend msi".to_vec();
        let (pubkey, signature) = sign(&msi);
        let base = serve(|base| vec![("/latest.json", manifest(base, "9.9.9", &signature)), ("/deskemy.msi", msi.clone())]);
        let endpoint = format!("{base}/latest.json");
        let update = check_with(&endpoint, &pubkey, "2.0.0", Some(MSI_TARGET)).unwrap().expect("an update");
        assert!(update.download_url.as_str().ends_with("/deskemy.msi"));
        let path = download(&update, |_, _| {}).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), msi);
        assert_eq!(path.file_name().unwrap(), "Deskemy_9.9.9_x64_en-US.msi");
        let _ = std::fs::remove_file(&path);

        // A manifest without an MSI entry offers nothing, rather than the setup.
        let nsis_only = format!(
            r#"{{"version":"9.9.9","platforms":{{"windows-x86_64":{{"signature":"{signature}","url":"{base}/setup.exe","format":"nsis"}}}}}}"#
        );
        let base = serve(move |_| vec![("/latest.json", nsis_only.into_bytes())]);
        assert!(check_with(&format!("{base}/latest.json"), &pubkey, "2.0.0", Some(MSI_TARGET)).is_err());
    }

    #[test]
    fn same_dir_ignores_case_and_trailing_separators() {
        let dir = tempfile::tempdir().unwrap();
        let upper = dir.path().to_string_lossy().to_uppercase();
        assert!(same_dir(Path::new(&format!("{upper}\\")), dir.path()));
        assert!(!same_dir(&dir.path().join("other"), dir.path()));
    }

    #[test]
    fn this_test_binary_is_not_an_msi_install() {
        assert!(!installed_by_msi());
    }

    #[test]
    fn a_tampered_installer_is_refused() {
        let installer = b"MZ pretend installer".to_vec();
        let (pubkey, signature) = sign(&installer);
        let base = serve(|base| {
            vec![("/latest.json", manifest(base, "9.9.9", &signature)), ("/setup.exe", b"MZ something else".to_vec())]
        });
        let update = check_with(&format!("{base}/latest.json"), &pubkey, "2.0.0", None).unwrap().unwrap();
        assert!(download(&update, |_, _| {}).is_err(), "the signature doesn't match");
        // And a different key refuses a genuine one.
        let (other_key, _) = sign(b"x");
        let base = serve(|base| {
            vec![("/latest.json", manifest(base, "9.9.9", &signature)), ("/setup.exe", installer.clone())]
        });
        let update = check_with(&format!("{base}/latest.json"), &other_key, "2.0.0", None).unwrap().unwrap();
        assert!(download(&update, |_, _| {}).is_err());
    }

    #[test]
    fn verifies_signatures_locally() {
        let (pubkey, signature) = sign(b"installer bytes");
        assert!(verify(b"installer bytes", &signature, &pubkey).is_ok());
        assert!(verify(b"other bytes", &signature, &pubkey).is_err());
        assert!(verify(b"installer bytes", &signature, PUBKEY).is_err(), "not signed with the release key");
    }

    /// With DESKEMY_TEST_INSTALLER / _SIG / _PUBKEY set (a cargo-packager
    /// signed installer and its key's .pub file), check they verify.
    #[test]
    fn packager_signatures_verify() {
        let (Ok(installer), Ok(sig), Ok(pubkey)) = (
            std::env::var("DESKEMY_TEST_INSTALLER"),
            std::env::var("DESKEMY_TEST_SIG"),
            std::env::var("DESKEMY_TEST_PUBKEY"),
        ) else {
            return;
        };
        let data = std::fs::read(installer).unwrap();
        let sig = std::fs::read_to_string(sig).unwrap();
        let pubkey = std::fs::read_to_string(pubkey).unwrap();
        verify(&data, &sig, &pubkey).unwrap();
    }

    #[test]
    fn names_and_progress() {
        assert_eq!(installer_name("v2.1.0", UpdateFormat::Nsis), "Deskemy_2.1.0_x64-setup.exe");
        assert_eq!(installer_name("2.1.0", UpdateFormat::Wix), "Deskemy_2.1.0_x64_en-US.msi");
        assert_eq!(percent(50, Some(200)), 25);
        assert_eq!(percent(500, Some(200)), 100);
        assert_eq!(percent(50, None), 0);
    }
}
