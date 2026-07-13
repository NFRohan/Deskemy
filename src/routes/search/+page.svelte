<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Search, BookOpen, ListVideo, Play, Paperclip, LoaderCircle } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import type { SearchHit } from "$lib/types";

  let query = $state("");
  let results = $state<SearchHit[]>([]);
  let loading = $state(false);
  let searched = $state(false);
  let inputEl = $state<HTMLInputElement | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let seq = 0; // guards against out-of-order responses

  onMount(() => {
    setCrumbs([{ label: "Search" }]);
    inputEl?.focus();
  });

  function onInput() {
    clearTimeout(timer);
    if (!query.trim()) {
      results = [];
      searched = false;
      loading = false;
      return;
    }
    loading = true;
    timer = setTimeout(runSearch, 180);
  }

  async function runSearch() {
    const q = query.trim();
    if (!q) return;
    const mine = ++seq;
    try {
      const hits = await api.search(q);
      if (mine !== seq) return; // a newer query superseded this one
      results = hits;
    } catch {
      if (mine === seq) results = [];
    } finally {
      if (mine === seq) {
        loading = false;
        searched = true;
      }
    }
  }

  const meta: Record<string, { icon: any; label: string }> = {
    course: { icon: BookOpen, label: "Course" },
    section: { icon: ListVideo, label: "Section" },
    lecture: { icon: Play, label: "Lecture" },
    attachment: { icon: Paperclip, label: "Attachment" },
  };

  function openHit(hit: SearchHit) {
    if (hit.kind === "lecture") goto(`/watch/${hit.entity_id}`);
    else goto(`/course/${hit.course_id}`);
  }
</script>

<div class="p-6 max-w-3xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <Search size={18} /> Search
  </h2>

  <div class="relative">
    <Search
      size={18}
      class="absolute left-3 top-1/2 -translate-y-1/2 text-on-surface-variant pointer-events-none"
    />
    <input
      bind:this={inputEl}
      bind:value={query}
      oninput={onInput}
      onkeydown={(e) => e.key === "Enter" && (clearTimeout(timer), runSearch())}
      placeholder="Search courses, sections, lectures…"
      class="w-full bg-surface-container-low border border-outline-variant rounded-lg text-body-md text-on-surface pl-10 pr-10 py-2.5 outline-none focus:border-accent-blue focus:ring-1 focus:ring-accent-blue transition-all placeholder:text-on-surface-variant"
    />
    {#if loading}
      <LoaderCircle
        size={18}
        class="absolute right-3 top-1/2 -translate-y-1/2 animate-spin text-on-surface-variant"
      />
    {/if}
  </div>

  {#if results.length > 0}
    <ul
      class="rounded-lg border border-outline-variant divide-y divide-outline-variant overflow-hidden"
    >
      {#each results as hit (hit.kind + hit.entity_id)}
        {@const m = meta[hit.kind] ?? meta.lecture}
        <li>
          <button
            onclick={() => openHit(hit)}
            class="w-full flex items-center gap-3 px-4 py-3 text-left bg-surface-container-low hover:bg-surface-container transition-colors"
          >
            <m.icon size={18} class="text-on-surface-variant shrink-0" />
            <span class="flex-1 min-w-0">
              <span class="block truncate text-body-md text-on-surface">{hit.title}</span>
              <span class="block truncate text-label-sm text-on-surface-variant">
                {m.label}{#if hit.kind !== "course"} · {hit.course_title}{/if}
              </span>
            </span>
          </button>
        </li>
      {/each}
    </ul>
  {:else if searched && !loading}
    <p class="text-body-sm text-on-surface-variant py-16 text-center">
      No matches for "{query.trim()}".
    </p>
  {:else if !query.trim()}
    <p class="text-body-sm text-on-surface-variant py-16 text-center">
      Search across your library — course titles, sections, lectures, and attachments.
    </p>
  {/if}
</div>
