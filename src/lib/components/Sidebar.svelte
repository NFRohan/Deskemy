<script lang="ts">
  import { page } from "$app/stores";
  import {
    Library,
    Route,
    History,
    Bookmark,
    Star,
    Search,
    ChartColumn,
    Settings,
    FolderPlus,
    LoaderCircle,
    PanelLeftClose,
    PanelLeftOpen,
    X,
    Layers,
    Video,
    Paperclip,
    Captions,
    TriangleAlert,
  } from "@lucide/svelte";
  import { api, pickFolder } from "$lib/api";
  import { listen } from "@tauri-apps/api/event";
  import { loadLibrary, toggleSidebar, ui } from "$lib/stores/app.svelte";
  import { formatDuration } from "$lib/format";
  import type { ImportPreview } from "$lib/types";

  let importing = $state(false);
  let scanning = $state(false);
  let scanProgress = $state<{ done: number; total: number } | null>(null);
  let importError = $state<string | null>(null);
  let preview = $state<ImportPreview | null>(null);
  let previewPath = $state<string | null>(null);

  const nav = [
    { href: "/", label: "Library", icon: Library },
    { href: "/tracks", label: "Career Tracks", icon: Route },
    { href: "/history", label: "History", icon: History },
    { href: "/bookmarks", label: "Bookmarks", icon: Bookmark },
    { href: "/favorites", label: "Favorites", icon: Star },
    { href: "/search", label: "Search", icon: Search },
    { href: "/stats", label: "Stats", icon: ChartColumn },
    { href: "/settings", label: "Settings", icon: Settings },
  ];

  function isActive(href: string): boolean {
    const p = $page.url.pathname;
    if (href === "/") return p === "/" || p.startsWith("/course");
    return p.startsWith(href);
  }

  // Pick a folder → dry-run preview (probes off the DB lock) → confirm imports
  // the already-probed plan (no re-probe).
  async function addFolder() {
    const path = await pickFolder();
    if (!path) return;
    scanning = true;
    scanProgress = null;
    importError = null;
    // Live per-video probe progress while previewing.
    const un = await listen<[number, number]>("import:progress", (e) => {
      scanProgress = { done: e.payload[0], total: e.payload[1] };
    });
    try {
      preview = await api.previewImport(path);
      previewPath = path;
    } catch (e: any) {
      importError = e?.message ?? String(e);
    } finally {
      un();
      scanning = false;
      scanProgress = null;
    }
  }

  async function confirmImport() {
    if (!previewPath || importing) return;
    importing = true;
    try {
      await api.importCourse(previewPath);
      await loadLibrary(true);
      preview = null;
      previewPath = null;
    } catch (e: any) {
      importError = e?.message ?? String(e);
    } finally {
      importing = false;
    }
  }

  function cancelPreview() {
    if (importing) return;
    preview = null;
    previewPath = null;
  }
</script>

<aside
  class="{ui.sidebarCollapsed ? 'w-16' : 'w-[240px]'} shrink-0 h-full bg-surface-container-low
    border-r border-outline-variant flex flex-col py-6 transition-[width] duration-200 overflow-hidden"
>
  <!-- Brand + collapse control -->
  <div class="mb-8 flex items-center gap-3 {ui.sidebarCollapsed ? 'justify-center px-2' : 'px-6'}">
    {#if ui.sidebarCollapsed}
      <!-- Collapsed: the logo becomes the expander -->
      <button
        onclick={toggleSidebar}
        class="p-2 rounded-lg text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest transition-colors"
        title="Expand sidebar"
        aria-label="Expand sidebar"
      >
        <PanelLeftOpen size={20} />
      </button>
    {:else}
      <div
        class="w-8 h-8 rounded-lg bg-primary-container flex items-center justify-center font-bold text-on-primary-container shrink-0"
      >
        D
      </div>
      <div class="min-w-0 flex-1">
        <h1 class="text-headline-md text-primary">Deskemy</h1>
      </div>
      <button
        onclick={toggleSidebar}
        class="p-1.5 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest transition-colors shrink-0"
        title="Collapse sidebar"
        aria-label="Collapse sidebar"
      >
        <PanelLeftClose size={18} />
      </button>
    {/if}
  </div>

  <!-- Nav -->
  <nav class="flex-1 px-2 space-y-1 text-body-md">
    {#each nav as item (item.href)}
      <a
        href={item.href}
        title={item.label}
        class="flex items-center gap-3 py-2 rounded-r border-l-4 transition-colors
          {ui.sidebarCollapsed ? 'justify-center px-0' : 'px-4'}
          {isActive(item.href)
          ? 'border-secondary-container bg-surface-container-high text-on-surface'
          : 'border-transparent text-on-surface-variant hover:bg-surface-container-highest'}"
      >
        <item.icon size={20} class="shrink-0" />
        {#if !ui.sidebarCollapsed}<span class="truncate">{item.label}</span>{/if}
      </a>
    {/each}
  </nav>

  <!-- Add Folder -->
  <div class="mt-auto space-y-2 {ui.sidebarCollapsed ? 'px-2' : 'px-4'}">
    {#if importError && !ui.sidebarCollapsed}
      <p class="text-label-sm text-error line-clamp-2">{importError}</p>
    {/if}
    <button
      onclick={addFolder}
      disabled={scanning || importing}
      title="Add Folder"
      class="w-full py-2 flex items-center justify-center gap-2 bg-primary-container text-on-primary-container text-label-md rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
    >
      {#if scanning}
        <LoaderCircle size={18} class="animate-spin shrink-0" />
        {#if !ui.sidebarCollapsed}
          {scanProgress ? `Probing ${scanProgress.done}/${scanProgress.total}` : "Scanning…"}
        {/if}
      {:else}
        <FolderPlus size={18} class="shrink-0" />
        {#if !ui.sidebarCollapsed}Add Folder{/if}
      {/if}
    </button>
  </div>
</aside>

<!-- Import preview -->
{#if preview}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    onclick={(e) => e.target === e.currentTarget && cancelPreview()}
    role="presentation"
  >
    <div class="w-full max-w-md bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4">
      <div class="flex items-start justify-between gap-3">
        <div class="min-w-0">
          <p class="text-label-sm text-on-surface-variant">Import course</p>
          <h3 class="text-headline-sm text-on-surface truncate">{preview.title}</h3>
        </div>
        <button
          onclick={cancelPreview}
          disabled={importing}
          class="p-1 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest disabled:opacity-50"
          aria-label="Cancel"
        >
          <X size={18} />
        </button>
      </div>

      {#if preview.is_reimport}
        <p class="text-label-sm text-secondary-container bg-secondary-container/10 border border-secondary-container/25 rounded-lg px-3 py-2">
          Already in your library — re-importing keeps your progress, bookmarks and tags.
        </p>
      {/if}

      <div class="grid grid-cols-2 gap-2">
        <div class="flex items-center gap-2 bg-surface-container-low rounded-lg px-3 py-2.5">
          <Layers size={16} class="text-on-surface-variant shrink-0" />
          <span class="text-body-md text-on-surface"><b class="tabular-nums">{preview.sections}</b> sections</span>
        </div>
        <div class="flex items-center gap-2 bg-surface-container-low rounded-lg px-3 py-2.5">
          <Video size={16} class="text-on-surface-variant shrink-0" />
          <span class="text-body-md text-on-surface"><b class="tabular-nums">{preview.lectures}</b> lectures</span>
        </div>
        <div class="flex items-center gap-2 bg-surface-container-low rounded-lg px-3 py-2.5">
          <Paperclip size={16} class="text-on-surface-variant shrink-0" />
          <span class="text-body-md text-on-surface"><b class="tabular-nums">{preview.resources}</b> resources</span>
        </div>
        <div class="flex items-center gap-2 bg-surface-container-low rounded-lg px-3 py-2.5">
          <Captions size={16} class="text-on-surface-variant shrink-0" />
          <span class="text-body-md text-on-surface"><b class="tabular-nums">{preview.subtitles}</b> subtitles</span>
        </div>
      </div>

      {#if preview.total_duration}
        <p class="text-label-sm text-on-surface-variant">Total runtime · {formatDuration(preview.total_duration)}</p>
      {/if}
      {#if preview.unplayable > 0}
        <p class="flex items-center gap-1.5 text-label-sm text-error">
          <TriangleAlert size={14} class="shrink-0" />
          {preview.unplayable} video{preview.unplayable === 1 ? "" : "s"} couldn't be opened — imported but flagged.
        </p>
      {/if}
      {#if importError}
        <p class="text-label-sm text-error">{importError}</p>
      {/if}

      <div class="flex justify-end gap-2 pt-1">
        <button
          onclick={cancelPreview}
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
          {#if importing}<LoaderCircle size={15} class="animate-spin" />{/if}
          {preview.is_reimport ? "Re-import" : "Import"}
        </button>
      </div>
    </div>
  </div>
{/if}
