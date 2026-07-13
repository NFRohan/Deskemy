//! Pure filesystem layer: walk a course folder and classify every file into a
//! flat `ScannedTree`. No DB, no structuring rules, no media decoding — that
//! keeps this trivially testable and lets future `ZipScanner`/`NetworkScanner`
//! implementations slot in behind the `Scanner` trait.

use crate::error::Result;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Video,
    Subtitle,
    Image,
    Attachment,
}

/// Classify a file purely by extension.
pub fn classify(path: &Path) -> FileKind {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "mp4" | "mkv" | "webm" | "avi" | "mov" | "m4v" | "flv" | "ts" | "wmv" | "mpg" | "mpeg"
        | "ogv" | "3gp" => FileKind::Video,
        "srt" | "vtt" | "ass" | "ssa" | "sub" => FileKind::Subtitle,
        "jpg" | "jpeg" | "png" | "webp" | "gif" | "bmp" => FileKind::Image,
        _ => FileKind::Attachment,
    }
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Absolute path.
    pub path: PathBuf,
    /// File name including extension.
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    /// Modification time as unix seconds (0 if unavailable).
    pub mtime: i64,
    /// Directory containing the file, relative to the scan root
    /// (empty for files directly in the root).
    pub rel_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ScannedTree {
    pub root: PathBuf,
    pub files: Vec<ScannedFile>,
}

pub trait Scanner {
    fn scan(&self, root: &Path) -> Result<ScannedTree>;
}

pub struct FilesystemScanner;

impl Scanner for FilesystemScanner {
    fn scan(&self, root: &Path) -> Result<ScannedTree> {
        let mut files = Vec::new();

        for entry in walkdir::WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            let name = entry.file_name().to_string_lossy().to_string();

            let md = entry.metadata().ok();
            let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = md
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            let rel_dir = path
                .parent()
                .and_then(|p| p.strip_prefix(root).ok())
                .map(|p| p.to_path_buf())
                .unwrap_or_default();

            files.push(ScannedFile {
                kind: classify(&path),
                path,
                name,
                size,
                mtime,
                rel_dir,
            });
        }

        Ok(ScannedTree {
            root: root.to_path_buf(),
            files,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_by_extension() {
        assert_eq!(classify(Path::new("a/01 Intro.mp4")), FileKind::Video);
        assert_eq!(classify(Path::new("a/lesson.MKV")), FileKind::Video);
        assert_eq!(classify(Path::new("a/lesson.en.srt")), FileKind::Subtitle);
        assert_eq!(classify(Path::new("a/cover.jpg")), FileKind::Image);
        assert_eq!(classify(Path::new("a/slides.pdf")), FileKind::Attachment);
        assert_eq!(classify(Path::new("a/README")), FileKind::Attachment);
    }
}
