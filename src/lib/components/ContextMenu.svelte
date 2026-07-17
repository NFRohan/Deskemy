<script lang="ts">
  import { ctxMenu, closeContextMenu } from "$lib/stores/contextmenu.svelte";
</script>

{#if ctxMenu.open}
  <!-- Full-screen catcher: any click (or a second right-click) dismisses. -->
  <div
    class="fixed inset-0 z-[60]"
    role="presentation"
    onpointerdown={closeContextMenu}
    oncontextmenu={(e) => {
      e.preventDefault();
      closeContextMenu();
    }}
  >
    <div
      class="absolute min-w-56 bg-surface-container border border-outline-variant rounded-lg shadow-xl py-1"
      style="left: {ctxMenu.x}px; top: {ctxMenu.y}px;"
      role="menu"
      tabindex="-1"
    >
      {#each ctxMenu.items as item (item.label)}
        <button
          role="menuitem"
          onclick={() => {
            closeContextMenu();
            item.action();
          }}
          class="w-full text-left px-3 py-2 text-body-sm text-on-surface hover:bg-surface-container-highest transition-colors"
        >
          {item.label}
        </button>
      {/each}
    </div>
  </div>
{/if}

<svelte:window
  onkeydown={(e) => e.key === "Escape" && closeContextMenu()}
  onresize={closeContextMenu}
/>
