<script lang="ts">
  import { page } from "$app/stores";
  import { Library, Star, Search, Settings, FolderPlus, LoaderCircle } from "@lucide/svelte";
  import { api, pickFolder } from "$lib/api";
  import { loadLibrary } from "$lib/stores/app.svelte";

  let importing = $state(false);
  let importError = $state<string | null>(null);

  const nav = [
    { href: "/", label: "Library", icon: Library },
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
  class="w-[240px] shrink-0 h-full bg-surface-container-low border-r border-outline-variant flex flex-col py-6"
>
  <!-- Brand -->
  <div class="px-6 mb-8 flex items-center gap-3">
    <div
      class="w-8 h-8 rounded-lg bg-primary-container flex items-center justify-center font-bold text-on-primary-container"
    >
      D
    </div>
    <div>
      <h1 class="text-headline-md text-primary">Deskemy</h1>
      <p class="text-label-sm text-on-surface-variant">Power Learner</p>
    </div>
  </div>

  <!-- Nav -->
  <nav class="flex-1 px-2 space-y-1 text-body-md">
    {#each nav as item (item.href)}
      <a
        href={item.href}
        class="flex items-center gap-3 px-4 py-2 rounded-r border-l-4 transition-colors
          {isActive(item.href)
          ? 'border-secondary-container bg-surface-container-high text-on-surface'
          : 'border-transparent text-on-surface-variant hover:bg-surface-container-highest'}"
      >
        <item.icon size={20} />
        {item.label}
      </a>
    {/each}
  </nav>

  <!-- Add Folder -->
  <div class="px-4 mt-auto space-y-2">
    {#if importError}
      <p class="text-label-sm text-error line-clamp-2">{importError}</p>
    {/if}
    <button
      onclick={addFolder}
      disabled={importing}
      class="w-full py-2 flex items-center justify-center gap-2 bg-primary-container text-on-primary-container text-label-md rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
    >
      {#if importing}
        <LoaderCircle size={18} class="animate-spin" />
        Importing…
      {:else}
        <FolderPlus size={18} />
        Add Folder
      {/if}
    </button>
  </div>
</aside>
