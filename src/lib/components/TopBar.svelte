<script lang="ts">
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { onMount } from "svelte";
  import { Minus, Square, X } from "@lucide/svelte";
  import { ui } from "$lib/stores/app.svelte";

  const win = getCurrentWindow();

  // Track the maximized state so the button shows a restore glyph when maximized
  // (like Explorer/VS Code). onResized fires on maximize/restore/snap.
  let maximized = $state(false);
  onMount(() => {
    win.isMaximized().then((m) => (maximized = m));
    const un = win.onResized(() => win.isMaximized().then((m) => (maximized = m)));
    return () => void un.then((fn) => fn());
  });
</script>

<header
  data-tauri-drag-region
  class="h-12 shrink-0 bg-surface border-b border-outline-variant flex items-center justify-between pl-2 pr-2 select-none"
>
  <nav class="flex items-center gap-1 text-label-md text-on-surface-variant pl-2">
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
      aria-label={maximized ? "Restore" : "Maximize"}
      title={maximized ? "Restore" : "Maximize"}
    >
      {#if maximized}
        <!-- Restore glyph: two overlapping squares (front occludes back). -->
        <svg
          width="13"
          height="13"
          viewBox="0 0 13 13"
          fill="none"
          stroke="currentColor"
          stroke-width="1.2"
          stroke-linejoin="round"
        >
          <path d="M3.7 3.7 V2.2 A0.5 0.5 0 0 1 4.2 1.7 H10.8 A0.5 0.5 0 0 1 11.3 2.2 V8.8 A0.5 0.5 0 0 1 10.8 9.3 H9.3" />
          <rect x="1.7" y="3.7" width="7.6" height="7.6" rx="0.5" />
        </svg>
      {:else}
        <Square size={13} />
      {/if}
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
