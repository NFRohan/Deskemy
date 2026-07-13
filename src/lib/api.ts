// Typed wrappers over the namespaced Rust commands.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppConfig,
  CourseDetail,
  CourseSummary,
  LectureView,
  MediaTracks,
  PlayerState,
  RootDto,
  ScanResult,
} from "./types";

export const api = {
  health: () => invoke<string>("app_health"),

  // library_*
  addRoot: (path: string) => invoke<string>("library_add_root", { path }),
  listRoots: () => invoke<RootDto[]>("library_list_roots"),
  removeRoot: (id: string) => invoke<void>("library_remove_root", { id }),
  importCourse: (path: string) => invoke<string>("library_import_course", { path }),
  scanRoot: (rootId: string) => invoke<ScanResult>("library_scan_root", { rootId }),
  listCourses: () => invoke<CourseSummary[]>("library_list_courses"),

  // course_*
  getCourse: (id: string) => invoke<CourseDetail | null>("course_get", { id }),
  setFavorite: (id: string, favorite: boolean) =>
    invoke<void>("course_set_favorite", { id, favorite }),
  touchOpened: (id: string) => invoke<void>("course_touch_opened", { id }),
  setLectureCompleted: (lectureId: string, completed: boolean) =>
    invoke<void>("lecture_set_completed", { lectureId, completed }),

  // config_*
  getConfig: () => invoke<AppConfig>("config_get"),
  setConfig: (config: AppConfig) => invoke<void>("config_set", { config }),

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
