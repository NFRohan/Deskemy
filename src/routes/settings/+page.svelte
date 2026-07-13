<script lang="ts">
  import { onMount } from "svelte";
  import { Settings, RefreshCw, Trash2, FileSearch, LoaderCircle } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs, loadLibrary, applyTheme } from "$lib/stores/app.svelte";
  import type { AppConfig } from "$lib/types";

  const THEMES = [
    { value: "dark", label: "Dark" },
    { value: "light", label: "Light" },
    { value: "system", label: "System" },
  ];

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

  let config = $state<AppConfig | null>(null);
  let busy = $state<string | null>(null);
  let results = $state<Record<string, string>>({});

  onMount(async () => {
    setCrumbs([{ label: "Settings" }]);
    config = await api.getConfig().catch(() => null);
  });

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
  function onTheme(e: Event) {
    if (!config) return;
    config.theme = (e.target as HTMLSelectElement).value;
    applyTheme(config.theme);
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
  const cleanThumbs = () =>
    run("gc", async () => {
      const r = await api.gcThumbnails();
      return r.removed === 0
        ? "Cache already clean."
        : `Removed ${plural(r.removed, "file")} (${fmtBytes(r.freed_bytes)}).`;
    });
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
            class="bg-background border border-outline-variant rounded text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
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
            class="bg-background border border-outline-variant rounded text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
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
            <p class="text-body-md text-on-surface">Clean up thumbnail cache</p>
            <p class="text-label-sm text-on-surface-variant">
              Delete cached images no longer used by any course.
            </p>
            {#if results.gc}<p class="text-label-sm text-primary mt-1">{results.gc}</p>{/if}
          </div>
          <button
            onclick={cleanThumbs}
            disabled={busy !== null}
            class="shrink-0 inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors disabled:opacity-60"
          >
            {#if busy === "gc"}<LoaderCircle size={15} class="animate-spin" />{:else}<Trash2
                size={15}
              />{/if} Clean
          </button>
        </div>
      </div>
    </section>
  {/if}
</div>
