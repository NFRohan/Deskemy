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

/// User data snapshotted before a re-import so it survives the delete+reinsert,
/// remapped to the new lecture ids by file path.
struct Preserved {
    is_favorite: bool,
    last_opened_at: Option<i64>,
    last_lecture_file: Option<String>,
    thumbnail_path: Option<String>,
    resume_thumbnail_path: Option<String>,
    tags: Vec<String>,
    /// (file_path, content_hash, position_seconds, completed, last_watched_at)
    progress: Vec<(String, Option<String>, f64, bool, Option<i64>)>,
    /// (file_path, content_hash, position_seconds, label, created_at)
    bookmarks: Vec<(String, Option<String>, f64, Option<String>, i64)>,
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
    content_hash: Option<String>,
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
    content_hash: Option<String>,
    chapters: Vec<crate::media::Chapter>,
}

struct PlannedSection {
    id: String,
    key: String,
    title: String,
    position: i64,
    lectures: Vec<PlannedLecture>,
}

/// Phase-1 snapshot (the DB reads an import needs) carried from `read_snapshot`
/// to `build`/`persist`, so the slow probe between them holds no DB lock. Opaque
/// to callers.
pub struct ImportSnapshot {
    folder_path: String,
    title: String,
    prior: HashMap<String, PriorMeta>,
    preserved: Option<Preserved>,
}

/// Phase-2 output: the fully-probed, in-memory plan for a course, ready to
/// persist. Opaque to callers.
pub struct ImportPlan {
    scan: ScannedTree,
    sections: Vec<PlannedSection>,
}

impl ImportSnapshot {
    /// The course title (folder name).
    pub fn title(&self) -> &str {
        &self.title
    }
    /// Whether this folder already exists as a course (re-import keeps user data).
    pub fn is_reimport(&self) -> bool {
        self.preserved.is_some()
    }
}

impl ImportPlan {
    /// Total lectures across all sections (0 = nothing playable found).
    pub fn lecture_count(&self) -> usize {
        self.sections.iter().map(|s| s.lectures.len()).sum()
    }
    /// Sections that contain at least one lecture.
    pub fn section_count(&self) -> usize {
        self.sections.iter().filter(|s| !s.lectures.is_empty()).count()
    }
    /// Lectures mpv couldn't open (imported but flagged unplayable).
    pub fn unplayable_count(&self) -> usize {
        self.sections
            .iter()
            .flat_map(|s| &s.lectures)
            .filter(|l| !l.playable)
            .count()
    }
    /// Summed lecture duration, or None if nothing was probed for duration.
    pub fn total_duration(&self) -> Option<f64> {
        let mut any = false;
        let sum: f64 = self
            .sections
            .iter()
            .flat_map(|s| &s.lectures)
            .filter_map(|l| l.duration)
            .inspect(|_| any = true)
            .sum();
        any.then_some(sum)
    }
    /// Attachment (resource) files detected in the scan.
    pub fn resource_count(&self) -> usize {
        self.scan.files.iter().filter(|f| f.kind == FileKind::Attachment).count()
    }
    /// Sidecar subtitle files detected in the scan.
    pub fn subtitle_count(&self) -> usize {
        self.scan.files.iter().filter(|f| f.kind == FileKind::Subtitle).count()
    }
}

impl Importer {
    pub fn new(prober: Box<dyn MediaProber>) -> Self {
        Self { prober }
    }

    /// Import a single folder as one course, all three phases under the caller's
    /// lock. Commands instead call `read_snapshot` / `build` / `persist`
    /// separately so the slow probe in `build` runs without the DB lock; this
    /// wrapper is for tests and simple callers.
    pub fn import_course(
        &self,
        conn: &mut Connection,
        root_id: Option<&str>,
        course_dir: &Path,
    ) -> Result<String> {
        let snap = self.read_snapshot(conn, course_dir)?;
        let plan = self.build(course_dir, &snap, |_, _| {})?;
        self.persist(conn, root_id, &snap, &plan)
    }

    /// Phase 1 (brief lock): snapshot what the DB knows about this folder — prior
    /// media (to skip re-probing unchanged files) and user data (to survive the
    /// delete + reinsert in `persist`).
    pub fn read_snapshot(&self, conn: &Connection, course_dir: &Path) -> Result<ImportSnapshot> {
        let folder_path = course_dir.to_string_lossy().to_string();
        let title = course_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| folder_path.clone());

        let existing = queries::find_course_by_path(conn, &folder_path)?;
        let prior = match &existing {
            Some(id) => Self::snapshot_prior(conn, id)?,
            None => HashMap::new(),
        };
        let preserved = match &existing {
            Some(id) => Some(Self::snapshot_preserved(conn, id)?),
            None => None,
        };
        Ok(ImportSnapshot {
            folder_path,
            title,
            prior,
            preserved,
        })
    }

    /// Phase 2 (no lock): scan the folder and build the fully-probed plan. This
    /// is the slow part (mpv probing) — deliberately holds no DB lock. `progress`
    /// is called `(done, total)` as each video is processed, for a live UI.
    pub fn build(
        &self,
        course_dir: &Path,
        snap: &ImportSnapshot,
        progress: impl Fn(usize, usize),
    ) -> Result<ImportPlan> {
        let scan = FilesystemScanner.scan(course_dir)?;
        let sections = self.plan_sections(&scan, &snap.prior, &progress)?;
        Ok(ImportPlan { scan, sections })
    }

    /// Phase 3 (brief lock): persist the plan — replace any prior import of the
    /// same folder and restore preserved user data. Fast (writes only).
    pub fn persist(
        &self,
        conn: &mut Connection,
        root_id: Option<&str>,
        snap: &ImportSnapshot,
        plan: &ImportPlan,
    ) -> Result<String> {
        if plan.lecture_count() == 0 {
            return Err(DeskemyError::Import(format!(
                "no playable video files found in {}",
                snap.folder_path
            )));
        }
        let sections = &plan.sections;
        let scan = &plan.scan;

        let course_id = new_id();
        let thumbnail = detect_thumbnail(scan);

        let tx = conn.transaction()?;
        // Replace any previous import of the same folder. Re-read inside the tx:
        // the DB was unlocked during the probe, and folder_path is UNIQUE, so
        // replace whatever occupies it now (atomic with the re-insert).
        if let Some(existing) = queries::find_course_by_path(&tx, &snap.folder_path)? {
            queries::delete_course(&tx, &existing)?;
        }
        queries::insert_course(
            &tx,
            &course_id,
            root_id,
            &snap.title,
            &snap.folder_path,
            crate::domain::ScanStatus::Scanning.as_str(),
        )?;
        queries::fts_insert(&tx, "course", &course_id, &course_id, &snap.title)?;

        let mut lecture_count: i64 = 0;
        let mut total_secs: f64 = 0.0;
        let mut any_duration = false;

        for section in sections {
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
                        content_hash: lec.content_hash.as_deref(),
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
        self.associate_resources(&tx, &course_id, scan, sections)?;

        queries::update_course_stats(
            &tx,
            &course_id,
            lecture_count,
            any_duration.then_some(total_secs),
            thumbnail.as_deref(),
            crate::domain::ScanStatus::Ready.as_str(),
        )?;

        // Restore preserved user data, remapped to new lecture ids by file path,
        // falling back to content hash so a renamed file keeps its data.
        if let Some(p) = &snap.preserved {
            let mut by_path: HashMap<&str, &str> = HashMap::new();
            let mut by_hash: HashMap<&str, &str> = HashMap::new();
            for s in sections {
                for l in &s.lectures {
                    by_path.insert(l.path.as_str(), l.id.as_str());
                    if let Some(h) = &l.content_hash {
                        by_hash.insert(h.as_str(), l.id.as_str());
                    }
                }
            }
            // Resolve an old (path, hash) to a new lecture id: exact path first,
            // then hash (the file was renamed/moved).
            let remap = |path: &str, hash: &Option<String>| -> Option<&str> {
                by_path
                    .get(path)
                    .copied()
                    .or_else(|| hash.as_deref().and_then(|h| by_hash.get(h).copied()))
            };

            // Only carry over thumbnails whose files still exist, so a dangling
            // path doesn't win over a freshly-detected cover.
            let thumb = p.thumbnail_path.as_deref().filter(|p| Path::new(p).exists());
            let resume = p
                .resume_thumbnail_path
                .as_deref()
                .filter(|p| Path::new(p).exists());
            queries::restore_course_fields(
                &tx,
                &course_id,
                p.is_favorite,
                p.last_opened_at,
                thumb,
                resume,
            )?;
            if let Some(f) = &p.last_lecture_file {
                if let Some(&nid) = by_path.get(f.as_str()) {
                    queries::set_last_lecture_id(&tx, &course_id, nid)?;
                }
            }
            for tag in &p.tags {
                queries::add_tag(&tx, &course_id, tag)?;
            }
            for (path, hash, pos, completed, watched) in &p.progress {
                if let Some(nid) = remap(path, hash) {
                    queries::restore_progress(&tx, nid, *pos, *completed, *watched)?;
                }
            }
            for (path, hash, pos, label, created) in &p.bookmarks {
                if let Some(nid) = remap(path, hash) {
                    queries::restore_bookmark(
                        &tx,
                        &new_id(),
                        nid,
                        &course_id,
                        *pos,
                        label.as_deref(),
                        *created,
                    )?;
                }
            }
        }

        tx.commit()?;
        Ok(course_id)
    }

    /// Snapshot user data (progress, bookmarks, tags, course fields) so it can
    /// be reattached to the new lecture ids after a re-import.
    fn snapshot_preserved(conn: &Connection, course_id: &str) -> Result<Preserved> {
        let (is_favorite, last_opened_at, last_lecture_id, thumbnail_path, resume_thumbnail_path) =
            queries::course_preserve(conn, course_id)?;
        let last_lecture_file = match last_lecture_id {
            Some(lid) => queries::lecture_file_path(conn, &lid)?,
            None => None,
        };
        Ok(Preserved {
            is_favorite,
            last_opened_at,
            last_lecture_file,
            thumbnail_path,
            resume_thumbnail_path,
            tags: queries::tags_for_course(conn, course_id)?,
            progress: queries::progress_with_files(conn, course_id)?,
            bookmarks: queries::bookmarks_with_files(conn, course_id)?,
        })
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
        for (file_path, size, mtime, duration, container, video_codec, playable, content_hash) in
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
                    content_hash,
                    chapters,
                },
            );
        }
        Ok(map)
    }

    /// Build the in-memory section/lecture plan (ordered, cleaned, probed).
    /// Files whose path + size + mtime match `prior` reuse its metadata.
    /// `progress` is called `(done, total)` as each video is processed.
    fn plan_sections(
        &self,
        scan: &ScannedTree,
        prior: &HashMap<String, PriorMeta>,
        progress: &impl Fn(usize, usize),
    ) -> Result<Vec<PlannedSection>> {
        // Group video files by their top-level section key ("" = course root).
        let mut videos_by_key: HashMap<String, Vec<&ScannedFile>> = HashMap::new();
        for f in &scan.files {
            if f.kind == FileKind::Video {
                videos_by_key.entry(section_key(f)).or_default().push(f);
            }
        }

        let total = scan.files.iter().filter(|f| f.kind == FileKind::Video).count();
        let mut done = 0usize;

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

                // Reuse metadata when the file is unchanged (size + mtime) AND
                // we already have a duration — so files imported before mpv was
                // available (no duration) get re-probed on rescan.
                let reuse = prior.get(&path).filter(|p| {
                    p.size == Some(size) && p.mtime == Some(v.mtime) && p.duration.is_some()
                });
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

                // Content hash (size + bounded head) for move/rename detection:
                // reuse the stored one for an unchanged file, else compute once.
                let content_hash = match reuse {
                    Some(p) if p.content_hash.is_some() => p.content_hash.clone(),
                    _ => hash_lecture_file(&v.path, v.size),
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
                    content_hash,
                    chapters,
                });

                done += 1;
                progress(done, total);
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
