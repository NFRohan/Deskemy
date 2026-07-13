<script lang="ts">
  import "@fontsource-variable/inter";
  import "../app.css";
  import Sidebar from "$lib/components/Sidebar.svelte";
  import TopBar from "$lib/components/TopBar.svelte";

  let { children } = $props();
</script>

<div class="flex h-screen overflow-hidden">
  <Sidebar />
  <div class="flex-1 flex flex-col min-w-0">
    <TopBar />
    <main class="flex-1 overflow-y-auto bg-background">
      {@render children()}
    </main>
  </div>
</div>
