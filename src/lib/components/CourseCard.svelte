<script lang="ts">
  import { Play, Video, CheckCheck, TriangleAlert, LoaderCircle } from "@lucide/svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import type { CourseSummary } from "$lib/types";
  import { formatDuration, pct } from "$lib/format";
  import ProgressBar from "./ProgressBar.svelte";

  let { course }: { course: CourseSummary } = $props();

  const percent = $derived(pct(course.completed_count, course.lecture_count));
  const started = $derived(course.completed_count > 0 || course.last_opened_at != null);
  const completed = $derived(percent >= 100 && course.lecture_count > 0);
  const thumb = $derived(course.thumbnail_path ? convertFileSrc(course.thumbnail_path) : null);
  const busy = $derived(course.scan_status === "Importing" || course.scan_status === "Scanning");
  const broken = $derived(course.scan_status === "Error" || course.scan_status === "Missing");
</script>

<a
  href={`/course/${course.id}`}
  class="group bg-surface-container-low border border-outline-variant rounded-lg overflow-hidden hover:border-outline hover:bg-surface-container-high transition-all flex flex-col"
>
  <!-- Thumbnail -->
  <div class="aspect-video relative bg-surface-container-highest overflow-hidden">
    {#if thumb}
      <img
        src={thumb}
        alt={course.title}
        loading="lazy"
        decoding="async"
        class="w-full h-full object-cover opacity-90 group-hover:scale-105 transition-transform duration-500 {completed
          ? 'grayscale group-hover:grayscale-0'
          : ''}"
      />
    {:else}
      <div
        class="w-full h-full flex items-center justify-center bg-gradient-to-br from-surface-container-high to-surface-container-lowest"
      >
        <Video size={40} class="text-outline-variant" />
      </div>
    {/if}

    <!-- Status badges -->
    {#if completed}
      <div
        class="absolute top-2 left-2 bg-secondary-container/20 text-secondary-container px-1.5 py-0.5 rounded text-label-sm border border-secondary-container/30 backdrop-blur-sm flex items-center gap-1"
      >
        <CheckCheck size={12} /> Completed
      </div>
    {:else if broken}
      <div
        class="absolute top-2 left-2 bg-error/20 text-error px-1.5 py-0.5 rounded text-label-sm border border-error/30 backdrop-blur-sm flex items-center gap-1"
      >
        <TriangleAlert size={12} /> {course.scan_status}
      </div>
    {:else if busy}
      <div
        class="absolute top-2 left-2 bg-black/60 text-on-surface px-1.5 py-0.5 rounded text-label-sm backdrop-blur-sm flex items-center gap-1"
      >
        <LoaderCircle size={12} class="animate-spin" /> {course.scan_status}
      </div>
    {/if}

    {#if course.total_duration}
      <div
        class="absolute bottom-2 right-2 bg-black/80 px-1.5 py-0.5 rounded text-label-sm text-on-surface backdrop-blur-sm"
      >
        {formatDuration(course.total_duration)}
      </div>
    {/if}

    <!-- Progress fill anchored to image bottom -->
    {#if started && !completed}
      <div class="absolute bottom-0 left-0 right-0">
        <ProgressBar value={percent} />
      </div>
    {:else if completed}
      <div class="absolute bottom-0 left-0 right-0 h-1 bg-secondary-container"></div>
    {/if}

    <!-- Hover play overlay -->
    <div
      class="absolute inset-0 flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity bg-black/30"
    >
      <Play size={32} class="text-white" fill="currentColor" />
    </div>
  </div>

  <!-- Body -->
  <div class="p-3 flex flex-col flex-1">
    <h4 class="text-headline-sm text-on-surface mb-1 leading-tight line-clamp-2">{course.title}</h4>
    <div class="mt-auto pt-2 flex items-center justify-between text-on-surface-variant">
      <span class="text-label-sm flex items-center gap-1">
        <Video size={14} />
        {course.lecture_count} Lectures
      </span>
      {#if completed}
        <span class="text-label-sm">100%</span>
      {:else if started}
        <span class="text-label-sm">{percent}%</span>
      {:else}
        <span class="text-label-sm text-outline">Not started</span>
      {/if}
    </div>
  </div>
</a>
