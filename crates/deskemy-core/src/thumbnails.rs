//! Course thumbnail storage. Images (user-picked or pasted) are written into
//! the app data dir, content-addressed by hash, so identical images dedupe and
//! the asset protocol can serve them by a stable path.

use crate::error::{DeskemyError, Result};
use std::path::{Path, PathBuf};

/// Best-effort image type detection from magic bytes.
pub fn sniff_ext(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("jpg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("gif")
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("webp")
    } else if bytes.starts_with(b"BM") {
        Some("bmp")
    } else {
        None
    }
}

/// Normalize an extension/MIME hint (e.g. ".JPEG" or "image/png") to a bare,
/// known image extension, defaulting to png.
fn sanitize_ext(hint: &str) -> &'static str {
    let lower = hint.trim_start_matches('.').to_lowercase();
    match lower.rsplit('/').next().unwrap_or(&lower) {
        "jpeg" | "jpg" => "jpg",
        "png" => "png",
        "gif" => "gif",
        "webp" => "webp",
        "bmp" => "bmp",
        _ => "png",
    }
}

/// Write `bytes` into `thumbs_dir`, named by content hash. Returns the stored
/// absolute path. The extension is taken from the bytes' magic number when
/// recognizable, else from `ext_hint`, else png.
pub fn store(thumbs_dir: &Path, bytes: &[u8], ext_hint: Option<&str>) -> Result<PathBuf> {
    if bytes.is_empty() {
        return Err(DeskemyError::Other("empty image data".into()));
    }
    let ext = sniff_ext(bytes).unwrap_or_else(|| ext_hint.map(sanitize_ext).unwrap_or("png"));
    std::fs::create_dir_all(thumbs_dir)?;
    let hash = blake3::hash(bytes).to_hex();
    let path = thumbs_dir.join(format!("{}.{ext}", &hash[..16]));
    if !path.exists() {
        std::fs::write(&path, bytes)?;
    }
    Ok(path)
}
