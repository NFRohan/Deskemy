//! Turns a `ScannedTree` into a persisted course: detects sections, orders and
//! cleans titles, probes videos, and associates subtitles + resource files.
//!
//! Structuring rules are tuned against real Udemy downloads:
//! - immediate subfolders are sections; loose root videos form "Introduction"
//! - resources attach to the lecture sharing their leading number in the same
//!   section (Udemy names code files `011 foo.sql` next to `011 Lecture.mp4`)
//! - subtitles attach to the video whose stem prefixes the subtitle name

pub mod structure;

use crate::db::queries::{self, LectureInsert};
use crate::db::new_id;
use crate::error::{DeskemyError, Result};
use crate::hashing::content_hash;
use crate::media::MediaProber;
use crate::scanner::{FileKind, FilesystemScanner, ScannedFile, ScannedTree, Scanner};
use rusqlite::Connection;
use std::collections::HashMap;
use std::path::Path;
use structure::{clean_title, leading_number, sort_key};

pub struct Importer {
    prober: Box<dyn MediaProber>,
}

/// Metadata carried over from a previous import so an unchanged file's video
/// doesn't have to be re-probed on rescan.
struct PriorMeta {
    size: Option<i64>,
    mtime: Option<i64>,
    duration: Option<f64>,
    container: Option<String>,
    video_codec: Option<String>,
    playable: bool,
    chapters: Vec<crate::media::Chapter>,
}

struct PlannedLecture {
    id: String,
    title: String,
    position: i64,
    number: Option<i64>,
    stem: String,
    rel_dir: String,
    path: String,
    size: i64,
    mtime: i64,
    container: Option<String>,
    video_codec: Option<String>,
    playable: bool,
    duration: Option<f64>,
    chapters: Vec<crate::media::Chapter>,
}

struct PlannedSection {
    id: String,
    key: String,
    title: String,
    position: i64,
    lectures: Vec<PlannedLecture>,
}

impl Importer {
    pub fn new(prober: Box<dyn MediaProber>) -> Self {
        Self { prober }
    }

    /// Import a single folder as one course. Re-importing an already-known
    /// folder replaces it (simple reconcile for now).
    pub fn import_course(
        &self,
        conn: &mut Connection,
        root_id: Option<&str>,
        course_dir: &Path,
    ) -> Result<String> {
        let folder_path = course_dir.to_string_lossy().to_string();
        let title = course_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| folder_path.clone());

        let scan = FilesystemScanner.scan(course_dir)?;

        // Snapshot the prior import's media so unchanged files skip re-probing.
        let existing = queries::find_course_by_path(conn, &folder_path)?;
        let prior = match &existing {
            Some(id) => Self::snapshot_prior(conn, id)?,
            None => HashMap::new(),
        };

        let sections = self.plan_sections(&scan, &prior)?;
        if sections.iter().all(|s| s.lectures.is_empty()) {
            return Err(DeskemyError::Import(format!(
                "no playable video files found in {folder_path}"
            )));
        }

        // Replace any previous import of the same folder.
        if let Some(existing) = &existing {
            queries::delete_course(conn, existing)?;
        }

        let course_id = new_id();
        let thumbnail = detect_thumbnail(&scan);

        let tx = conn.transaction()?;
        queries::insert_course(
            &tx,
            &course_id,
            root_id,
            &title,
            &folder_path,
            crate::domain::ScanStatus::Scanning.as_str(),
        )?;
        queries::fts_insert(&tx, "course", &course_id, &course_id, &title)?;

        let mut lecture_count: i64 = 0;
        let mut total_secs: f64 = 0.0;
        let mut any_duration = false;

        for section in &sections {
            queries::insert_section(
                &tx,
                &section.id,
                &course_id,
                &section.title,
                section.position,
                Some(&section.key),
            )?;
            queries::fts_insert(&tx, "section", &section.id, &course_id, &section.title)?;

            for lec in &section.lectures {
                queries::insert_lecture(
                    &tx,
                    &LectureInsert {
                        id: &lec.id,
                        course_id: &course_id,
                        section_id: &section.id,
                        title: &lec.title,
                        file_path: &lec.path,
                        position: lec.position,
                        duration: lec.duration,
                        container: lec.container.as_deref(),
                        video_codec: lec.video_codec.as_deref(),
                        playable: lec.playable,
                        file_size: Some(lec.size),
                        mtime: Some(lec.mtime),
                        content_hash: None,
                    },
                )?;
                queries::fts_insert(&tx, "lecture", &lec.id, &course_id, &lec.title)?;

                for ch in &lec.chapters {
                    queries::insert_chapter(
                        &tx,
                        &new_id(),
                        &lec.id,
                        ch.index as i64,
                        ch.title.as_deref(),
                        ch.start.as_secs_f64(),
                    )?;
                }

                lecture_count += 1;
                if let Some(d) = lec.duration {
                    total_secs += d;
                    any_duration = true;
                }
            }
        }

        // Second pass: subtitles + attachments, now that lecture ids exist.
        self.associate_resources(&tx, &course_id, &scan, &sections)?;

        queries::update_course_stats(
            &tx,
            &course_id,
            lecture_count,
            any_duration.then_some(total_secs),
            thumbnail.as_deref(),
            crate::domain::ScanStatus::Ready.as_str(),
        )?;

        tx.commit()?;
        Ok(course_id)
    }

    /// Load a previous import's media keyed by file path, for reuse on rescan.
    fn snapshot_prior(conn: &Connection, course_id: &str) -> Result<HashMap<String, PriorMeta>> {
        let mut chapters_by_file: HashMap<String, Vec<crate::media::Chapter>> = HashMap::new();
        for (file_path, idx, title, start_time) in queries::course_chapters(conn, course_id)? {
            chapters_by_file
                .entry(file_path)
                .or_default()
                .push(crate::media::Chapter {
                    index: idx as usize,
                    title,
                    start: std::time::Duration::from_secs_f64(start_time.max(0.0)),
                });
        }

        let mut map = HashMap::new();
        for (file_path, size, mtime, duration, container, video_codec, playable) in
            queries::course_lecture_media(conn, course_id)?
        {
            let chapters = chapters_by_file.remove(&file_path).unwrap_or_default();
            map.insert(
                file_path,
                PriorMeta {
                    size,
                    mtime,
                    duration,
                    container,
                    video_codec,
                    playable,
                    chapters,
                },
            );
        }
        Ok(map)
    }

    /// Build the in-memory section/lecture plan (ordered, cleaned, probed).
    /// Files whose path + size + mtime match `prior` reuse its metadata.
    fn plan_sections(
        &self,
        scan: &ScannedTree,
        prior: &HashMap<String, PriorMeta>,
    ) -> Result<Vec<PlannedSection>> {
        // Group video files by their top-level section key ("" = course root).
        let mut videos_by_key: HashMap<String, Vec<&ScannedFile>> = HashMap::new();
        for f in &scan.files {
            if f.kind == FileKind::Video {
                videos_by_key.entry(section_key(f)).or_default().push(f);
            }
        }

        let mut keys: Vec<String> = videos_by_key.keys().cloned().collect();
        keys.sort_by_key(|k| section_sort_key(k));

        let mut sections = Vec::new();
        for (s_pos, key) in keys.into_iter().enumerate() {
            let mut vids = videos_by_key.remove(&key).unwrap();
            vids.sort_by(|a, b| sort_key(&a.name).cmp(&sort_key(&b.name)));

            let title = if key.is_empty() {
                "Introduction".to_string()
            } else {
                clean_title(&key)
            };

            let mut lectures = Vec::new();
            for (l_pos, v) in vids.into_iter().enumerate() {
                let path = v.path.to_string_lossy().to_string();
                let size = v.size as i64;

                // Reuse metadata when the same file is unchanged (size + mtime).
                let reuse = prior
                    .get(&path)
                    .filter(|p| p.size == Some(size) && p.mtime == Some(v.mtime));
                let (container, video_codec, playable, duration, chapters) = match reuse {
                    Some(p) => (
                        p.container.clone(),
                        p.video_codec.clone(),
                        p.playable,
                        p.duration,
                        p.chapters.clone(),
                    ),
                    None => {
                        let meta = self.prober.probe(&v.path)?;
                        (
                            (!meta.container.is_empty()).then(|| meta.container.clone()),
                            meta.video_codec.clone(),
                            meta.playable,
                            meta.duration.map(|d| d.as_secs_f64()),
                            meta.chapters,
                        )
                    }
                };

                let stem = Path::new(&v.name)
                    .file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_else(|| v.name.clone());
                lectures.push(PlannedLecture {
                    id: new_id(),
                    title: clean_title(&v.name),
                    position: l_pos as i64,
                    number: leading_number(&v.name),
                    stem,
                    rel_dir: v.rel_dir.to_string_lossy().to_string(),
                    path,
                    size,
                    mtime: v.mtime,
                    container,
                    video_codec,
                    playable,
                    duration,
                    chapters,
                });
            }

            sections.push(PlannedSection {
                id: new_id(),
                key,
                title,
                position: s_pos as i64,
                lectures,
            });
        }

        Ok(sections)
    }

    /// Attach subtitles and resource files to lectures/sections.
    fn associate_resources(
        &self,
        conn: &Connection,
        course_id: &str,
        scan: &ScannedTree,
        sections: &[PlannedSection],
    ) -> Result<()> {
        let by_key: HashMap<&str, &PlannedSection> =
            sections.iter().map(|s| (s.key.as_str(), s)).collect();

        for f in &scan.files {
            match f.kind {
                FileKind::Video | FileKind::Image => continue,
                FileKind::Subtitle => {
                    let key = section_key(f);
                    if let Some(section) = by_key.get(key.as_str()) {
                        if let Some((lec, lang)) = match_subtitle(f, section) {
                            queries::insert_subtitle(
                                conn,
                                &new_id(),
                                &lec.id,
                                lang.as_deref(),
                                lang.as_deref(),
                                &f.path.to_string_lossy(),
                            )?;
                        }
                    }
                }
                FileKind::Attachment => {
                    let key = section_key(f);
                    let section = by_key.get(key.as_str()).copied();
                    let lecture_id = section.and_then(|s| match_attachment(f, s)).map(|l| l.id.clone());
                    let att_id = new_id();
                    queries::insert_attachment(
                        conn,
                        &att_id,
                        course_id,
                        section.map(|s| s.id.as_str()),
                        lecture_id.as_deref(),
                        &f.name,
                        &f.path.to_string_lossy(),
                        Some(attachment_kind(f)),
                    )?;
                    queries::fts_insert(conn, "attachment", &att_id, course_id, &f.name)?;
                }
            }
        }
        Ok(())
    }
}

/// Optionally record a content hash for a lecture file (bounded blake3). Used by
/// re-scan to detect moved files; computed lazily to keep import fast.
pub fn hash_lecture_file(path: &Path, size: u64) -> Option<String> {
    content_hash(path, size).ok()
}

fn section_key(f: &ScannedFile) -> String {
    f.rel_dir
        .components()
        .next()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .unwrap_or_default()
}

/// Introduction (root, empty key) first; then numbered sections ascending;
/// then unnumbered named sections alphabetically.
fn section_sort_key(key: &str) -> (u8, i64, String) {
    if key.is_empty() {
        (0, 0, String::new())
    } else {
        (1, leading_number(key).unwrap_or(i64::MAX), key.to_lowercase())
    }
}

fn detect_thumbnail(scan: &ScannedTree) -> Option<String> {
    let root_images: Vec<&ScannedFile> = scan
        .files
        .iter()
        .filter(|f| f.kind == FileKind::Image && section_key(f).is_empty())
        .collect();

    // Prefer a conventionally named cover.
    for f in &root_images {
        let stem = f.name.rsplit_once('.').map(|(s, _)| s).unwrap_or(&f.name);
        let low = stem.to_lowercase();
        if matches!(low.as_str(), "cover" | "thumbnail" | "poster" | "folder" | "course") {
            return Some(f.path.to_string_lossy().to_string());
        }
    }
    root_images
        .first()
        .map(|f| f.path.to_string_lossy().to_string())
}

/// Match a subtitle file to a lecture in its section by stem prefix.
/// Returns the lecture and parsed language (`name.en.srt` → `Some("en")`).
fn match_subtitle<'a>(
    f: &ScannedFile,
    section: &'a PlannedSection,
) -> Option<(&'a PlannedLecture, Option<String>)> {
    let sub_stem = Path::new(&f.name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())?;
    let sub_rel = f.rel_dir.to_string_lossy().to_string();

    // Prefer the longest matching lecture stem (most specific).
    let mut best: Option<(&PlannedLecture, Option<String>)> = None;
    for lec in &section.lectures {
        if lec.rel_dir != sub_rel {
            continue;
        }
        if sub_stem == lec.stem {
            return Some((lec, None));
        }
        if let Some(rest) = sub_stem.strip_prefix(&lec.stem) {
            if let Some(lang) = rest.strip_prefix('.') {
                if !lang.is_empty() {
                    let better = best
                        .as_ref()
                        .map(|(b, _)| lec.stem.len() > b.stem.len())
                        .unwrap_or(true);
                    if better {
                        best = Some((lec, Some(lang.to_string())));
                    }
                }
            }
        }
    }
    best
}

/// Match a resource file to a lecture: exact stem, then same leading number
/// within the section.
fn match_attachment<'a>(f: &ScannedFile, section: &'a PlannedSection) -> Option<&'a PlannedLecture> {
    let stem = Path::new(&f.name)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let rel = f.rel_dir.to_string_lossy().to_string();

    if let Some(l) = section
        .lectures
        .iter()
        .find(|l| l.rel_dir == rel && l.stem == stem)
    {
        return Some(l);
    }
    let num = leading_number(&f.name)?;
    section
        .lectures
        .iter()
        .find(|l| l.rel_dir == rel && l.number == Some(num))
}

fn attachment_kind(f: &ScannedFile) -> &'static str {
    let ext = Path::new(&f.name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    match ext.as_str() {
        "pdf" => "pdf",
        "zip" | "rar" | "7z" | "tar" | "gz" => "archive",
        "html" | "htm" => "html",
        "sql" | "js" | "ts" | "py" | "json" | "dbml" | "java" | "rb" | "go" | "rs" | "css"
        | "c" | "cpp" | "h" | "sh" | "yml" | "yaml" | "xml" | "md" => "code",
        "txt" => "text",
        _ => "other",
    }
}
