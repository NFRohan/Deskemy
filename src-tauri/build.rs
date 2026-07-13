use std::{env, fs, path::PathBuf};

fn main() {
    // Stage the bundled libmpv DLL next to the built executable so it can be
    // loaded at runtime (Windows searches the exe's own directory first).
    #[cfg(target_os = "windows")]
    {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let src = manifest.join("vendor").join("libmpv-2.dll");
        if src.exists() {
            let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
            let dest_dir = manifest.join("target").join(&profile);
            let _ = fs::create_dir_all(&dest_dir);
            let dest = dest_dir.join("libmpv-2.dll");
            let differs = fs::metadata(&dest).map(|m| m.len()).ok()
                != fs::metadata(&src).map(|m| m.len()).ok();
            if differs {
                let _ = fs::copy(&src, &dest);
            }
        }
        println!("cargo:rerun-if-changed=vendor/libmpv-2.dll");
    }

    tauri_build::build()
}
