// In-app updates: check on launch, notify via a banner + Settings, and only
// download/install when the user opts in. The installer build self-installs via
// the Tauri updater; a portable copy can't replace its own loose files, so it's
// pointed at the release page instead.
import { check, type Update } from "@tauri-apps/plugin-updater";
import { invoke } from "@tauri-apps/api/core";
import { openUrl } from "@tauri-apps/plugin-opener";

const RELEASES_URL = "https://github.com/NFRohan/Deskemy/releases/latest";

export const updates = $state<{
  available: { version: string; notes: string } | null;
  handle: Update | null;
  checking: boolean;
  installing: boolean;
  dismissed: boolean;
  error: string | null;
}>({
  available: null,
  handle: null,
  checking: false,
  installing: false,
  dismissed: false,
  error: null,
});

/** Ask the update endpoint whether a newer version exists. Safe to call on
 *  launch — a missing manifest / offline just leaves `available` null. */
export async function checkForUpdate(): Promise<void> {
  if (updates.checking) return;
  updates.checking = true;
  updates.error = null;
  try {
    const u = await check();
    if (u?.available) {
      updates.handle = u;
      updates.available = { version: u.version, notes: u.body ?? "" };
    } else {
      updates.handle = null;
      updates.available = null;
    }
  } catch (e: any) {
    updates.error = e?.message ?? String(e);
  } finally {
    updates.checking = false;
  }
}

/** User-initiated. Installer builds download + install (then relaunch); portable
 *  copies open the release page to grab the new zip manually. */
export async function installUpdate(): Promise<void> {
  if (!updates.handle || updates.installing) return;
  updates.installing = true;
  updates.error = null;
  try {
    const portable = await invoke<boolean>("is_portable").catch(() => false);
    if (portable) {
      await openUrl(RELEASES_URL);
      updates.installing = false;
      return;
    }
    // On Windows the NSIS updater replaces the app and relaunches it.
    await updates.handle.downloadAndInstall();
  } catch (e: any) {
    updates.error = e?.message ?? String(e);
    updates.installing = false;
  }
}
