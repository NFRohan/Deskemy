use std::{env, fs, path::PathBuf};

fn main() {
    slint_build::compile("ui/app.slint").expect("compile .slint UI");

    // Stage libmpv next to the built executable (Windows searches the exe's
    // own directory first). The DLL is too large for git; drop a copy in
    // app/vendor/, or reuse the one the Tauri app keeps in src-tauri/vendor/.
    #[cfg(target_os = "windows")]
    {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let sources = [
            manifest.join("vendor").join("libmpv-2.dll"),
            manifest.join("..").join("src-tauri").join("vendor").join("libmpv-2.dll"),
        ];
        for src in &sources {
            println!("cargo:rerun-if-changed={}", src.display());
        }
        if let Some(src) = sources.iter().find(|p| p.exists()) {
            let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
            let dest_dir = manifest.join("target").join(&profile);
            let _ = fs::create_dir_all(&dest_dir);
            let dest = dest_dir.join("libmpv-2.dll");
            let differs = fs::metadata(&dest).map(|m| m.len()).ok()
                != fs::metadata(src).map(|m| m.len()).ok();
            if differs {
                let _ = fs::copy(src, &dest);
            }
        }
    }
}
