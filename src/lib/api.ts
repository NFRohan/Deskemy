// Typed wrappers over the namespaced Rust commands.

import { invoke } from "@tauri-apps/api/core";
import { open, save } from "@tauri-apps/plugin-dialog";
import type {
  AppConfig,
  Attachment,
  Bookmark,
  BookmarkDetail,
  CourseDetail,
  CourseSummary,
  GcReport,
  HistoryEntry,
  ImportPreview,
  LectureView,
  LibraryStats,
  MediaTracks,
  PlayerState,
  ReconcileReport,
  RootDto,
  ScanResult,
  SearchHit,
  StorageStats,
  SubtitleHit,
  TrackDetail,
  TrackSummary,
} from "./types";

export const api = {
  health: () => invoke<string>("app_health"),
  // Phase-1 compositing spike (see docs/player-compositing.md).
  compositorTest: () => invoke<void>("compositor_test"),
  compositorEnabled: () => invoke<boolean>("compositor_enabled"),
  // Player fullscreen with a native-feeling maximized→fullscreen transition
  // (staged native-side so the window never visibly shrinks).
  windowSetImmersive: (on: boolean) => invoke<void>("window_set_immersive", { on }),

  // library_*
  addRoot: (path: string) => invoke<string>("library_add_root", { path }),
  listRoots: () => invoke<RootDto[]>("library_list_roots"),
  removeRoot: (id: string) => invoke<void>("library_remove_root", { id }),
  importCourse: (path: string) => invoke<string>("library_import_course", { path }),
  previewImport: (path: string) => invoke<ImportPreview>("library_preview_import", { path }),
  scanRoot: (rootId: string) => invoke<ScanResult>("library_scan_root", { rootId }),
  listCourses: () => invoke<CourseSummary[]>("library_list_courses"),

  // course_*
  getCourse: (id: string) => invoke<CourseDetail | null>("course_get", { id }),
  setFavorite: (id: string, favorite: boolean) =>
    invoke<void>("course_set_favorite", { id, favorite }),
  touchOpened: (id: string) => invoke<void>("course_touch_opened", { id }),
  deleteCourse: (id: string) => invoke<void>("library_delete_course", { id }),
  getCourseAttachments: (courseId: string) =>
    invoke<Attachment[]>("course_attachments", { courseId }),
  openResource: (path: string) => invoke<void>("open_resource", { path }),
  getCourseTags: (courseId: string) => invoke<string[]>("course_tags", { courseId }),
  addCourseTag: (courseId: string, tag: string) =>
    invoke<string[]>("course_add_tag", { courseId, tag }),
  removeCourseTag: (courseId: string, tag: string) =>
    invoke<string[]>("course_remove_tag", { courseId, tag }),
  setCourseThumbnailFile: (id: string, srcPath: string) =>
    invoke<string>("course_set_thumbnail_file", { id, srcPath }),
  setCourseThumbnailBytes: (id: string, dataBase64: string, ext: string | null) =>
    invoke<string>("course_set_thumbnail_bytes", { id, dataBase64, ext }),
  clearCourseThumbnail: (id: string) => invoke<void>("course_clear_thumbnail", { id }),
  setLectureCompleted: (lectureId: string, completed: boolean) =>
    invoke<void>("lecture_set_completed", { lectureId, completed }),

  // bookmark_*
  addBookmark: (lectureId: string, positionSeconds: number, label: string | null) =>
    invoke<Bookmark>("bookmark_add", { lectureId, positionSeconds, label }),
  listBookmarks: (lectureId: string) => invoke<Bookmark[]>("bookmark_list", { lectureId }),
  deleteBookmark: (id: string) => invoke<void>("bookmark_delete", { id }),
  listAllBookmarks: () => invoke<BookmarkDetail[]>("bookmark_list_all"),

  // history
  history: () => invoke<HistoryEntry[]>("history_list"),

  // track_* (career tracks)
  listTracks: () => invoke<TrackSummary[]>("track_list"),
  getTrack: (id: string) => invoke<TrackDetail | null>("track_get", { id }),
  createTrack: (name: string, description: string | null) =>
    invoke<string>("track_create", { name, description }),
  updateTrack: (id: string, name: string, description: string | null) =>
    invoke<void>("track_update", { id, name, description }),
  deleteTrack: (id: string) => invoke<void>("track_delete", { id }),
  trackAddCourse: (trackId: string, courseId: string) =>
    invoke<void>("track_add_course", { trackId, courseId }),
  trackRemoveCourse: (trackId: string, courseId: string) =>
    invoke<void>("track_remove_course", { trackId, courseId }),
  trackReorderCourses: (trackId: string, courseIds: string[]) =>
    invoke<void>("track_reorder_courses", { trackId, courseIds }),

  // search_*
  search: (query: string) => invoke<SearchHit[]>("search_query", { query }),
  reindexSearch: () => invoke<number>("search_reindex"),
  searchSubtitles: (query: string) => invoke<SubtitleHit[]>("subtitle_search", { query }),
  reindexSubtitles: () => invoke<number>("subtitles_reindex"),

  // stats
  getStats: () => invoke<LibraryStats>("stats_get"),

  // maintenance
  reconcileLibrary: () => invoke<ReconcileReport>("library_reconcile"),
  gcThumbnails: () => invoke<GcReport>("thumbnails_gc"),

  // storage
  storageStats: () => invoke<StorageStats>("storage_stats"),
  compactDb: () => invoke<number>("db_compact"),
  clearSubtitleIndex: () => invoke<number>("subtitle_index_clear"),

  // config_*
  getConfig: () => invoke<AppConfig>("config_get"),
  setConfig: (config: AppConfig) => invoke<void>("config_set", { config }),

  // data_* — export/import the library (db + config + thumbnails) as one zip
  dataExport: (dest: string) => invoke<void>("data_export", { dest }),
  dataImport: (src: string) => invoke<void>("data_import", { src }),

  // player_*
  playerAvailable: () => invoke<boolean>("player_available"),
  playerOpen: (lectureId: string) => invoke<void>("player_open", { lectureId }),
  playerTogglePause: () => invoke<void>("player_toggle_pause"),
  playerSetPaused: (paused: boolean) => invoke<void>("player_set_paused", { paused }),
  playerSeek: (position: number) => invoke<void>("player_seek", { position }),
  playerSetSpeed: (speed: number) => invoke<void>("player_set_speed", { speed }),
  playerNext: () => invoke<void>("player_next"),
  playerPrev: () => invoke<void>("player_prev"),
  playerSetRect: (x: number, y: number, w: number, h: number) =>
    invoke<void>("player_set_rect", { x, y, w, h }),
  playerStop: () => invoke<void>("player_stop"),
  playerGrabResumeFrame: (courseId: string) =>
    invoke<string | null>("player_grab_resume_frame", { courseId }),
  playerState: () => invoke<PlayerState | null>("player_state"),
  playerTracks: () => invoke<MediaTracks>("player_tracks"),
  playerSetSubtitle: (sid: number | null) => invoke<void>("player_set_subtitle", { sid }),
  playerSetAudio: (aid: number | null) => invoke<void>("player_set_audio", { aid }),
  playerSetChapter: (index: number) => invoke<void>("player_set_chapter", { index }),
  playerSetVolume: (volume: number) => invoke<void>("player_set_volume", { volume }),
  playerSetMuted: (muted: boolean) => invoke<void>("player_set_muted", { muted }),
  lectureGet: (id: string) => invoke<LectureView | null>("lecture_get", { id }),
};

/** Native folder picker. Returns the chosen path or null if cancelled. */
export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}

/** Native image-file picker. Returns the chosen path or null if cancelled. */
export async function pickImage(): Promise<string | null> {
  const result = await open({
    multiple: false,
    filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp", "bmp"] }],
  });
  return typeof result === "string" ? result : null;
}

const BACKUP_FILTER = [{ name: "Deskemy backup", extensions: ["zip"] }];

/** Save dialog for an export archive. Returns the chosen path or null. */
export async function pickBackupDest(defaultName: string): Promise<string | null> {
  return await save({ defaultPath: defaultName, filters: BACKUP_FILTER });
}

/** Open dialog for an import archive. Returns the chosen path or null. */
export async function pickBackupSource(): Promise<string | null> {
  const result = await open({ multiple: false, filters: BACKUP_FILTER });
  return typeof result === "string" ? result : null;
}
