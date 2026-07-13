<script lang="ts">
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import {
    Play,
    Star,
    CircleCheck,
    Circle,
    ChevronDown,
    ChevronRight,
    TriangleAlert,
    ListVideo,
    LoaderCircle,
    Upload,
    Clipboard,
    ImagePlus,
    X,
    Pencil,
  } from "@lucide/svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { api, pickImage } from "$lib/api";
  import { setCrumbs, loadLibrary } from "$lib/stores/app.svelte";
  import { formatDuration, formatClock, pct } from "$lib/format";
  import type { CourseDetail, Lecture, Section } from "$lib/types";
  import ProgressBar from "$lib/components/ProgressBar.svelte";

  let course = $state<CourseDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let expanded = $state<Set<string>>(new Set());
  let thumbBusy = $state(false);
  let thumbError = $state<string | null>(null);
  let showThumbModal = $state(false);

  const thumbSrc = $derived(course?.thumbnail_path ? convertFileSrc(course.thumbnail_path) : "");

  function flat(c: CourseDetail): Lecture[] {
    return c.sections.flatMap((s) => s.lectures);
  }
  function sectionDuration(s: Section): number | null {
    const known = s.lectures.filter((l) => l.duration != null);
    if (known.length === 0) return null;
    return known.reduce((a, l) => a + (l.duration ?? 0), 0);
  }
  function nextLecture(c: CourseDetail): Lecture | undefined {
    if (c.last_lecture_id) {
      const l = flat(c).find((x) => x.id === c.last_lecture_id);
      if (l) return l;
    }
    return flat(c).find((l) => !l.completed && l.playable) ?? flat(c)[0];
  }

  const total = $derived(course ? flat(course).length : 0);
  const done = $derived(course ? flat(course).filter((l) => l.completed).length : 0);
  const overall = $derived(pct(done, total));
  const resume = $derived(course ? nextLecture(course) : undefined);

  async function load(courseId: string) {
    loading = true;
    error = null;
    try {
      const c = await api.getCourse(courseId);
      course = c;
      if (c) {
        setCrumbs([{ label: "Library", href: "/" }, { label: c.title }]);
        api.touchOpened(courseId).catch(() => {});
        const nxt = nextLecture(c);
        const openId = nxt
          ? c.sections.find((s) => s.lectures.some((l) => l.id === nxt.id))?.id
          : c.sections[0]?.id;
        expanded = new Set(openId ? [openId] : []);
      }
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    load($page.params.id);
  });

  function toggle(sectionId: string) {
    const next = new Set(expanded);
    next.has(sectionId) ? next.delete(sectionId) : next.add(sectionId);
    expanded = next;
  }

  async function toggleFavorite() {
    if (!course) return;
    const val = !course.is_favorite;
    course.is_favorite = val;
    await api.setFavorite(course.id, val).catch(() => {});
    loadLibrary(true);
  }

  // --- Thumbnail: pick a file, paste an image, or remove ---

  function bytesToBase64(bytes: Uint8Array): string {
    let binary = "";
    const chunk = 0x8000; // avoid arg-count limits on fromCharCode
    for (let i = 0; i < bytes.length; i += chunk) {
      binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
    }
    return btoa(binary);
  }

  async function setFromBlob(blob: Blob) {
    if (!course) return;
    thumbBusy = true;
    thumbError = null;
    try {
      const bytes = new Uint8Array(await blob.arrayBuffer());
      const ext = blob.type.startsWith("image/") ? blob.type.slice(6) : null;
      const stored = await api.setCourseThumbnailBytes(course.id, bytesToBase64(bytes), ext);
      course.thumbnail_path = stored;
      loadLibrary(true);
    } catch (e: any) {
      thumbError = e?.message ?? String(e);
    } finally {
      thumbBusy = false;
    }
  }

  async function setFromFile() {
    if (!course) return;
    const path = await pickImage();
    if (!path) return;
    thumbBusy = true;
    thumbError = null;
    try {
      const stored = await api.setCourseThumbnailFile(course.id, path);
      course.thumbnail_path = stored;
      loadLibrary(true);
    } catch (e: any) {
      thumbError = e?.message ?? String(e);
    } finally {
      thumbBusy = false;
    }
  }

  function onPaste(e: ClipboardEvent) {
    const tag = (e.target as HTMLElement | null)?.tagName;
    if (tag === "INPUT" || tag === "TEXTAREA") return;
    const items = e.clipboardData?.items;
    if (!items) return;
    // DataTransferItemList is not reliably iterable; index it.
    for (let i = 0; i < items.length; i++) {
      const item = items[i];
      if (item.type.startsWith("image/")) {
        const blob = item.getAsFile();
        if (blob) {
          e.preventDefault();
          setFromBlob(blob);
          return;
        }
      }
    }
  }

  async function removeThumb() {
    if (!course) return;
    await api.clearCourseThumbnail(course.id).catch(() => {});
    course.thumbnail_path = null;
    loadLibrary(true);
  }

  function openLecture(l: Lecture) {
    if (!l.playable) return;
    goto(`/watch/${l.id}`);
  }

  async function toggleComplete(l: Lecture) {
    const val = !l.completed;
    l.completed = val; // optimistic (deeply reactive $state)
    await api.setLectureCompleted(l.id, val).catch(() => {});
    loadLibrary(true); // keep the library progress count in sync
  }
</script>

<svelte:window
  onpaste={onPaste}
  onkeydown={(e) => e.key === "Escape" && showThumbModal && (showThumbModal = false)}
/>

{#if loading}
  <div class="flex items-center justify-center py-32 text-on-surface-variant">
    <LoaderCircle size={28} class="animate-spin" />
  </div>
{:else if error}
  <div class="p-6 max-w-5xl mx-auto">
    <div class="bg-error/10 border border-error/30 text-error rounded-lg p-4 text-body-sm">{error}</div>
  </div>
{:else if course}
  <div class="p-6 max-w-5xl mx-auto space-y-6">
    <!-- Header -->
    <div class="flex gap-6 bg-surface-container-low border border-outline-variant rounded-xl p-4">
      <button
        onclick={() => (showThumbModal = true)}
        class="group/thumb w-64 shrink-0 aspect-video rounded-lg overflow-hidden border border-outline-variant bg-surface-container-highest relative"
        title="Edit thumbnail"
        aria-label="Edit thumbnail"
      >
        {#if course.thumbnail_path}
          <img src={thumbSrc} alt={course.title} class="w-full h-full object-cover" />
        {:else}
          <div
            class="w-full h-full flex flex-col items-center justify-center gap-1.5 bg-gradient-to-br from-surface-container-high to-surface-container-lowest text-outline-variant"
          >
            <ImagePlus size={32} />
            <span class="text-label-sm">No thumbnail</span>
          </div>
        {/if}
        <!-- Hover-reveal edit affordance -->
        <div
          class="absolute inset-0 bg-black/45 opacity-0 group-hover/thumb:opacity-100 transition-opacity flex items-center justify-center gap-1.5 text-on-surface text-label-md"
        >
          <Pencil size={16} /> Edit
        </div>
        {#if thumbBusy}
          <div class="absolute inset-0 bg-black/50 flex items-center justify-center">
            <LoaderCircle size={24} class="animate-spin text-on-surface" />
          </div>
        {/if}
      </button>
      <div class="flex-1 min-w-0 flex flex-col">
        <div class="flex items-start justify-between gap-4">
          <h1 class="text-display-sm text-on-surface">{course.title}</h1>
          <button
            onclick={toggleFavorite}
            class="p-2 rounded hover:bg-surface-container-highest transition-colors shrink-0"
            aria-label="Toggle favorite"
          >
            <Star
              size={20}
              class={course.is_favorite ? "text-primary" : "text-on-surface-variant"}
              fill={course.is_favorite ? "currentColor" : "none"}
            />
          </button>
        </div>

        <div class="mt-auto pt-4">
          <div class="flex items-center justify-between mb-2">
            <span class="text-label-md text-on-surface">
              {done} / {total} lectures completed
            </span>
            <span class="text-label-sm text-on-surface-variant">
              {#if course.total_duration}{formatDuration(course.total_duration)} total{/if}
            </span>
          </div>
          <ProgressBar value={overall} complete={overall >= 100} />

          {#if resume}
            <button
              onclick={() => openLecture(resume)}
              class="mt-4 inline-flex items-center gap-2 bg-primary-container text-on-primary-container px-4 py-2 rounded text-label-md hover:bg-inverse-primary transition-colors"
            >
              <Play size={16} fill="currentColor" />
              {done > 0 ? "Resume Lecture" : "Start Course"}
            </button>
          {/if}
        </div>
      </div>
    </div>

    <!-- Curriculum -->
    <div>
      <h2 class="flex items-center gap-2 text-headline-sm text-on-surface mb-4">
        <ListVideo size={18} /> Course Curriculum
      </h2>

      <div class="space-y-2">
        {#each course.sections as section (section.id)}
          {@const secDone = section.lectures.filter((l) => l.completed).length}
          {@const secDur = sectionDuration(section)}
          <div class="bg-surface-container-low border border-outline-variant rounded-lg overflow-hidden">
            <button
              onclick={() => toggle(section.id)}
              class="w-full flex items-center justify-between px-4 py-3 hover:bg-surface-container transition-colors text-left"
            >
              <div class="flex items-center gap-2 min-w-0">
                {#if expanded.has(section.id)}
                  <ChevronDown size={18} class="text-on-surface-variant shrink-0" />
                {:else}
                  <ChevronRight size={18} class="text-on-surface-variant shrink-0" />
                {/if}
                <span class="text-headline-sm text-on-surface truncate">{section.title}</span>
              </div>
              <span class="text-label-sm text-on-surface-variant shrink-0 pl-3">
                {secDone}/{section.lectures.length}{#if secDur} · {formatDuration(secDur)}{/if}
              </span>
            </button>

            {#if expanded.has(section.id)}
              <ul class="border-t border-outline-variant">
                {#each section.lectures as lecture (lecture.id)}
                  {@const isNext = resume?.id === lecture.id}
                  <li
                    class="group flex items-center gap-3 px-4 py-2 transition-colors
                      {isNext ? 'bg-primary-container/15' : 'hover:bg-surface-container'}"
                  >
                    <!-- completion toggle -->
                    <button
                      onclick={() => toggleComplete(lecture)}
                      class="shrink-0"
                      title={lecture.completed ? "Mark as not done" : "Mark as done"}
                      aria-label={lecture.completed ? "Mark as not done" : "Mark as done"}
                    >
                      {#if lecture.completed}
                        <CircleCheck size={18} class="text-secondary-container" />
                      {:else}
                        <Circle
                          size={18}
                          class="text-outline hover:text-secondary-container transition-colors"
                        />
                      {/if}
                    </button>

                    <!-- open lecture -->
                    <button
                      onclick={() => openLecture(lecture)}
                      disabled={!lecture.playable}
                      class="flex-1 min-w-0 flex items-center gap-2 text-left
                        {lecture.playable ? '' : 'opacity-60 cursor-not-allowed'}"
                    >
                      {#if lecture.playable}
                        <Play
                          size={14}
                          class="text-primary opacity-0 group-hover:opacity-100 transition-opacity shrink-0"
                          fill="currentColor"
                        />
                      {/if}
                      <span
                        class="flex-1 min-w-0 truncate text-body-md
                          {lecture.completed ? 'text-on-surface-variant line-through' : 'text-on-surface'}"
                      >
                        {lecture.title}
                      </span>

                      {#if isNext}
                        <span
                          class="text-label-sm text-primary bg-primary-container/20 px-1.5 py-0.5 rounded shrink-0"
                        >
                          Next up
                        </span>
                      {/if}

                      {#if !lecture.playable}
                        <span class="text-label-sm text-error flex items-center gap-1 shrink-0">
                          <TriangleAlert size={12} /> Corrupted
                        </span>
                      {/if}

                      <span class="text-label-sm text-on-surface-variant shrink-0 tabular-nums">
                        {lecture.duration ? formatClock(lecture.duration) : ""}
                      </span>
                    </button>
                  </li>
                {/each}
              </ul>
            {/if}
          </div>
        {/each}
      </div>
    </div>
  </div>

  <!-- Thumbnail editor modal -->
  {#if showThumbModal}
    <div
      class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
      role="presentation"
      onclick={() => (showThumbModal = false)}
    >
      <div
        class="w-full max-w-md bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4"
        role="dialog"
        aria-modal="true"
        aria-label="Course thumbnail"
        tabindex="-1"
        onclick={(e) => e.stopPropagation()}
      >
        <div class="flex items-center justify-between">
          <h3 class="text-headline-sm text-on-surface">Course thumbnail</h3>
          <button
            onclick={() => (showThumbModal = false)}
            class="p-1.5 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest transition-colors"
            aria-label="Close"
          >
            <X size={18} />
          </button>
        </div>

        <div
          class="aspect-video rounded-lg overflow-hidden border border-outline-variant bg-surface-container-highest relative"
        >
          {#if course.thumbnail_path}
            <img src={thumbSrc} alt={course.title} class="w-full h-full object-cover" />
          {:else}
            <div
              class="w-full h-full flex flex-col items-center justify-center gap-1.5 bg-gradient-to-br from-surface-container-high to-surface-container-lowest text-outline-variant"
            >
              <ImagePlus size={36} />
              <span class="text-label-sm">No thumbnail</span>
            </div>
          {/if}
          {#if thumbBusy}
            <div class="absolute inset-0 bg-black/50 flex items-center justify-center">
              <LoaderCircle size={24} class="animate-spin text-on-surface" />
            </div>
          {/if}
        </div>

        <div class="flex items-center gap-2">
          <button
            onclick={setFromFile}
            disabled={thumbBusy}
            class="flex-1 inline-flex items-center justify-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-3 py-2 rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
          >
            <Upload size={15} /> Upload image
          </button>
          {#if course.thumbnail_path}
            <button
              onclick={removeThumb}
              disabled={thumbBusy}
              class="inline-flex items-center gap-1.5 text-label-md text-on-surface-variant px-3 py-2 rounded hover:text-error hover:bg-surface-container-highest transition-colors disabled:opacity-60"
            >
              <X size={15} /> Remove
            </button>
          {/if}
        </div>

        <p class="text-label-sm text-on-surface-variant flex items-center gap-1.5">
          <Clipboard size={13} class="shrink-0" /> Tip: copy any image and press Ctrl+V to paste it here.
        </p>
        {#if thumbError}
          <p class="text-label-sm text-error">{thumbError}</p>
        {/if}
      </div>
    </div>
  {/if}
{/if}
