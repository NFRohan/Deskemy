// Shared reactive app state (Svelte 5 runes in a module).

import { api } from "$lib/api";
import type { CourseSummary } from "$lib/types";

export const library = $state<{
  courses: CourseSummary[];
  loading: boolean;
  error: string | null;
  loaded: boolean;
}>({
  courses: [],
  loading: false,
  error: null,
  loaded: false,
});

export async function loadLibrary(force = false): Promise<void> {
  if (library.loading) return;
  if (library.loaded && !force) return;
  library.loading = true;
  library.error = null;
  try {
    library.courses = await api.listCourses();
    library.loaded = true;
  } catch (e: any) {
    library.error = e?.message ?? String(e);
  } finally {
    library.loading = false;
  }
}

export interface Crumb {
  label: string;
  href?: string;
}

export const ui = $state<{ crumbs: Crumb[]; immersive: boolean; sidebarCollapsed: boolean }>({
  crumbs: [{ label: "Library" }],
  immersive: false,
  sidebarCollapsed: false,
});

export function setCrumbs(crumbs: Crumb[]): void {
  ui.crumbs = crumbs;
}

/** Immersive/cinema mode hides the sidebar + titlebar so the video fills the window. */
export function setImmersive(on: boolean): void {
  ui.immersive = on;
}

/** Collapse the left sidebar to an icon rail (more room for course content). */
export function toggleSidebar(): void {
  ui.sidebarCollapsed = !ui.sidebarCollapsed;
}
