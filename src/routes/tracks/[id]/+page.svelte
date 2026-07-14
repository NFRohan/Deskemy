<script lang="ts">
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import {
    Plus, X, Pencil, Trash2, ChevronUp, ChevronDown, CircleCheck, CircleDot, Circle,
    Video, LoaderCircle, Search,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { pct } from "$lib/format";
  import type { TrackDetail, TrackCourse, CourseSummary } from "$lib/types";

  let track = $state<TrackDetail | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  // modals
  let editing = $state(false);
  let editName = $state("");
  let editDesc = $state("");
  let adding = $state(false);
  let deleteConfirm = $state(false);
  let library = $state<CourseSummary[]>([]);
  let pickQuery = $state("");
  let saving = $state(false);

  const totalDone = $derived(track ? track.courses.reduce((s, c) => s + c.completed_lectures, 0) : 0);
  const totalLect = $derived(track ? track.courses.reduce((s, c) => s + c.lecture_count, 0) : 0);
  const percent = $derived(pct(totalDone, totalLect));
  // First course that isn't fully complete — the one to work on next.
  const nextIndex = $derived(track ? track.courses.findIndex((c) => !isDone(c)) : -1);
  const inTrack = $derived(new Set(track?.courses.map((c) => c.id) ?? []));
  const pickable = $derived(
    library
      .filter((c) => !inTrack.has(c.id))
      .filter((c) => c.title.toLowerCase().includes(pickQuery.trim().toLowerCase())),
  );

  function isDone(c: TrackCourse): boolean {
    return c.lecture_count > 0 && c.completed_lectures >= c.lecture_count;
  }
  function thumbOf(p: string | null): string | null {
    return p ? convertFileSrc(p) : null;
  }

  async function load(id: string) {
    loading = true;
    error = null;
    try {
      track = await api.getTrack(id);
      if (track) setCrumbs([{ label: "Career Tracks", href: "/tracks" }, { label: track.name }]);
      else error = "Track not found.";
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    const id = $page.params.id;
    if (id) load(id);
  });

  function openEdit() {
    if (!track) return;
    editName = track.name;
    editDesc = track.description ?? "";
    editing = true;
  }
  async function saveEdit() {
    if (!track || !editName.trim() || saving) return;
    saving = true;
    try {
      await api.updateTrack(track.id, editName.trim(), editDesc.trim() || null);
      track.name = editName.trim();
      track.description = editDesc.trim() || null;
      setCrumbs([{ label: "Career Tracks", href: "/tracks" }, { label: track.name }]);
      editing = false;
    } finally {
      saving = false;
    }
  }

  async function removeTrack() {
    if (!track) return;
    await api.deleteTrack(track.id).catch(() => {});
    goto("/tracks");
  }

  async function openAdd() {
    adding = true;
    pickQuery = "";
    if (library.length === 0) library = await api.listCourses().catch(() => []);
  }
  async function addCourse(c: CourseSummary) {
    if (!track) return;
    await api.trackAddCourse(track.id, c.id).catch(() => {});
    track.courses.push({
      id: c.id,
      title: c.title,
      thumbnail_path: c.thumbnail_path,
      lecture_count: c.lecture_count,
      completed_lectures: c.completed_count,
    });
  }
  async function removeCourse(c: TrackCourse) {
    if (!track) return;
    await api.trackRemoveCourse(track.id, c.id).catch(() => {});
    track.courses = track.courses.filter((x) => x.id !== c.id);
  }
  async function move(i: number, dir: -1 | 1) {
    if (!track) return;
    const j = i + dir;
    if (j < 0 || j >= track.courses.length) return;
    const arr = [...track.courses];
    [arr[i], arr[j]] = [arr[j], arr[i]];
    track.courses = arr;
    await api.trackReorderCourses(track.id, arr.map((c) => c.id)).catch(() => {});
  }
</script>

<div class="p-6 max-w-4xl mx-auto space-y-6">
  {#if loading}
    <div class="flex items-center gap-2 text-body-sm text-on-surface-variant py-16 justify-center">
      <LoaderCircle size={18} class="animate-spin" /> Loading…
    </div>
  {:else if error || !track}
    <p class="text-body-sm text-error py-16 text-center">{error ?? "Track not found."}</p>
  {:else}
    <!-- Header -->
    <div class="space-y-3">
      <div class="flex items-start justify-between gap-4">
        <div class="min-w-0">
          <h2 class="text-headline-md text-on-surface">{track.name}</h2>
          {#if track.description}
            <p class="text-body-sm text-on-surface-variant mt-1">{track.description}</p>
          {/if}
        </div>
        <div class="flex items-center gap-1 shrink-0">
          <button
            onclick={openEdit}
            class="p-2 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest transition-colors"
            aria-label="Edit track" title="Edit track"
          >
            <Pencil size={16} />
          </button>
          <button
            onclick={() => (deleteConfirm = true)}
            class="p-2 rounded text-on-surface-variant hover:text-error hover:bg-surface-container-highest transition-colors"
            aria-label="Delete track" title="Delete track"
          >
            <Trash2 size={16} />
          </button>
        </div>
      </div>

      <div class="flex items-center gap-3">
        <div class="flex-1 h-2 rounded-full bg-surface-container-highest overflow-hidden">
          <div class="h-full bg-primary rounded-full" style="width: {percent}%"></div>
        </div>
        <span class="text-label-md text-on-surface-variant tabular-nums shrink-0">
          {percent}% · {totalDone} / {totalLect} lectures
        </span>
      </div>
    </div>

    <!-- Courses -->
    <div class="flex items-center justify-between">
      <h3 class="text-label-md text-on-surface-variant uppercase tracking-wide">
        {track.courses.length} course{track.courses.length === 1 ? "" : "s"}
      </h3>
      <button
        onclick={openAdd}
        class="inline-flex items-center gap-1.5 text-label-md bg-surface-container-high text-on-surface px-3 py-1.5 rounded hover:bg-surface-container-highest transition-colors"
      >
        <Plus size={15} /> Add courses
      </button>
    </div>

    {#if track.courses.length === 0}
      <p class="text-body-sm text-on-surface-variant py-12 text-center">
        No courses in this track yet. Add courses to build the learning path — order matters.
      </p>
    {:else}
      <ul class="space-y-2">
        {#each track.courses as c, i (c.id)}
          {@const done = isDone(c)}
          {@const p = pct(c.completed_lectures, c.lecture_count)}
          {@const isNext = i === nextIndex}
          <li
            class="flex items-center gap-3 rounded-lg border p-2.5 transition-colors
              {isNext ? 'border-primary/50 bg-primary-container/10' : 'border-outline-variant bg-surface-container-low'}"
          >
            <!-- order controls -->
            <div class="flex flex-col shrink-0 text-on-surface-variant">
              <button
                onclick={() => move(i, -1)}
                disabled={i === 0}
                class="p-0.5 rounded hover:text-on-surface hover:bg-surface-container-highest disabled:opacity-30 transition-colors"
                aria-label="Move up"
              >
                <ChevronUp size={16} />
              </button>
              <button
                onclick={() => move(i, 1)}
                disabled={i === track.courses.length - 1}
                class="p-0.5 rounded hover:text-on-surface hover:bg-surface-container-highest disabled:opacity-30 transition-colors"
                aria-label="Move down"
              >
                <ChevronDown size={16} />
              </button>
            </div>

            <!-- status -->
            <span class="shrink-0" title={done ? "Completed" : c.completed_lectures > 0 ? "In progress" : "Not started"}>
              {#if done}
                <CircleCheck size={20} class="text-secondary-container" />
              {:else if c.completed_lectures > 0}
                <CircleDot size={20} class="text-primary" />
              {:else}
                <Circle size={20} class="text-outline" />
              {/if}
            </span>

            <!-- course link -->
            <a href={`/course/${c.id}`} class="flex items-center gap-3 flex-1 min-w-0 group">
              <div class="w-16 h-10 rounded bg-surface-container-highest overflow-hidden shrink-0 flex items-center justify-center">
                {#if thumbOf(c.thumbnail_path)}
                  <img src={thumbOf(c.thumbnail_path)} alt="" class="w-full h-full object-cover" />
                {:else}
                  <Video size={18} class="text-outline-variant" />
                {/if}
              </div>
              <div class="min-w-0 flex-1">
                <p class="truncate text-body-md text-on-surface group-hover:text-primary transition-colors">
                  {c.title}
                </p>
                <div class="flex items-center gap-2 mt-1">
                  <span class="h-1 w-24 rounded-full bg-surface-container-highest overflow-hidden">
                    <span class="block h-full bg-primary rounded-full" style="width: {p}%"></span>
                  </span>
                  <span class="text-label-sm text-on-surface-variant tabular-nums">
                    {c.completed_lectures}/{c.lecture_count}
                  </span>
                  {#if isNext}<span class="text-label-sm text-primary font-semibold">Up next</span>{/if}
                </div>
              </div>
            </a>

            <button
              onclick={() => removeCourse(c)}
              class="p-2 rounded text-on-surface-variant hover:text-error hover:bg-surface-container-highest transition-colors shrink-0"
              aria-label="Remove from track" title="Remove from track"
            >
              <X size={16} />
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  {/if}
</div>

<!-- Add courses modal -->
{#if adding && track}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    onclick={(e) => e.target === e.currentTarget && (adding = false)}
    role="presentation"
  >
    <div class="w-full max-w-md max-h-[80vh] bg-surface-container rounded-xl border border-outline-variant flex flex-col">
      <div class="flex items-center justify-between p-4 border-b border-outline-variant">
        <h3 class="text-headline-sm text-on-surface">Add courses</h3>
        <button onclick={() => (adding = false)} class="p-1 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest" aria-label="Close">
          <X size={18} />
        </button>
      </div>
      <div class="p-3 border-b border-outline-variant">
        <div class="flex items-center gap-2 bg-surface-container-low border border-outline-variant rounded-lg px-3">
          <Search size={15} class="text-on-surface-variant" />
          <input
            bind:value={pickQuery}
            placeholder="Filter courses…"
            class="flex-1 bg-transparent py-2 text-body-md text-on-surface focus:outline-none"
          />
        </div>
      </div>
      <div class="overflow-auto p-2">
        {#if pickable.length === 0}
          <p class="text-body-sm text-on-surface-variant text-center py-8">
            {library.length === 0 ? "No courses in your library yet." : "Every matching course is already in this track."}
          </p>
        {:else}
          <ul class="space-y-1">
            {#each pickable as c (c.id)}
              <li>
                <button
                  onclick={() => addCourse(c)}
                  class="flex items-center gap-3 w-full text-left p-2 rounded-lg hover:bg-surface-container-high transition-colors"
                >
                  <div class="w-14 h-9 rounded bg-surface-container-highest overflow-hidden shrink-0 flex items-center justify-center">
                    {#if thumbOf(c.thumbnail_path)}
                      <img src={thumbOf(c.thumbnail_path)} alt="" class="w-full h-full object-cover" />
                    {:else}
                      <Video size={16} class="text-outline-variant" />
                    {/if}
                  </div>
                  <span class="flex-1 min-w-0">
                    <span class="block line-clamp-2 text-body-md text-on-surface leading-snug">{c.title}</span>
                    <span class="block text-label-sm text-on-surface-variant mt-0.5">{c.lecture_count} lectures</span>
                  </span>
                  <Plus size={16} class="text-on-surface-variant shrink-0" />
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </div>
    </div>
  </div>
{/if}

<!-- Edit modal -->
{#if editing && track}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onclick={(e) => e.target === e.currentTarget && (editing = false)} role="presentation">
    <div class="w-full max-w-md bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4">
      <h3 class="text-headline-sm text-on-surface">Edit track</h3>
      <div class="space-y-1">
        <label for="edit-name" class="text-label-md text-on-surface-variant">Name</label>
        <input id="edit-name" bind:value={editName} onkeydown={(e) => e.key === "Enter" && saveEdit()}
          class="w-full bg-surface-container-low border border-outline-variant rounded-lg px-3 py-2 text-body-md text-on-surface focus:outline-none focus:border-primary" />
      </div>
      <div class="space-y-1">
        <label for="edit-desc" class="text-label-md text-on-surface-variant">Description (optional)</label>
        <textarea id="edit-desc" bind:value={editDesc} rows="2"
          class="w-full bg-surface-container-low border border-outline-variant rounded-lg px-3 py-2 text-body-md text-on-surface focus:outline-none focus:border-primary resize-none"></textarea>
      </div>
      <div class="flex justify-end gap-2">
        <button onclick={() => (editing = false)} class="text-label-md text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors">Cancel</button>
        <button onclick={saveEdit} disabled={!editName.trim() || saving}
          class="inline-flex items-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-4 py-2 rounded hover:bg-inverse-primary transition-colors disabled:opacity-60">
          {#if saving}<LoaderCircle size={15} class="animate-spin" />{/if} Save
        </button>
      </div>
    </div>
  </div>
{/if}

<!-- Delete confirm -->
{#if deleteConfirm && track}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4" onclick={(e) => e.target === e.currentTarget && (deleteConfirm = false)} role="presentation">
    <div class="w-full max-w-sm bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4">
      <h3 class="text-headline-sm text-on-surface">Delete "{track.name}"?</h3>
      <p class="text-body-sm text-on-surface-variant">
        This removes the track and its ordering only. Your courses, progress and bookmarks are untouched.
      </p>
      <div class="flex justify-end gap-2">
        <button onclick={() => (deleteConfirm = false)} class="text-label-md text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors">Cancel</button>
        <button onclick={removeTrack} class="text-label-md bg-error text-on-surface px-4 py-2 rounded hover:opacity-90 transition-opacity">Delete track</button>
      </div>
    </div>
  </div>
{/if}
