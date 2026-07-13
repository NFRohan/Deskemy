<script lang="ts">
  import { page } from "$app/stores";
  import { Library, Bookmark, Star, Search, Settings, FolderPlus, LoaderCircle } from "@lucide/svelte";
  import { api, pickFolder } from "$lib/api";
  import { loadLibrary, ui } from "$lib/stores/app.svelte";

  let importing = $state(false);
  let importError = $state<string | null>(null);

  const nav = [
    { href: "/", label: "Library", icon: Library },
    { href: "/bookmarks", label: "Bookmarks", icon: Bookmark },
    { href: "/favorites", label: "Favorites", icon: Star },
    { href: "/search", label: "Search", icon: Search },
    { href: "/settings", label: "Settings", icon: Settings },
  ];

  function isActive(href: string): boolean {
    const p = $page.url.pathname;
    if (href === "/") return p === "/" || p.startsWith("/course");
    return p.startsWith(href);
  }

  async function addFolder() {
    const path = await pickFolder();
    if (!path) return;
    importing = true;
    importError = null;
    try {
      await api.importCourse(path);
      await loadLibrary(true);
    } catch (e: any) {
      importError = e?.message ?? String(e);
    } finally {
      importing = false;
    }
  }
</script>

<aside
  class="{ui.sidebarCollapsed ? 'w-16' : 'w-[240px]'} shrink-0 h-full bg-surface-container-low
    border-r border-outline-variant flex flex-col py-6 transition-[width] duration-200 overflow-hidden"
>
  <!-- Brand -->
  <div class="mb-8 flex items-center gap-3 {ui.sidebarCollapsed ? 'justify-center px-0' : 'px-6'}">
    <div
      class="w-8 h-8 rounded-lg bg-primary-container flex items-center justify-center font-bold text-on-primary-container shrink-0"
    >
      D
    </div>
    {#if !ui.sidebarCollapsed}
      <div class="min-w-0">
        <h1 class="text-headline-md text-primary">Deskemy</h1>
        <p class="text-label-sm text-on-surface-variant">Power Learner</p>
      </div>
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
      disabled={importing}
      title="Add Folder"
      class="w-full py-2 flex items-center justify-center gap-2 bg-primary-container text-on-primary-container text-label-md rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
    >
      {#if importing}
        <LoaderCircle size={18} class="animate-spin shrink-0" />
        {#if !ui.sidebarCollapsed}Importing…{/if}
      {:else}
        <FolderPlus size={18} class="shrink-0" />
        {#if !ui.sidebarCollapsed}Add Folder{/if}
      {/if}
    </button>
  </div>
</aside>
