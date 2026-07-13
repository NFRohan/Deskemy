<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { Minus, Square, X } from "@lucide/svelte";
  import { ui } from "$lib/stores/app.svelte";

  const win = getCurrentWindow();
</script>

<header
  data-tauri-drag-region
  class="h-12 shrink-0 bg-surface border-b border-outline-variant flex items-center justify-between pl-4 pr-2 select-none"
>
  <nav class="flex items-center gap-2 text-label-md text-on-surface-variant">
    {#each ui.crumbs as crumb, i (i)}
      {#if i > 0}
        <span class="text-outline-variant">/</span>
      {/if}
      {#if crumb.href}
        <a href={crumb.href} class="hover:text-on-surface transition-colors">{crumb.label}</a>
      {:else}
        <span class="text-on-surface">{crumb.label}</span>
      {/if}
    {/each}
  </nav>

  <div class="flex items-center gap-1 text-on-surface-variant">
    <button
      onclick={() => win.minimize()}
      class="p-1.5 rounded hover:bg-surface-container-highest hover:text-on-surface transition-colors"
      aria-label="Minimize"
    >
      <Minus size={16} />
    </button>
    <button
      onclick={() => win.toggleMaximize()}
      class="p-1.5 rounded hover:bg-surface-container-highest hover:text-on-surface transition-colors"
      aria-label="Maximize"
    >
      <Square size={13} />
    </button>
    <button
      onclick={() => win.close()}
      class="p-1.5 rounded hover:bg-error/80 hover:text-on-surface transition-colors"
      aria-label="Close"
    >
      <X size={16} />
    </button>
  </div>
</header>
