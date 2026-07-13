<script lang="ts">
  import { onMount } from "svelte";
  import { convertFileSrc } from "@tauri-apps/api/core";
  import { Filter, Play, LoaderCircle, FolderOpen } from "@lucide/svelte";
  import CourseCard from "$lib/components/CourseCard.svelte";
  import ProgressBar from "$lib/components/ProgressBar.svelte";
  import { library, loadLibrary, setCrumbs } from "$lib/stores/app.svelte";
  import { formatDuration, pct } from "$lib/format";

  let filter = $state("");
  let sort = $state<"recent" | "alpha" | "progress">("recent");

  onMount(() => {
    setCrumbs([{ label: "Library" }]);
    // Force so "Continue Watching" reflects recently opened courses.
    loadLibrary(true);
  });

  // Most recently opened, started course → "Continue Watching" hero.
  const hero = $derived(
    library.courses.find((c) => c.last_opened_at != null && c.completed_count < c.lecture_count),
  );

  const filtered = $derived.by(() => {
    let list = library.courses.filter((c) =>
      c.title.toLowerCase().includes(filter.toLowerCase()),
    );
    if (sort === "alpha") {
      list = [...list].sort((a, b) => a.title.localeCompare(b.title));
    } else if (sort === "progress") {
      list = [...list].sort(
        (a, b) =>
          pct(b.completed_count, b.lecture_count) - pct(a.completed_count, a.lecture_count),
      );
    }
    return list;
  });
</script>

<div class="p-6 max-w-7xl mx-auto space-y-8">
  {#if library.loading && !library.loaded}
    <div class="flex items-center justify-center py-32 text-on-surface-variant">
      <LoaderCircle size={28} class="animate-spin" />
    </div>
  {:else if library.error}
    <div class="bg-error/10 border border-error/30 text-error rounded-lg p-4 text-body-sm">
      {library.error}
    </div>
  {:else if library.courses.length === 0}
    <!-- Empty state -->
    <div class="flex flex-col items-center justify-center py-32 text-center gap-3">
      <div class="w-16 h-16 rounded-xl bg-surface-container flex items-center justify-center">
        <FolderOpen size={30} class="text-outline" />
      </div>
      <h2 class="text-headline-md text-on-surface">Your library is empty</h2>
      <p class="text-body-sm text-on-surface-variant max-w-sm">
        Use <span class="text-primary">Add Folder</span> in the sidebar to import a downloaded
        course. Deskemy will auto-structure it into sections and lectures.
      </p>
    </div>
  {:else}
    <!-- Continue Watching -->
    {#if hero}
      {@const heroPct = pct(hero.completed_count, hero.lecture_count)}
      {@const heroImg = hero.resume_thumbnail_path ?? hero.thumbnail_path}
      <section>
        <h2 class="text-headline-sm text-on-surface mb-4">Continue Watching</h2>
        <a
          href={`/course/${hero.id}`}
          class="group relative flex bg-surface-container-low border border-outline-variant rounded-xl overflow-hidden hover:border-primary-container transition-colors"
        >
          <div
            class="w-1/3 relative bg-gradient-to-br from-surface-container-high to-surface-container-lowest flex-shrink-0 flex items-center justify-center min-h-[180px] overflow-hidden"
          >
            {#if heroImg}
              <img
                src={convertFileSrc(heroImg)}
                alt={hero.title}
                class="absolute inset-0 w-full h-full object-cover"
              />
              <div
                class="absolute inset-0 bg-black/25 group-hover:bg-black/10 transition-colors flex items-center justify-center"
              >
                <Play size={44} class="text-white/90 drop-shadow-lg" fill="currentColor" />
              </div>
            {:else}
              <Play size={44} class="text-outline-variant group-hover:text-primary transition-colors" />
            {/if}
          </div>
          <div class="w-2/3 p-6 flex flex-col justify-between">
            <div>
              <p class="text-label-sm text-on-surface-variant mb-2">Continue where you left off</p>
              <h3 class="text-display-sm text-on-surface mb-2 line-clamp-2">{hero.title}</h3>
            </div>
            <div class="mt-6">
              <div class="flex justify-between items-end mb-2">
                <span class="text-label-md text-on-surface">{heroPct}% Complete</span>
                <span class="text-label-sm text-on-surface-variant">
                  {hero.completed_count} / {hero.lecture_count} lectures
                </span>
              </div>
              <ProgressBar value={heroPct} />
            </div>
          </div>
        </a>
      </section>
    {/if}

    <!-- All Courses -->
    <section>
      <div class="flex justify-between items-center mb-6 border-b border-outline-variant pb-4">
        <h2 class="text-headline-sm text-on-surface">All Courses</h2>
        <div class="flex items-center gap-3">
          <div class="relative">
            <input
              bind:value={filter}
              placeholder="Filter courses…"
              class="w-48 bg-background border border-outline-variant rounded text-body-sm text-on-surface pl-3 pr-8 py-1.5 focus:border-accent-blue focus:ring-1 focus:ring-accent-blue outline-none transition-all placeholder:text-on-surface-variant"
            />
            <Filter size={16} class="absolute right-2 top-2 text-on-surface-variant pointer-events-none" />
          </div>
          <select
            bind:value={sort}
            class="bg-background border border-outline-variant rounded text-body-sm text-on-surface px-3 py-1.5 outline-none focus:border-accent-blue"
          >
            <option value="recent">Recent</option>
            <option value="alpha">Alphabetical</option>
            <option value="progress">Progress</option>
          </select>
        </div>
      </div>

      {#if filtered.length === 0}
        <p class="text-body-sm text-on-surface-variant py-8 text-center">No courses match “{filter}”.</p>
      {:else}
        <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
          {#each filtered as course (course.id)}
            <CourseCard {course} />
          {/each}
        </div>
      {/if}
    </section>
  {/if}
</div>
