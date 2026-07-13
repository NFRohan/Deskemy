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
