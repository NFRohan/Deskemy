<script lang="ts">
  import { onMount } from "svelte";
  import {
    Settings,
    RefreshCw,
    Trash2,
    FileSearch,
    Captions,
    Minimize2,
    LoaderCircle,
    Download,
    Upload,
  } from "@lucide/svelte";
  import { api, pickBackupDest, pickBackupSource } from "$lib/api";
  import { setCrumbs, loadLibrary, applyTheme } from "$lib/stores/app.svelte";
  import type { AppConfig, StorageStats } from "$lib/types";

  const THEMES = [
    { value: "dark", label: "Dark" },
    { value: "light", label: "Light" },
    { value: "system", label: "System" },
  ];

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

  let config = $state<AppConfig | null>(null);
  let busy = $state<string | null>(null);
  let results = $state<Record<string, string>>({});
  let storage = $state<StorageStats | null>(null);

  onMount(async () => {
    setCrumbs([{ label: "Settings" }]);
    config = await api.getConfig().catch(() => null);
    storage = await api.storageStats().catch(() => null);
  });

  async function refreshStorage() {
    storage = await api.storageStats().catch(() => null);
  }

  async function save() {
    if (config) await api.setConfig($state.snapshot(config)).catch(() => {});
  }
  function onSpeed(e: Event) {
    if (!config) return;
    config.default_speed = +(e.target as HTMLSelectElement).value;
    save();
  }
  function toggleAutoplay() {
    if (!config) return;
    config.autoplay_next = !config.autoplay_next;
    save();
  }
  function toggleAutoRescan() {
    if (!config) return;
    config.auto_rescan = !config.auto_rescan;
    save();
  }
  function onTheme(e: Event) {
    if (!config) return;
    config.theme = (e.target as HTMLSelectElement).value;
    applyTheme(config.theme);
    save();
  }
  function onGoal(e: Event) {
    if (!config) return;
    config.daily_goal_minutes = +(e.target as HTMLSelectElement).value;
    save();
  }

  async function run(key: string, fn: () => Promise<string>) {
    if (busy) return;
    busy = key;
    try {
      results[key] = await fn();
    } catch (e: any) {
      results[key] = e?.message ?? String(e);
    } finally {
      busy = null;
    }
  }

  function fmtBytes(n: number): string {
    if (n < 1024) return `${n} B`;
    if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
    return `${(n / 1024 / 1024).toFixed(1)} MB`;
  }
  const plural = (n: number, w: string) => `${n} ${w}${n === 1 ? "" : "s"}`;

  const checkMissing = () =>
    run("reconcile", async () => {
      const r = await api.reconcileLibrary();
      await loadLibrary(true);
      return r.files_missing === 0
        ? `All files present across ${plural(r.courses_checked, "course")}.`
        : `${plural(r.files_missing, "missing file")} across ${plural(
            r.courses_missing,
            "course",
          )} — flagged in the library.`;
    });
  const reindex = () =>
    run("reindex", async () => `Reindexed ${plural(await api.reindexSearch(), "item")}.`);
  const indexSubs = () =>
    run("subs", async () => {
      const n = await api.reindexSubtitles();
      return n === 0
        ? "No sidecar subtitle files found."
        : `Indexed ${plural(n, "subtitle line")}.`;
    });
  const cleanThumbs = () =>
    run("gc", async () => {
      const r = await api.gcThumbnails();
      await refreshStorage();
      return r.removed === 0
        ? "Cache already clean."
        : `Removed ${plural(r.removed, "file")} (${fmtBytes(r.freed_bytes)}).`;
    });
  const compactDb = () =>
    run("compact", async () => {
      const bytes = await api.compactDb();
      await refreshStorage();
      return `Database compacted — now ${fmtBytes(bytes)}.`;
    });
  const clearSubs = () =>
    run("clearsubs", async () => {
      const n = await api.clearSubtitleIndex();
      await refreshStorage();
      return n === 0
        ? "Subtitle index already empty."
        : `Cleared ${plural(n, "cue")}. Compact the database to reclaim the space.`;
    });

  // Data export / import
  let importSrc = $state<string | null>(null);
  let showImportConfirm = $state(false);
  let importing = $state(false);

  function exportData() {
    pickBackupDest("deskemy-backup.zip").then((dest) => {
      if (!dest) return;
      run("export", async () => {
        await api.dataExport(dest);
        return "Backup saved.";
      });
    });
  }
  function chooseImport() {
    pickBackupSource().then((src) => {
      if (src) {
        importSrc = src;
        showImportConfirm = true;
      }
    });
  }
  async function confirmImport() {
    if (!importSrc || importing) return;
    importing = true;
    results.import = "";
    try {
      await api.dataImport(importSrc); // relaunches on success — never resolves
    } catch (e: any) {
      results.import = e?.message ?? String(e);
      importing = false;
      showImportConfirm = false;
    }
  }
</script>

<div class="p-6 max-w-2xl mx-auto space-y-8">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <Settings size={18} /> Settings
  </h2>

  {#if config}
    <!-- Appearance -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Appearance</h3>
      <div
        class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="text-body-md text-on-surface">Theme</p>
            <p class="text-label-sm text-on-surface-variant">Follow the system or force light/dark.</p>
          </div>
          <select
            value={config.theme}
            onchange={onTheme}
            class="bg-background border border-outline-variant rounded-lg text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
          >
            {#each THEMES as t (t.value)}
              <option value={t.value}>{t.label}</option>
            {/each}
          </select>
        </div>
      </div>
    </section>

    <!-- Playback -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Playback</h3>
      <div
        class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="text-body-md text-on-surface">Default playback speed</p>
            <p class="text-label-sm text-on-surface-variant">Applied when a lecture opens.</p>
          </div>
          <select
            value={config.default_speed}
            onchange={onSpeed}
            class="bg-background border border-outline-variant rounded-lg text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
          >
            {#each SPEEDS as s (s)}
              <option value={s}>{s}×</option>
            {/each}
          </select>
        </div>
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="text-body-md text-on-surface">Autoplay next lecture</p>
            <p class="text-label-sm text-on-surface-variant">Advance automatically when one ends.</p>
          </div>
          <button
            onclick={toggleAutoplay}
            role="switch"
            aria-checked={config.autoplay_next}
            aria-label="Autoplay next lecture"
            class="relative w-11 h-6 rounded-full transition-colors shrink-0
              {config.autoplay_next ? 'bg-primary-container' : 'bg-surface-container-highest'}"
          >
            <span
              class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform
                {config.autoplay_next ? 'translate-x-5' : ''}"
            ></span>
          </button>
        </div>
        <div class="flex items-center justify-between gap-4 p-4">
          <div>
            <p class="text-body-md text-on-surface">Daily goal</p>
            <p class="text-label-sm text-on-surface-variant">Target watch time per day (Stats).</p>
          </div>
          <select
            value={config.daily_goal_minutes}
            onchange={onGoal}
            class="bg-background border border-outline-variant rounded-lg text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
          >
            {#each [15, 30, 45, 60, 90, 120] as m (m)}
              <option value={m}>{m} min</option>
            {/each}
          </select>
        </div>
      </div>
    </section>

    <!-- Library maintenance -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">
        Library maintenance
      </h3>
      <div
        class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Auto-rescan folders</p>
            <p class="text-label-sm text-on-surface-variant">
              Re-import a course when its folder changes on disk. May briefly pause the UI while it
              re-scans; the course you're watching is never touched.
            </p>
          </div>
          <button
            onclick={toggleAutoRescan}
            role="switch"
            aria-checked={config.auto_rescan}
            aria-label="Auto-rescan folders"
            class="relative w-11 h-6 rounded-full transition-colors shrink-0
              {config.auto_rescan ? 'bg-primary-container' : 'bg-surface-container-highest'}"
          >
            <span
              class="absolute top-0.5 left-0.5 w-5 h-5 rounded-full bg-white transition-transform
                {config.auto_rescan ? 'translate-x-5' : ''}"
            ></span>
          </button>
        </div>
        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Check for missing files</p>
            <p class="text-label-sm text-on-surface-variant">
              Flag courses whose video files have moved or been deleted.
            </p>
            {#if results.reconcile}<p class="text-label-sm text-primary mt-1">{results.reconcile}</p>{/if}
          </div>
          <button
            onclick={checkMissing}
            disabled={busy !== null}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            {#if busy === "reconcile"}<LoaderCircle size={15} class="animate-spin" />{:else}<FileSearch
                size={15}
              />{/if} Check
          </button>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Rebuild search index</p>
            <p class="text-label-sm text-on-surface-variant">
              Refresh full-text search from the current library.
            </p>
            {#if results.reindex}<p class="text-label-sm text-primary mt-1">{results.reindex}</p>{/if}
          </div>
          <button
            onclick={reindex}
            disabled={busy !== null}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            {#if busy === "reindex"}<LoaderCircle size={15} class="animate-spin" />{:else}<RefreshCw
                size={15}
              />{/if} Rebuild
          </button>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Index subtitle text</p>
            <p class="text-label-sm text-on-surface-variant">
              Parse sidecar subtitles so Search can find spoken words.
            </p>
            {#if results.subs}<p class="text-label-sm text-primary mt-1">{results.subs}</p>{/if}
          </div>
          <button
            onclick={indexSubs}
            disabled={busy !== null}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            {#if busy === "subs"}<LoaderCircle size={15} class="animate-spin" />{:else}<Captions
                size={15}
              />{/if} Index
          </button>
        </div>

      </div>
    </section>

    <!-- Data -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Data</h3>
      <div
        class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Export backup</p>
            <p class="text-label-sm text-on-surface-variant">
              Save your library — progress, bookmarks, tags, tracks, thumbnails — to one
              <code>.zip</code>. Your video files aren't included. Handy for moving a portable copy
              to a new release, or as a safety backup.
            </p>
            {#if results.export}<p class="text-label-sm text-primary mt-1">{results.export}</p>{/if}
          </div>
          <button
            onclick={exportData}
            disabled={busy !== null}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            {#if busy === "export"}<LoaderCircle size={15} class="animate-spin" />{:else}<Download
                size={15}
              />{/if} Export
          </button>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Import backup</p>
            <p class="text-label-sm text-on-surface-variant">
              Replace this library with a backup, then restart. It re-links to your videos where they
              still exist — it can't restore progress onto a different download of a course.
            </p>
            {#if results.import}<p class="text-label-sm text-error mt-1">{results.import}</p>{/if}
          </div>
          <button
            onclick={chooseImport}
            disabled={busy !== null || importing}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            <Upload size={15} /> Import
          </button>
        </div>
      </div>
    </section>

    <!-- Storage -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Storage</h3>
      <div
        class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant"
      >
        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Database</p>
            <p class="text-label-sm text-on-surface-variant">
              Course metadata, progress, bookmarks and search indexes — videos are never stored here.
              Compact reclaims space freed by removed courses or cleared indexes.
            </p>
            {#if results.compact}<p class="text-label-sm text-primary mt-1">{results.compact}</p>{/if}
          </div>
          <div class="shrink-0 flex flex-col items-end gap-2">
            <span class="text-label-md text-on-surface tabular-nums">
              {storage ? fmtBytes(storage.db_bytes) : "—"}
            </span>
            <button
              onclick={compactDb}
              disabled={busy !== null}
              class="inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
            >
              {#if busy === "compact"}<LoaderCircle size={15} class="animate-spin" />{:else}<Minimize2
                  size={15}
                />{/if} Compact
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Subtitle search index</p>
            <p class="text-label-sm text-on-surface-variant">
              Indexed subtitle text — the main database growth. Clearing frees the most space;
              re-index anytime with "Index subtitle text" above.
            </p>
            {#if results.clearsubs}<p class="text-label-sm text-primary mt-1">{results.clearsubs}</p>{/if}
          </div>
          <div class="shrink-0 flex flex-col items-end gap-2">
            <span class="text-label-md text-on-surface tabular-nums">
              {storage ? `${storage.subtitle_cues.toLocaleString()} cues` : "—"}
            </span>
            <button
              onclick={clearSubs}
              disabled={busy !== null || storage?.subtitle_cues === 0}
              class="inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
            >
              {#if busy === "clearsubs"}<LoaderCircle size={15} class="animate-spin" />{:else}<Trash2
                  size={15}
                />{/if} Clear
            </button>
          </div>
        </div>

        <div class="flex items-center justify-between gap-4 p-4">
          <div class="min-w-0">
            <p class="text-body-md text-on-surface">Thumbnail cache</p>
            <p class="text-label-sm text-on-surface-variant">
              Cached course covers and resume frames (stored on disk, not in the database). Clean
              deletes images no longer used by any course.
            </p>
            {#if results.gc}<p class="text-label-sm text-primary mt-1">{results.gc}</p>{/if}
          </div>
          <div class="shrink-0 flex flex-col items-end gap-2">
            <span class="text-label-md text-on-surface tabular-nums">
              {storage ? fmtBytes(storage.thumbnail_bytes) : "—"}
            </span>
            <button
              onclick={cleanThumbs}
              disabled={busy !== null}
              class="inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
            >
              {#if busy === "gc"}<LoaderCircle size={15} class="animate-spin" />{:else}<Trash2
                  size={15}
                />{/if} Clean
            </button>
          </div>
        </div>
      </div>
    </section>
  {/if}
</div>

{#if showImportConfirm}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
    <div
      class="w-full max-w-md bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4"
      role="dialog"
      aria-modal="true"
    >
      <h3 class="text-headline-sm text-on-surface">Import backup?</h3>
      <p class="text-body-sm text-on-surface-variant">
        This replaces your current library — progress, bookmarks, tags, everything — with the
        backup, then restarts Deskemy. Your current data can't be recovered afterward unless you
        exported it first.
      </p>
      <div class="flex justify-end gap-2">
        <button
          onclick={() => (showImportConfirm = false)}
          disabled={importing}
          class="text-label-md text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-50"
        >
          Cancel
        </button>
        <button
          onclick={confirmImport}
          disabled={importing}
          class="inline-flex items-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-4 py-2 rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
        >
          {#if importing}<LoaderCircle size={15} class="animate-spin" />{/if} Import &amp; restart
        </button>
      </div>
    </div>
  </div>
{/if}
