<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { listen } from "@tauri-apps/api/event";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import ContextMenu from "$lib/components/ContextMenu.svelte";
  import { api } from "$lib/api";
  import { ui, applyTheme, loadLibrary } from "$lib/stores/app.svelte";
  import { updates, checkForUpdate, installUpdate } from "$lib/updates.svelte";
  import { Download, X, LoaderCircle } from "@lucide/svelte";

  let { children } = $props();

  onMount(() => {
    (async () => {
      const cfg = await api.getConfig().catch(() => null);
      applyTheme(cfg?.theme ?? "dark");
      // Compositing mode: mpv renders behind the webview, so the window (body)
      // must be transparent for the video to show through the watch pane. Other
      // pages stay opaque via their own bg-background, covering the video layer.
      ui.compositor = await api.compositorEnabled().catch(() => false);
      if (ui.compositor) {
        document.documentElement.style.background = "transparent";
        document.body.style.background = "transparent";
      }
      // Non-blocking: surface a newer release via the banner if there is one.
      void checkForUpdate();
    })();
    // Auto-rescan pushes this when a watched course folder changes.
    const unlisten = listen("library:changed", () => loadLibrary(true));
    return () => void unlisten.then((fn) => fn());
  });

  // The player renders into a native child window that does NOT move with DOM
  // scroll, so the watch route must never scroll — it manages its own fixed
  // layout. Other routes scroll their content normally.
  const scrolls = $derived(!$page.url.pathname.startsWith("/watch"));

  // DEV: Ctrl+Shift+G — Phase-1 compositing spike. Make the whole page
  // transparent and paint a DirectComposition magenta layer behind the webview.
  // If magenta shows through (top-left) → compositing works. Run it on a page
  // with no active player (Library), so the old mpv child window isn't on top.
  async function onGlobalKey(e: KeyboardEvent) {
    if (e.ctrlKey && e.shiftKey && (e.key === "G" || e.key === "g")) {
      e.preventDefault();
      for (const el of [document.documentElement, document.body, document.querySelector("main")]) {
        if (el) (el as HTMLElement).style.background = "transparent";
      }
      await api.compositorTest().catch((err) => console.error("compositor_test", err));
    }
  }
</script>

<svelte:window onkeydown={onGlobalKey} />

<!-- h-full (chained off html/body height:100%) rather than h-screen: 100vh can
     lag the fullscreen resize and leave a sliver of body background at the edge. -->
<div class="flex h-full overflow-hidden">
  {#if !ui.immersive}
    <Sidebar />
  {/if}
  <div class="flex-1 flex flex-col min-w-0">
    {#if !ui.immersive}
      <TopBar />
    {/if}
    {#if !ui.immersive && updates.available && !updates.dismissed}
      <div
        class="flex items-center gap-3 px-4 py-2 bg-primary-container text-on-primary-container text-label-md border-b border-outline-variant"
      >
        <Download size={16} class="shrink-0" />
        <span class="flex-1 min-w-0 truncate">Deskemy {updates.available.version} is available.</span>
        <button
          onclick={installUpdate}
          disabled={updates.installing}
          class="shrink-0 inline-flex items-center gap-1.5 bg-on-primary-container/10 hover:bg-on-primary-container/20 px-3 py-1 rounded transition-colors disabled:opacity-60"
        >
          {#if updates.installing}<LoaderCircle size={14} class="animate-spin" />{/if} Update
        </button>
        <button
          onclick={() => (updates.dismissed = true)}
          aria-label="Dismiss update notice"
          class="shrink-0 p-1 rounded hover:bg-on-primary-container/20 transition-colors"
        >
          <X size={16} />
        </button>
      </div>
    {/if}
    <!-- On /watch the video pane is black, so any uncovered gap falls back to
         black (not the light theme's bg-background) — unless we're compositing,
         where the pane must stay transparent to reveal the video behind it. -->
    <main
      class="flex-1 min-h-0 {scrolls
        ? 'bg-background overflow-y-auto'
        : ui.compositor
          ? 'overflow-hidden'
          : 'bg-black overflow-hidden'}"
    >
      {@render children()}
    </main>
  </div>
</div>

<ContextMenu />
