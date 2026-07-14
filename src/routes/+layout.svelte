<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import { onMount } from "svelte";
  import { page } from "$app/stores";
  import { listen } from "@tauri-apps/api/event";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import { api } from "$lib/api";
  import { ui, applyTheme, loadLibrary } from "$lib/stores/app.svelte";

  let { children } = $props();

  onMount(() => {
    (async () => {
      const cfg = await api.getConfig().catch(() => null);
      applyTheme(cfg?.theme ?? "dark");
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
    <!-- On /watch the video pane is black, so any uncovered gap must fall back to
         black too — not the near-white bg-background of the light theme. -->
    <main
      class="flex-1 min-h-0 {scrolls ? 'bg-background overflow-y-auto' : 'bg-black overflow-hidden'}"
    >
      {@render children()}
    </main>
  </div>
</div>
