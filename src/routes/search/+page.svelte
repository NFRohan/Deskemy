<script lang="ts">
  import { Search } from "@lucide/svelte";
  import { setCrumbs } from "$lib/stores/app.svelte";

  $effect(() => {
    setCrumbs([{ label: "Search" }]);
  });
</script>

<div class="flex flex-col items-center justify-center h-full text-center gap-4 p-8">
  <div class="w-16 h-16 rounded-xl bg-surface-container flex items-center justify-center">
    <Search size={30} class="text-outline" />
  </div>
  <h2 class="text-headline-md text-on-surface">Search arrives in M7</h2>
  <p class="text-body-sm text-on-surface-variant max-w-sm">
    Full-text search across courses, sections, and lectures (FTS5) is already indexed in the
    backend — the UI lands in a later milestone.
  </p>
</div>
