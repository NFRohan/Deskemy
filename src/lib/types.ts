// Mirrors the Rust `domain` structs returned by commands.

export interface CourseSummary {
  id: string;
  title: string;
  folder_path: string;
  thumbnail_path: string | null;
  lecture_count: number;
  total_duration: number | null;
  is_favorite: boolean;
  scan_status: string;
  last_opened_at: number | null;
  completed_count: number;
  resume_thumbnail_path: string | null;
}

export interface Lecture {
  id: string;
  section_id: string;
  title: string;
  file_path: string;
  position: number;
  duration: number | null;
  container: string | null;
  video_codec: string | null;
  playable: boolean;
  position_seconds: number;
  completed: boolean;
}

export interface Section {
  id: string;
  title: string;
  position: number;
  lectures: Lecture[];
}

export interface CourseDetail {
  id: string;
  title: string;
  folder_path: string;
  thumbnail_path: string | null;
  total_duration: number | null;
  is_favorite: boolean;
  scan_status: string;
  last_opened_at: number | null;
  last_lecture_id: string | null;
  sections: Section[];
}

export interface RootDto {
  id: string;
  path: string;
}

export interface ScanResult {
  imported: number;
  errors: string[];
}

export interface AppConfig {
  theme: string;
  default_speed: number;
  autoplay_next: boolean;
  last_root: string | null;
}

export interface ReconcileReport {
  courses_checked: number;
  courses_missing: number;
  files_missing: number;
}

export interface GcReport {
  removed: number;
  freed_bytes: number;
}

export interface PlayerState {
  lecture_id: string | null;
  position: number;
  duration: number;
  paused: boolean;
  speed: number;
  eof: boolean;
  sid: number | null;
  aid: number | null;
  chapter: number;
  volume: number;
  muted: boolean;
}

export interface TrackInfo {
  id: number;
  kind: string;
  lang: string | null;
  title: string | null;
  codec: string | null;
  selected: boolean;
  filename: string | null;
}

export interface ChapterInfo {
  index: number;
  title: string | null;
  time: number;
}

export interface MediaTracks {
  audio: TrackInfo[];
  subtitle: TrackInfo[];
  chapters: ChapterInfo[];
}

export interface LectureView {
  id: string;
  title: string;
  course_id: string;
  course_title: string;
  section_title: string;
}

export interface Bookmark {
  id: string;
  lecture_id: string;
  position_seconds: number;
  label: string | null;
  created_at: number;
}

export interface Attachment {
  id: string;
  name: string;
  file_path: string;
  kind: string | null;
  section_id: string | null;
  lecture_id: string | null;
}

export interface SearchHit {
  kind: string; // course | section | lecture | attachment
  entity_id: string;
  course_id: string;
  course_title: string;
  title: string;
}

export interface BookmarkDetail {
  id: string;
  lecture_id: string;
  lecture_title: string;
  section_title: string;
  course_id: string;
  course_title: string;
  position_seconds: number;
  label: string | null;
  created_at: number;
}
