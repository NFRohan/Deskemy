<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Bookmark, Trash2, Play, LoaderCircle } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { formatClock } from "$lib/format";
  import type { BookmarkDetail } from "$lib/types";

  let bookmarks = $state<BookmarkDetail[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      bookmarks = await api.listAllBookmarks();
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    setCrumbs([{ label: "Bookmarks" }]);
    load();
  });

  // Group by course, preserving the backend's ordering.
  const groups = $derived.by(() => {
    const map = new Map<string, { id: string; title: string; items: BookmarkDetail[] }>();
    for (const b of bookmarks) {
      let g = map.get(b.course_id);
      if (!g) {
        g = { id: b.course_id, title: b.course_title, items: [] };
        map.set(b.course_id, g);
      }
      g.items.push(b);
    }
    return [...map.values()];
  });

  function jump(b: BookmarkDetail) {
    // Deep-link into the player and seek to the bookmark's time.
    goto(`/watch/${b.lecture_id}?t=${Math.floor(b.position_seconds)}`);
  }

  async function remove(b: BookmarkDetail) {
    await api.deleteBookmark(b.id).catch(() => {});
    bookmarks = bookmarks.filter((x) => x.id !== b.id);
  }
</script>

<div class="p-6 max-w-5xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <Bookmark size={18} /> Bookmarks
  </h2>

  {#if loading}
    <div class="flex items-center gap-2 text-body-sm text-on-surface-variant py-16 justify-center">
      <LoaderCircle size={18} class="animate-spin" /> Loading…
    </div>
  {:else if error}
    <p class="text-body-sm text-error py-16 text-center">{error}</p>
  {:else if bookmarks.length === 0}
    <p class="text-body-sm text-on-surface-variant py-16 text-center">
      No bookmarks yet. While watching a lecture, open the bookmark panel in the player to add one.
    </p>
  {:else}
    <div class="space-y-6">
      {#each groups as group (group.id)}
        <section class="space-y-2">
          <a
            href={`/course/${group.id}`}
            class="inline-block text-headline-sm text-on-surface hover:text-primary transition-colors"
          >
            {group.title}
          </a>
          <ul class="rounded-lg border border-outline-variant divide-y divide-outline-variant overflow-hidden">
            {#each group.items as b (b.id)}
              <li class="flex items-center gap-3 bg-surface-container-low hover:bg-surface-container transition-colors">
                <button
                  onclick={() => jump(b)}
                  class="flex items-center gap-3 flex-1 min-w-0 text-left px-4 py-3"
                >
                  <span
                    class="flex items-center gap-1.5 text-label-md text-accent-blue tabular-nums shrink-0 w-20"
                  >
                    <Play size={13} fill="currentColor" />
                    {formatClock(b.position_seconds)}
                  </span>
                  <span class="flex-1 min-w-0">
                    <span class="block truncate text-body-md text-on-surface">
                      {b.label ?? b.lecture_title}
                    </span>
                    <span class="block truncate text-label-sm text-on-surface-variant">
                      {b.section_title} · {b.lecture_title}
                    </span>
                  </span>
                </button>
                <button
                  onclick={() => remove(b)}
                  class="p-2 mr-2 rounded text-on-surface-variant hover:text-error hover:bg-surface-container-highest transition-colors shrink-0"
                  aria-label="Delete bookmark"
                  title="Delete bookmark"
                >
                  <Trash2 size={16} />
                </button>
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</div>
