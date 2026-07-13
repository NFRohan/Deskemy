//! Domain models — pure, serializable, persistence-agnostic (no rusqlite types).
//! The `db` module owns all SQL and maps rows into these. Sent to the frontend
//! as-is by commands.

use serde::Serialize;

/// Lifecycle of a course's import/scan. Stored as TEXT.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanStatus {
    Importing,
    Scanning,
    Ready,
    Missing,
    Error,
}

impl ScanStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ScanStatus::Importing => "Importing",
            ScanStatus::Scanning => "Scanning",
            ScanStatus::Ready => "Ready",
            ScanStatus::Missing => "Missing",
            ScanStatus::Error => "Error",
        }
    }
}

/// Compact course record for the library grid.
#[derive(Debug, Clone, Serialize)]
pub struct CourseSummary {
    pub id: String,
    pub title: String,
    pub folder_path: String,
    pub thumbnail_path: Option<String>,
    pub lecture_count: i64,
    pub total_duration: Option<f64>,
    pub is_favorite: bool,
    pub scan_status: String,
    pub last_opened_at: Option<i64>,
    /// Number of completed lectures (for progress %).
    pub completed_count: i64,
    /// Frame grabbed at the resume point, for the Continue Watching entry.
    pub resume_thumbnail_path: Option<String>,
    /// User-defined tags (sorted).
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Lecture {
    pub id: String,
    pub section_id: String,
    pub title: String,
    pub file_path: String,
    pub position: i64,
    pub duration: Option<f64>,
    pub container: Option<String>,
    pub video_codec: Option<String>,
    pub playable: bool,
    /// Resume position in seconds (0 if unwatched).
    pub position_seconds: f64,
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Section {
    pub id: String,
    pub title: String,
    pub position: i64,
    pub lectures: Vec<Lecture>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CourseDetail {
    pub id: String,
    pub title: String,
    pub folder_path: String,
    pub thumbnail_path: Option<String>,
    pub total_duration: Option<f64>,
    pub is_favorite: bool,
    pub scan_status: String,
    pub last_opened_at: Option<i64>,
    pub last_lecture_id: Option<String>,
    pub sections: Vec<Section>,
}

/// A user-placed marker at a time position within a lecture.
#[derive(Debug, Clone, Serialize)]
pub struct Bookmark {
    pub id: String,
    pub lecture_id: String,
    pub position_seconds: f64,
    pub label: Option<String>,
    pub created_at: i64,
}

/// Aggregate library/watch statistics for the stats page.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryStats {
    pub courses_total: i64,
    pub courses_completed: i64,
    pub courses_in_progress: i64,
    pub lectures_total: i64,
    pub lectures_completed: i64,
    /// Approx seconds watched (full duration for completed, else resume pos).
    pub watched_seconds: f64,
    /// Total duration of every lecture in the library.
    pub library_seconds: f64,
    pub days_active: i64,
    pub current_streak: i64,
    pub bookmarks_total: i64,
}

/// A non-media resource that shipped with a course (pdf, zip, code, …).
#[derive(Debug, Clone, Serialize)]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub file_path: String,
    pub kind: Option<String>,
    pub section_id: Option<String>,
    pub lecture_id: Option<String>,
}

/// One full-text search result across courses/sections/lectures/attachments.
#[derive(Debug, Clone, Serialize)]
pub struct SearchHit {
    pub kind: String,
    pub entity_id: String,
    pub course_id: String,
    pub course_title: String,
    pub title: String,
}

/// A subtitle full-text match: a spoken snippet with the lecture + timestamp.
#[derive(Debug, Clone, Serialize)]
pub struct SubtitleHit {
    pub lecture_id: String,
    pub course_id: String,
    pub course_title: String,
    pub lecture_title: String,
    pub start_ms: i64,
    pub snippet: String,
}

/// A bookmark plus the lecture/course context the global bookmarks page needs
/// to group and jump into it.
#[derive(Debug, Clone, Serialize)]
pub struct BookmarkDetail {
    pub id: String,
    pub lecture_id: String,
    pub lecture_title: String,
    pub section_title: String,
    pub course_id: String,
    pub course_title: String,
    pub position_seconds: f64,
    pub label: Option<String>,
    pub created_at: i64,
}
