<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { History, Play, CircleCheck, LoaderCircle } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { formatDayGroup, formatTimeOfDay, pct } from "$lib/format";
  import type { HistoryEntry } from "$lib/types";

  let items = $state<HistoryEntry[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      items = await api.history();
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    setCrumbs([{ label: "History" }]);
    load();
  });

  // Group by local day, preserving the backend's newest-first order.
  const groups = $derived.by(() => {
    const map = new Map<string, { label: string; items: HistoryEntry[] }>();
    for (const h of items) {
      const label = formatDayGroup(h.last_watched_at);
      let g = map.get(label);
      if (!g) {
        g = { label, items: [] };
        map.set(label, g);
      }
      g.items.push(h);
    }
    return [...map.values()];
  });

  function progress(h: HistoryEntry): number | null {
    if (h.completed) return 100;
    return h.duration ? Math.min(100, pct(h.position_seconds, h.duration)) : null;
  }

  function resume(h: HistoryEntry) {
    // Resume where we left off; rewatch a completed lecture from the start.
    const t = h.completed ? 0 : Math.floor(h.position_seconds);
    goto(t > 0 ? `/watch/${h.lecture_id}?t=${t}` : `/watch/${h.lecture_id}`);
  }
</script>

<div class="p-6 max-w-5xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <History size={18} /> History
  </h2>

  {#if loading}
    <div class="flex items-center gap-2 text-body-sm text-on-surface-variant py-16 justify-center">
      <LoaderCircle size={18} class="animate-spin" /> Loading…
    </div>
  {:else if error}
    <p class="text-body-sm text-error py-16 text-center">{error}</p>
  {:else if items.length === 0}
    <p class="text-body-sm text-on-surface-variant py-16 text-center">
      Nothing watched yet. Play a lecture and it'll show up here so you can pick up where you left off.
    </p>
  {:else}
    <div class="space-y-6">
      {#each groups as group (group.label)}
        <section class="space-y-2">
          <h3 class="text-label-lg text-on-surface-variant">{group.label}</h3>
          <ul
            class="rounded-lg border border-outline-variant divide-y divide-outline-variant overflow-hidden"
          >
            {#each group.items as h (h.lecture_id)}
              {@const p = progress(h)}
              <li class="bg-surface-container-low hover:bg-surface-container transition-colors">
                <button
                  onclick={() => resume(h)}
                  class="flex items-center gap-3 w-full text-left px-4 py-3"
                >
                  <span class="text-label-sm text-on-surface-variant tabular-nums shrink-0 w-14">
                    {formatTimeOfDay(h.last_watched_at)}
                  </span>
                  <span class="flex-1 min-w-0">
                    <span class="block truncate text-body-md text-on-surface">{h.lecture_title}</span>
                    <span class="block truncate text-label-sm text-on-surface-variant">
                      {h.course_title} · {h.section_title}
                    </span>
                  </span>
                  <span class="shrink-0 w-28 flex items-center justify-end gap-2">
                    {#if h.completed}
                      <span class="flex items-center gap-1 text-label-md text-secondary-container">
                        <CircleCheck size={14} /> Done
                      </span>
                    {:else if p != null}
                      <span class="h-1.5 flex-1 rounded-full bg-surface-container-highest overflow-hidden">
                        <span class="block h-full bg-primary rounded-full" style="width: {p}%"></span>
                      </span>
                      <span class="text-label-sm text-on-surface-variant tabular-nums w-9 text-right">
                        {p}%
                      </span>
                    {:else}
                      <span class="flex items-center gap-1 text-label-md text-accent-blue">
                        <Play size={13} fill="currentColor" /> Resume
                      </span>
                    {/if}
                  </span>
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</div>
