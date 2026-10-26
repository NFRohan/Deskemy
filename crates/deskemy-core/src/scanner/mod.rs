//! Pure filesystem layer: walk a course folder and classify every file into a
//! flat `ScannedTree`. No DB, no structuring rules, no media decoding — that
//! keeps this trivially testable and lets future `ZipScanner`/`NetworkScanner`
//! implementations slot in behind the `Scanner` trait.
//!
//! Two cleanups keep downloaded code out of the way: dependency / VCS folders
//! (`node_modules`, `.git`, …) are skipped, and a video-free folder that holds
//! a code project becomes a single folder entry rather than hundreds of files.

use crate::error::Result;
use std::collections::{HashMap, HashSet};
use std::io::Read;
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

/// MPEG transport-stream packets are 188 bytes, each starting with the sync
/// byte 0x47 — which tells a `.ts` video from a TypeScript source file.
pub fn looks_like_mpeg_ts(head: &[u8]) -> bool {
    const PACKET: usize = 188;
    head.len() > PACKET && head[0] == 0x47 && head[PACKET] == 0x47
}

fn read_head(path: &Path, n: usize) -> Vec<u8> {
    let mut buf = vec![0; n];
    let read = std::fs::File::open(path).and_then(|mut f| f.read(&mut buf)).unwrap_or(0);
    buf.truncate(read);
    buf
}

/// A video by its extension, and a `.ts` by its content too.
pub(crate) fn is_video(path: &Path) -> bool {
    classify(path) == FileKind::Video
        && (!path.extension().is_some_and(|e| e.eq_ignore_ascii_case("ts")) || looks_like_mpeg_ts(&read_head(path, 189)))
}

/// Folders never worth listing: dependencies, VCS data, caches.
pub(crate) const SKIPPED_DIRS: &[&str] = &[
    "node_modules", ".git", ".svn", ".hg", "__pycache__", ".venv", "venv", ".idea", ".vscode", ".next",
    ".gradle", "target", "bower_components",
];

/// Files or folders that mark a code project.
const PROJECT_MARKERS: &[&str] = &[
    "package.json", ".github", ".git", "node_modules", ".gitignore", "tsconfig.json", "cargo.toml",
    "go.mod", "pom.xml", "build.gradle", "requirements.txt", "pyproject.toml", "composer.json",
    "gemfile", "makefile", "cmakelists.txt", "dockerfile", "docker-compose.yml", "docker-compose.yaml",
    "angular.json", "vite.config.js", "vite.config.ts", "webpack.config.js",
];

/// More files than this in a video-free folder reads as a project too.
const PROJECT_FILE_COUNT: usize = 20;

/// Source files: a video-free folder inside a section holding these (and no
/// documents) is a code download, e.g. a lecture's `index.html` + `styles.css`.
const CODE_EXTENSIONS: &[&str] = &[
    "html", "htm", "css", "scss", "js", "mjs", "cjs", "jsx", "ts", "tsx", "vue", "svelte", "json", "py",
    "java", "kt", "rb", "go", "rs", "c", "cc", "cpp", "h", "hpp", "cs", "php", "sql", "sh", "ps1", "yml",
    "yaml", "xml", "toml", "ipynb", "dart", "swift",
];
/// Documents keep a folder listed file by file (each can be opened and ticked).
const DOCUMENT_EXTENSIONS: &[&str] =
    &["pdf", "doc", "docx", "ppt", "pptx", "xls", "xlsx", "odt", "odp", "ods", "epub", "txt", "rtf"];

fn extension_in(f: &ScannedFile, list: &[&str]) -> bool {
    Path::new(&f.name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| list.contains(&e.to_lowercase().as_str()))
}

#[derive(Debug, Clone)]
pub struct ScannedFile {
    /// Absolute path.
    pub path: PathBuf,
    /// File name including extension (a folder entry's folder name).
    pub name: String,
    pub kind: FileKind,
    pub size: u64,
    /// Modification time as unix seconds (0 if unavailable).
    pub mtime: i64,
    /// Directory containing the file, relative to the scan root
    /// (empty for files directly in the root).
    pub rel_dir: PathBuf,
    /// A whole code-project folder, listed as one resource.
    pub is_dir: bool,
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
        // Relative folders holding a project marker (the marker's own folder).
        let mut marked: HashSet<PathBuf> = HashSet::new();

        let walker = walkdir::WalkDir::new(root).follow_links(false).into_iter().filter_entry(|e| {
            // Never the root itself, whatever it's called.
            e.depth() == 0 || !(e.file_type().is_dir() && SKIPPED_DIRS.contains(&e.file_name().to_string_lossy().as_ref()))
        });
        for entry in walker.filter_map(|e| e.ok()) {
            let name = entry.file_name().to_string_lossy().to_string();
            let rel_dir = entry
                .path()
                .parent()
                .and_then(|p| p.strip_prefix(root).ok())
                .map(|p| p.to_path_buf())
                .unwrap_or_default();
            if entry.depth() > 0 && PROJECT_MARKERS.contains(&name.to_lowercase().as_str()) {
                marked.insert(rel_dir.clone());
            }
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path().to_path_buf();
            let md = entry.metadata().ok();
            let size = md.as_ref().map(|m| m.len()).unwrap_or(0);
            let mtime = md
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            let kind = match classify(&path) {
                FileKind::Video if !is_video(&path) => FileKind::Attachment,
                kind => kind,
            };
            files.push(ScannedFile { kind, path, name, size, mtime, rel_dir, is_dir: false });
        }

        let base = wrapper(&files);
        let mut files = collapse_projects(root, files, &marked, &base);
        // Relative to where the course starts.
        for f in &mut files {
            if let Ok(rest) = f.rel_dir.strip_prefix(&base) {
                f.rel_dir = rest.to_path_buf();
            }
        }
        Ok(ScannedTree { root: root.to_path_buf(), files })
    }
}

/// The folders a course is wrapped in, where its sections start: `""` when
/// it isn't. A wrapper is the one folder holding the course's videos, none
/// directly in it (a download site's `~Get Your Files Here !`, or the
/// course's own name again), maybe beside a promo video; wrapped again, both.
pub fn wrapper(files: &[ScannedFile]) -> PathBuf {
    let mut base = PathBuf::new();
    loop {
        // Per folder in `base`: its videos, and whether any is directly in it.
        let mut tops: HashMap<&std::ffi::OsStr, (usize, bool)> = HashMap::new();
        let mut loose = 0;
        for f in files.iter().filter(|f| f.kind == FileKind::Video) {
            let Ok(rel) = f.rel_dir.strip_prefix(&base) else { continue };
            let mut parts = rel.components();
            match parts.next() {
                Some(top) => {
                    let entry = tops.entry(top.as_os_str()).or_default();
                    entry.0 += 1;
                    entry.1 |= parts.next().is_none();
                }
                None => loose += 1,
            }
        }
        // A video beside it (a promo, at the top) is fine; the course's own aren't.
        let in_wrapper = match tops.into_iter().collect::<Vec<_>>()[..] {
            [(top, (n, false))] if loose == 0 || (base.as_os_str().is_empty() && loose < n) => top.to_owned(),
            _ => return base,
        };
        base.push(in_wrapper);
    }
}

/// Every non-empty ancestor of a relative folder, shortest first.
fn prefixes(rel: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for c in rel.components() {
        acc.push(c);
        out.push(acc.clone());
    }
    out
}

/// Replace the files of each code-project folder with one folder entry. A
/// project has no videos inside and either a project marker, many files, or
/// — inside a section — source files but no documents. The outermost such
/// folder wins, so a project's subfolders don't become entries of their own.
/// Sections start in `base` (see [`wrapper`]).
pub fn collapse_projects(
    root: &Path,
    files: Vec<ScannedFile>,
    marked: &HashSet<PathBuf>,
    base: &Path,
) -> Vec<ScannedFile> {
    let mut with_video: HashSet<PathBuf> = HashSet::new();
    let mut with_code: HashSet<PathBuf> = HashSet::new();
    let mut with_documents: HashSet<PathBuf> = HashSet::new();
    let mut counts: HashMap<PathBuf, usize> = HashMap::new();
    let mut has_marker: HashSet<PathBuf> = HashSet::new();
    for f in &files {
        let (code, document) = (extension_in(f, CODE_EXTENSIONS), extension_in(f, DOCUMENT_EXTENSIONS));
        for p in prefixes(&f.rel_dir) {
            if f.kind == FileKind::Video {
                with_video.insert(p.clone());
            }
            if code {
                with_code.insert(p.clone());
            }
            if document {
                with_documents.insert(p.clone());
            }
            *counts.entry(p).or_default() += 1;
        }
    }
    for m in marked {
        for p in prefixes(m) {
            has_marker.insert(p);
        }
    }
    let project = |p: &PathBuf| {
        // Below a section folder (a section of only article pages stays a section).
        let in_section = p.strip_prefix(base).is_ok_and(|rel| rel.components().count() >= 2);
        !with_video.contains(p)
            && (has_marker.contains(p)
                || counts.get(p).copied().unwrap_or(0) > PROJECT_FILE_COUNT
                || (in_section && with_code.contains(p) && !with_documents.contains(p)))
    };

    let mut out = Vec::new();
    let mut folders: HashMap<PathBuf, ScannedFile> = HashMap::new();
    for f in files {
        match prefixes(&f.rel_dir).into_iter().find(|p| project(p)) {
            Some(dir) => {
                let entry = folders.entry(dir.clone()).or_insert_with(|| ScannedFile {
                    path: root.join(&dir),
                    name: dir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
                    kind: FileKind::Attachment,
                    size: 0,
                    mtime: 0,
                    rel_dir: dir.parent().map(Path::to_path_buf).unwrap_or_default(),
                    is_dir: true,
                });
                entry.size += f.size;
                entry.mtime = entry.mtime.max(f.mtime);
            }
            None => out.push(f),
        }
    }
    let mut folders: Vec<ScannedFile> = folders.into_values().collect();
    folders.sort_by(|a, b| a.path.cmp(&b.path));
    out.extend(folders);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn classifies_by_extension() {
        assert_eq!(classify(Path::new("a/01 Intro.mp4")), FileKind::Video);
        assert_eq!(classify(Path::new("a/lesson.MKV")), FileKind::Video);
        assert_eq!(classify(Path::new("a/lesson.en.srt")), FileKind::Subtitle);
        assert_eq!(classify(Path::new("a/cover.jpg")), FileKind::Image);
        assert_eq!(classify(Path::new("a/slides.pdf")), FileKind::Attachment);
        assert_eq!(classify(Path::new("a/README")), FileKind::Attachment);
    }

    #[test]
    fn mpeg_ts_is_told_from_typescript() {
        let mut video = vec![0u8; 400];
        video[0] = 0x47;
        video[188] = 0x47;
        assert!(looks_like_mpeg_ts(&video));
        assert!(!looks_like_mpeg_ts(b"export declare function getInput(name: string): string;"));
        assert!(!looks_like_mpeg_ts(&[0x47]));
    }

    fn write(path: &Path, bytes: &[u8]) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    #[test]
    fn a_wrapper_is_where_the_sections_start() {
        let videos = |rel_dirs: &[&str]| -> Vec<ScannedFile> {
            rel_dirs
                .iter()
                .map(|d| ScannedFile {
                    path: PathBuf::new(),
                    name: "v.mp4".into(),
                    kind: FileKind::Video,
                    size: 0,
                    mtime: 0,
                    rel_dir: PathBuf::from(d),
                    is_dir: false,
                })
                .collect()
        };
        // Wrapped twice; then beside a promo; sections of its own.
        assert_eq!(wrapper(&videos(&["Get Files/Course/01 A", "Get Files/Course/02 B"])), Path::new("Get Files/Course"));
        assert_eq!(wrapper(&videos(&["", "Course/01 A", "Course/02 B"])), Path::new("Course"));
        assert_eq!(wrapper(&videos(&["01 A", "02 B"])), Path::new(""));
        // A course of one section is that section's; one with more loose
        // videos than in its one folder keeps them.
        assert_eq!(wrapper(&videos(&["01 Only", "01 Only"])), Path::new(""));
        assert_eq!(wrapper(&videos(&["", "", "Extras/clips"])), Path::new(""));
        // A video directly in a wrapper makes it the course.
        assert_eq!(wrapper(&videos(&["Course", "Course/01 A", "Course/02 B"])), Path::new(""));
    }

    #[test]
    fn code_projects_become_one_folder_and_dependencies_are_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Course");
        let s = root.join("03 - Basics");
        write(&s.join("004 Creating a Workflow.mp4"), b"x");
        write(&s.join("004 Creating a Workflow_en.srt"), b"x");
        // A lecture's code download: an unzipped project.
        let project = s.join("005 01-First-Workflow").join("01 First Workflow");
        write(&project.join("package.json"), b"{}");
        write(&project.join("src").join("index.ts"), b"export const x = 1;");
        write(&project.join("node_modules").join("dep").join("index.d.ts"), b"export {};");
        write(&project.join(".github").join("workflows").join("ci.yml"), b"on: push");
        // A plain folder of notes stays file by file.
        write(&s.join("notes").join("cheatsheet.pdf"), b"x");
        // A small web project with no marker: source files, no documents.
        let web = s.join("010 03-Extra-Files").join("03 Extra Files");
        write(&web.join("index.html"), b"<html>");
        write(&web.join("styles.css"), b"body{}");
        write(&web.join("logo.png"), b"x");
        // A section made only of article pages stays a section.
        write(&root.join("04 - Reading").join("001 Article.html"), b"<html>");
        // A real transport-stream video.
        let mut ts = vec![0u8; 400];
        ts[0] = 0x47;
        ts[188] = 0x47;
        write(&s.join("006 Recording.ts"), &ts);

        let tree = FilesystemScanner.scan(&root).unwrap();
        let mut names: Vec<(String, FileKind, bool)> =
            tree.files.iter().map(|f| (f.name.clone(), f.kind, f.is_dir)).collect();
        names.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            names,
            vec![
                ("001 Article.html".into(), FileKind::Attachment, false),
                ("004 Creating a Workflow.mp4".into(), FileKind::Video, false),
                ("004 Creating a Workflow_en.srt".into(), FileKind::Subtitle, false),
                ("005 01-First-Workflow".into(), FileKind::Attachment, true),
                ("006 Recording.ts".into(), FileKind::Video, false),
                ("010 03-Extra-Files".into(), FileKind::Attachment, true),
                ("cheatsheet.pdf".into(), FileKind::Attachment, false),
            ]
        );
        let folder = tree.files.iter().find(|f| f.is_dir && f.name.starts_with("005")).unwrap();
        assert_eq!(folder.rel_dir, PathBuf::from("03 - Basics"));
        assert_eq!(folder.path, s.join("005 01-First-Workflow"));
    }

    #[test]
    fn a_folder_with_videos_is_never_collapsed() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("Course");
        // A section that happens to hold a package.json next to its videos.
        write(&root.join("01 - Intro").join("001 Welcome.mp4"), b"x");
        write(&root.join("01 - Intro").join("package.json"), b"{}");
        let tree = FilesystemScanner.scan(&root).unwrap();
        assert!(tree.files.iter().all(|f| !f.is_dir));
        assert_eq!(tree.files.len(), 2);
    }
}
