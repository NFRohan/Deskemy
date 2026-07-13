// Typed wrappers over the namespaced Rust commands.

import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import type {
  AppConfig,
  CourseDetail,
  CourseSummary,
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

  // config_*
  getConfig: () => invoke<AppConfig>("config_get"),
  setConfig: (config: AppConfig) => invoke<void>("config_set", { config }),
};

/** Native folder picker. Returns the chosen path or null if cancelled. */
export async function pickFolder(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === "string" ? result : null;
}
