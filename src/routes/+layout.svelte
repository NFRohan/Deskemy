<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TopBar from "$lib/components/TopBar.svelte";
  import { ui } from "$lib/stores/app.svelte";

  let { children } = $props();
</script>

<div class="flex h-screen overflow-hidden">
  {#if !ui.immersive}
    <Sidebar />
  {/if}
  <div class="flex-1 flex flex-col min-w-0">
    {#if !ui.immersive}
      <TopBar />
    {/if}
    <main class="flex-1 overflow-y-auto bg-background">
      {@render children()}
    </main>
  </div>
</div>
