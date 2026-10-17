use std::{env, fs, path::PathBuf};

fn main() {
    slint_build::compile("ui/app.slint").expect("compile .slint UI");

    // Windows: embed the icon and version info in the exe, so Explorer,
    // shortcuts and Task Manager show Deskemy rather than a generic program.
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rerun-if-changed=icons/deskemy.ico");
        let mut res = winresource::WindowsResource::new();
        res.set_icon("icons/deskemy.ico")
            .set("ProductName", "Deskemy")
            .set("FileDescription", "Deskemy")
            .set("CompanyName", "Spooksy")
            .set("LegalCopyright", "© 2026 Nayeem Fardin")
            .set("OriginalFilename", "deskemy.exe");
        if let Err(e) = res.compile() {
            // A missing resource compiler shouldn't stop the build.
            println!("cargo:warning=could not embed the Windows icon: {e}");
        }
    }

    // Ship the third-party licenses beside the executable.
    {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
        let dest = manifest.join("target").join(&profile).join("licenses");
        let _ = fs::create_dir_all(&dest);
        println!("cargo:rerun-if-changed=licenses");
        for name in ["../LICENSE", "licenses"] {
            let src = manifest.join(name);
            if src.is_file() {
                let _ = fs::copy(&src, dest.join("LICENSE-deskemy.txt"));
            } else if let Ok(entries) = fs::read_dir(&src) {
                for e in entries.flatten() {
                    let _ = fs::copy(e.path(), dest.join(e.file_name()));
                }
            }
        }
    }

    // Stage libmpv and PDFium next to the built executable (Windows searches
    // the exe's own directory first). The DLLs are too large for git; drop
    // copies in app/vendor/ (scripts/get-pdfium.ps1 fetches PDFium). Other
    // platforms use the system's libraries. (The target's OS, not the build
    // machine's.)
    if env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
        for dll in ["libmpv-2.dll", "pdfium.dll"] {
            let src = manifest.join("vendor").join(dll);
            println!("cargo:rerun-if-changed={}", src.display());
            if src.exists() {
                let profile = env::var("PROFILE").unwrap_or_else(|_| "debug".into());
                let dest_dir = manifest.join("target").join(&profile);
                let _ = fs::create_dir_all(&dest_dir);
                let dest = dest_dir.join(dll);
                let differs = fs::metadata(&dest).map(|m| m.len()).ok()
                    != fs::metadata(&src).map(|m| m.len()).ok();
                if differs {
                    let _ = fs::copy(&src, &dest);
                }
            }
        }
    }
}
