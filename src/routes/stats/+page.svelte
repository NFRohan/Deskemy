<script lang="ts">
  import { onMount } from "svelte";
  import { ChartColumn, Flame, Clock, GraduationCap, CircleCheck, Bookmark, LoaderCircle } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { formatDuration, pct } from "$lib/format";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import type { LibraryStats } from "$lib/types";

  let stats = $state<LibraryStats | null>(null);
  let loading = $state(true);

  onMount(async () => {
    setCrumbs([{ label: "Stats" }]);
    stats = await api.getStats().catch(() => null);
    loading = false;
  });

  const lecturePct = $derived(stats ? pct(stats.lectures_completed, stats.lectures_total) : 0);
</script>

<div class="p-6 max-w-4xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <ChartColumn size={18} /> Stats
  </h2>

  {#if loading}
    <div class="flex justify-center py-24 text-on-surface-variant">
      <LoaderCircle size={26} class="animate-spin" />
    </div>
  {:else if stats}
    <!-- Headline cards -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
        <div class="flex items-center gap-2 text-on-surface-variant mb-2">
          <Clock size={16} /><span class="text-label-md">Watched</span>
        </div>
        <p class="text-display-sm text-on-surface">{formatDuration(stats.watched_seconds)}</p>
      </div>
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
        <div class="flex items-center gap-2 text-on-surface-variant mb-2">
          <Flame size={16} /><span class="text-label-md">Current streak</span>
        </div>
        <p class="text-display-sm text-on-surface">
          {stats.current_streak}<span class="text-headline-sm text-on-surface-variant">
            {stats.current_streak === 1 ? " day" : " days"}</span
          >
        </p>
      </div>
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
        <div class="flex items-center gap-2 text-on-surface-variant mb-2">
          <CircleCheck size={16} /><span class="text-label-md">Lectures done</span>
        </div>
        <p class="text-display-sm text-on-surface">
          {stats.lectures_completed}<span class="text-headline-sm text-on-surface-variant"
            >/{stats.lectures_total}</span
          >
        </p>
      </div>
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
        <div class="flex items-center gap-2 text-on-surface-variant mb-2">
          <GraduationCap size={16} /><span class="text-label-md">Courses done</span>
        </div>
        <p class="text-display-sm text-on-surface">
          {stats.courses_completed}<span class="text-headline-sm text-on-surface-variant"
            >/{stats.courses_total}</span
          >
        </p>
      </div>
    </div>

    <!-- Overall library progress -->
    <div class="bg-surface-container-low border border-outline-variant rounded-xl p-5 space-y-3">
      <div class="flex items-center justify-between">
        <span class="text-body-md text-on-surface">Library completion</span>
        <span class="text-label-md text-on-surface-variant">{lecturePct}%</span>
      </div>
      <ProgressBar value={lecturePct} complete={lecturePct >= 100} />
      <p class="text-label-sm text-on-surface-variant">
        {stats.lectures_completed} of {stats.lectures_total} lectures{#if stats.library_seconds > 0}
          · {formatDuration(stats.watched_seconds)} of {formatDuration(stats.library_seconds)}{/if}
      </p>
    </div>

    <!-- Secondary stats -->
    <div class="grid grid-cols-2 md:grid-cols-4 gap-4">
      {#each [{ label: "In progress", value: stats.courses_in_progress }, { label: "Days active", value: stats.days_active }, { label: "Bookmarks", value: stats.bookmarks_total }, { label: "Courses", value: stats.courses_total }] as s (s.label)}
        <div class="bg-surface-container-low border border-outline-variant rounded-lg p-4">
          <p class="text-headline-md text-on-surface">{s.value}</p>
          <p class="text-label-sm text-on-surface-variant">{s.label}</p>
        </div>
      {/each}
    </div>
  {/if}
</div>
