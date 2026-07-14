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

/// One day's watch activity, for the heatmap / weekly chart.
#[derive(Debug, Clone, Serialize)]
pub struct DayActivity {
    pub day: String, // YYYY-MM-DD
    pub watch_seconds: f64,
    pub lectures_completed: i64,
}

/// Aggregate library/watch statistics for the stats page.
#[derive(Debug, Clone, Serialize)]
pub struct LibraryStats {
    // Overview
    pub courses_total: i64,
    pub courses_completed: i64,
    pub courses_in_progress: i64,
    pub lectures_total: i64,
    pub lectures_completed: i64,
    /// Total duration of every lecture in the library.
    pub library_seconds: f64,
    /// Lifetime content watched (full duration for completed, else resume pos).
    pub watched_seconds: f64,
    pub bookmarks_total: i64,
    // Watch-time telemetry (real, from daily_activity)
    pub watch_seconds_today: f64,
    pub watch_seconds_week: f64,
    pub active_days_month: i64,
    // Streaks (a day counts with >= 15 min watched)
    pub current_streak: i64,
    pub best_streak: i64,
    // Velocity / records
    pub lectures_last_7: i64,
    pub best_day_seconds: f64,
    pub daily_goal_minutes: i64,
    // Most-focused (highest-completion in-progress) course
    pub focus_course_id: Option<String>,
    pub focus_course_title: Option<String>,
    pub focus_course_pct: i64,
    /// Daily series (oldest → today) for the heatmap + weekly chart.
    pub activity: Vec<DayActivity>,
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

/// A recently-watched lecture (a progress row + its lecture/course context) for
/// the playback-history page — grouped by day and deep-linked back into the
/// player to resume where you left off.
#[derive(Debug, Clone, Serialize)]
pub struct HistoryEntry {
    pub lecture_id: String,
    pub lecture_title: String,
    pub section_title: String,
    pub course_id: String,
    pub course_title: String,
    pub position_seconds: f64,
    pub duration: Option<f64>,
    pub completed: bool,
    pub last_watched_at: i64,
}

/// A career track (ordered grouping of courses) with aggregate completion, for
/// the tracks list. Completion is over every lecture in the track's courses.
#[derive(Debug, Clone, Serialize)]
pub struct TrackSummary {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub course_count: i64,
    pub total_lectures: i64,
    pub completed_lectures: i64,
}

/// A course as it appears inside a track: enough to render its card + progress.
#[derive(Debug, Clone, Serialize)]
pub struct TrackCourse {
    pub id: String,
    pub title: String,
    pub thumbnail_path: Option<String>,
    pub lecture_count: i64,
    pub completed_lectures: i64,
}

/// A track plus its ordered courses, for the track detail page.
#[derive(Debug, Clone, Serialize)]
pub struct TrackDetail {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub courses: Vec<TrackCourse>,
}

/// On-disk footprint of the local stores, for the Settings → Storage panel.
#[derive(Debug, Clone, Serialize)]
pub struct StorageStats {
    /// SQLite database files (main + WAL + shared-memory).
    pub db_bytes: u64,
    /// Content-addressed thumbnail cache directory.
    pub thumbnail_bytes: u64,
    /// Rows in the subtitle full-text index (the main DB growth vector).
    pub subtitle_cues: i64,
}
