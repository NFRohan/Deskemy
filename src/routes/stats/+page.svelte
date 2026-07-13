<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import {
    ChartColumn,
    Flame,
    Clock,
    GraduationCap,
    CircleCheck,
    Bookmark,
    Play,
    Trophy,
    TrendingUp,
    LoaderCircle,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { formatDuration, pct } from "$lib/format";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import type { LibraryStats, DayActivity } from "$lib/types";

  let stats = $state<LibraryStats | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  onMount(async () => {
    setCrumbs([{ label: "Stats" }]);
    try {
      stats = await api.getStats();
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  });

  function key(d: Date): string {
    return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(
      d.getDate(),
    ).padStart(2, "0")}`;
  }
  function level(secs: number): number {
    const m = secs / 60;
    if (m <= 0) return 0;
    if (m < 15) return 1;
    if (m < 30) return 2;
    if (m < 60) return 3;
    return 4;
  }
  const LEVELS = [
    "bg-surface-container-high",
    "bg-primary-container/30",
    "bg-primary-container/55",
    "bg-primary-container/80",
    "bg-primary-container",
  ];

  const byDay = $derived(new Map((stats?.activity ?? []).map((a: DayActivity) => [a.day, a])));
  const lecturePct = $derived(stats ? pct(stats.lectures_completed, stats.lectures_total) : 0);
  const goalMin = $derived(stats?.daily_goal_minutes ?? 30);
  const todayMin = $derived(stats ? Math.round(stats.watch_seconds_today / 60) : 0);
  const goalPct = $derived(goalMin > 0 ? Math.min(100, Math.round((todayMin / goalMin) * 100)) : 0);
  const totalWatch = $derived.by(() => {
    if (!stats) return 0;
    if (stats.watched_seconds > 0) return stats.watched_seconds;
    return stats.activity.reduce((a, d) => a + d.watch_seconds, 0);
  });

  // Heatmap: weeks of columns (Sun..Sat), oldest → newest, stretched to width.
  const WEEKS = 26;
  const heatmap = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const lastSat = new Date(today);
    lastSat.setDate(today.getDate() + (6 - today.getDay()));
    const cols: { day: string; secs: number; future: boolean }[][] = [];
    for (let w = 0; w < WEEKS; w++) {
      const col = [];
      for (let d = 0; d < 7; d++) {
        const date = new Date(lastSat);
        date.setDate(lastSat.getDate() - (WEEKS - 1 - w) * 7 - (6 - d));
        col.push({
          day: key(date),
          secs: byDay.get(key(date))?.watch_seconds ?? 0,
          future: date > today,
        });
      }
      cols.push(col);
    }
    return cols;
  });

  // This week's watch time, per day (last 7 days, oldest → today).
  const week = $derived.by(() => {
    const today = new Date();
    today.setHours(0, 0, 0, 0);
    const days = [];
    for (let i = 6; i >= 0; i--) {
      const date = new Date(today);
      date.setDate(today.getDate() - i);
      days.push({
        label: ["S", "M", "T", "W", "T", "F", "S"][date.getDay()],
        secs: byDay.get(key(date))?.watch_seconds ?? 0,
        today: i === 0,
      });
    }
    return days;
  });
  const weekMax = $derived(Math.max(1, ...week.map((d) => d.secs)));

  const ring = 2 * Math.PI * 34; // circumference for the goal ring
</script>

<div class="p-6 max-w-4xl mx-auto space-y-8">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <ChartColumn size={18} /> Stats
  </h2>

  {#if loading}
    <div class="flex justify-center py-24 text-on-surface-variant">
      <LoaderCircle size={26} class="animate-spin" />
    </div>
  {:else if error}
    <p class="text-body-sm text-error py-16 text-center">Couldn't load stats: {error}</p>
  {:else if stats}
    <!-- Today's Progress -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Today</h3>
      <div class="grid grid-cols-1 sm:grid-cols-3 gap-4">
        <!-- Goal ring -->
        <div
          class="bg-surface-container-low border border-outline-variant rounded-xl p-4 flex items-center gap-4"
        >
          <div class="relative w-20 h-20 shrink-0">
            <svg viewBox="0 0 80 80" class="w-20 h-20 -rotate-90">
              <circle cx="40" cy="40" r="34" fill="none" stroke="var(--color-surface-container-highest)" stroke-width="8" />
              <circle
                cx="40"
                cy="40"
                r="34"
                fill="none"
                stroke="var(--color-primary-container)"
                stroke-width="8"
                stroke-linecap="round"
                stroke-dasharray={ring}
                stroke-dashoffset={ring - (goalPct / 100) * ring}
              />
            </svg>
            <span
              class="absolute inset-0 flex items-center justify-center text-label-md text-on-surface"
              >{goalPct}%</span
            >
          </div>
          <div class="min-w-0">
            <p class="text-label-sm text-on-surface-variant">Daily goal</p>
            <p class="text-headline-md text-on-surface">{todayMin} / {goalMin} min</p>
          </div>
        </div>
        <!-- Streak -->
        <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
          <div class="flex items-center gap-2 text-on-surface-variant mb-2">
            <Flame size={16} /><span class="text-label-md">Current streak</span>
          </div>
          <p class="text-display-sm text-on-surface">
            {stats.current_streak}<span class="text-headline-sm text-on-surface-variant">
              {stats.current_streak === 1 ? "day" : "days"}</span
            >
          </p>
          <p class="text-label-sm text-on-surface-variant mt-1">Best: {stats.best_streak} days</p>
        </div>
        <!-- Continue -->
        <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4 flex flex-col">
          <p class="text-label-sm text-on-surface-variant mb-1">Currently focused</p>
          {#if stats.focus_course_id}
            <button
              onclick={() => goto(`/course/${stats!.focus_course_id}`)}
              class="text-left flex-1 flex flex-col justify-center"
            >
              <span class="text-body-md text-on-surface line-clamp-2">{stats.focus_course_title}</span>
              <span class="mt-2 inline-flex items-center gap-1.5 text-label-md text-primary">
                <Play size={14} fill="currentColor" /> Continue · {stats.focus_course_pct}%
              </span>
            </button>
          {:else}
            <span class="text-body-sm text-on-surface-variant flex-1 flex items-center">
              Start a course to see it here.
            </span>
          {/if}
        </div>
      </div>
    </section>

    <!-- Learning Overview -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Learning overview</h3>
      <div class="grid grid-cols-2 md:grid-cols-3 gap-4">
        <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
          <div class="flex items-center gap-2 text-on-surface-variant mb-2">
            <Clock size={16} /><span class="text-label-md">Watch time</span>
          </div>
          <p class="text-display-sm text-on-surface">{formatDuration(totalWatch)}</p>
          <p class="text-label-sm text-on-surface-variant mt-1">{todayMin}m today</p>
        </div>
        <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
          <div class="flex items-center gap-2 text-on-surface-variant mb-2">
            <CircleCheck size={16} /><span class="text-label-md">Lectures completed</span>
          </div>
          <p class="text-display-sm text-on-surface">
            {stats.lectures_completed}<span class="text-headline-sm text-on-surface-variant"
              >/{stats.lectures_total}</span
            >
          </p>
          <p class="text-label-sm text-on-surface-variant mt-1">{lecturePct}%</p>
        </div>
        <div class="bg-surface-container-low border border-outline-variant rounded-xl p-4">
          <div class="flex items-center gap-2 text-on-surface-variant mb-2">
            <GraduationCap size={16} /><span class="text-label-md">Completed courses</span>
          </div>
          <p class="text-display-sm text-on-surface">
            {stats.courses_completed}<span class="text-headline-sm text-on-surface-variant"
              >/{stats.courses_total}</span
            >
          </p>
        </div>
      </div>
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-5 space-y-2">
        <div class="flex items-center justify-between">
          <span class="text-body-md text-on-surface">Overall learning progress</span>
          <span class="text-label-md text-on-surface-variant">{lecturePct}%</span>
        </div>
        <ProgressBar value={lecturePct} complete={lecturePct >= 100} />
        <p class="text-label-sm text-on-surface-variant">
          {stats.lectures_completed} / {stats.lectures_total} lectures completed
        </p>
      </div>
    </section>

    <!-- Activity -->
    <section class="space-y-3">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Activity</h3>
      <div class="bg-surface-container-low border border-outline-variant rounded-xl p-5 space-y-5">
        <!-- Heatmap -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-body-md text-on-surface">Watch activity</span>
            <span class="text-label-sm text-on-surface-variant">
              {stats.active_days_month} active days this month
            </span>
          </div>
          <div class="flex gap-1">
            {#each heatmap as col (col[0].day)}
              <div class="flex-1 flex flex-col gap-1">
                {#each col as cell (cell.day)}
                  <div
                    class="w-full aspect-square rounded-[3px] {cell.future
                      ? 'opacity-0'
                      : LEVELS[level(cell.secs)]}"
                    title={cell.future ? "" : `${cell.day}: ${formatDuration(cell.secs)}`}
                  ></div>
                {/each}
              </div>
            {/each}
          </div>
          <div class="flex items-center gap-1 mt-2 text-label-sm text-on-surface-variant">
            <span>Less</span>
            {#each LEVELS as c (c)}<div class="w-3 h-3 rounded-sm {c}"></div>{/each}
            <span>More</span>
          </div>
        </div>

        <!-- This week -->
        <div>
          <div class="flex items-center justify-between mb-2">
            <span class="text-body-md text-on-surface">This week</span>
            <span class="text-label-sm text-on-surface-variant">
              {formatDuration(stats.watch_seconds_week)}
            </span>
          </div>
          <div class="flex items-end justify-between gap-2 h-24">
            {#each week as d, i (i)}
              <div class="flex-1 flex flex-col items-center gap-1 h-full justify-end">
                <div
                  class="w-full rounded-t transition-all {d.today
                    ? 'bg-primary-container'
                    : 'bg-surface-container-highest'}"
                  style="height: {Math.max(4, (d.secs / weekMax) * 100)}%"
                  title={formatDuration(d.secs)}
                ></div>
                <span class="text-label-sm text-on-surface-variant">{d.label}</span>
              </div>
            {/each}
          </div>
        </div>
      </div>
    </section>

    <!-- Library + Records -->
    <div class="grid grid-cols-1 md:grid-cols-2 gap-8">
      <section class="space-y-3">
        <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Library</h3>
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-surface-container-low border border-outline-variant rounded-lg p-4">
            <p class="text-headline-md text-on-surface">{stats.courses_in_progress}</p>
            <p class="text-label-sm text-on-surface-variant">In progress</p>
          </div>
          <div class="bg-surface-container-low border border-outline-variant rounded-lg p-4">
            <div class="flex items-center gap-1.5 text-headline-md text-on-surface">
              <Bookmark size={16} class="text-on-surface-variant" />{stats.bookmarks_total}
            </div>
            <p class="text-label-sm text-on-surface-variant">Saved moments</p>
          </div>
        </div>
      </section>

      <section class="space-y-3">
        <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">Personal records</h3>
        <div class="grid grid-cols-2 gap-4">
          <div class="bg-surface-container-low border border-outline-variant rounded-lg p-4">
            <div class="flex items-center gap-1.5 text-headline-md text-on-surface">
              <Trophy size={16} class="text-on-surface-variant" />{stats.best_streak}
            </div>
            <p class="text-label-sm text-on-surface-variant">Best streak (days)</p>
          </div>
          <div class="bg-surface-container-low border border-outline-variant rounded-lg p-4">
            <div class="flex items-center gap-1.5 text-headline-md text-on-surface">
              <TrendingUp size={16} class="text-on-surface-variant" />+{stats.lectures_last_7}
            </div>
            <p class="text-label-sm text-on-surface-variant">Lectures (last 7 days)</p>
          </div>
        </div>
      </section>
    </div>
  {/if}
</div>
