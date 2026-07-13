<script lang="ts">
  import { onMount } from "svelte";
  import { Settings } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import type { AppConfig } from "$lib/types";

  let config = $state<AppConfig | null>(null);

  onMount(async () => {
    setCrumbs([{ label: "Settings" }]);
    config = await api.getConfig().catch(() => null);
  });
</script>

<div class="p-6 max-w-2xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <Settings size={18} /> Settings
  </h2>

  {#if config}
    <div class="bg-surface-container-low border border-outline-variant rounded-lg divide-y divide-outline-variant">
      <div class="flex items-center justify-between p-4">
        <span class="text-body-md text-on-surface">Theme</span>
        <span class="text-body-sm text-on-surface-variant">{config.theme}</span>
      </div>
      <div class="flex items-center justify-between p-4">
        <span class="text-body-md text-on-surface">Default playback speed</span>
        <span class="text-body-sm text-on-surface-variant">{config.default_speed}×</span>
      </div>
      <div class="flex items-center justify-between p-4">
        <span class="text-body-md text-on-surface">Autoplay next lecture</span>
        <span class="text-body-sm text-on-surface-variant">{config.autoplay_next ? "On" : "Off"}</span>
      </div>
    </div>
    <p class="text-label-sm text-outline">Editable settings arrive in M8.</p>
  {/if}
</div>
