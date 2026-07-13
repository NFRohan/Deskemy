<script lang="ts">
  import { onMount } from "svelte";
  import { Star } from "@lucide/svelte";
  import CourseCard from "$lib/components/CourseCard.svelte";
  import { library, loadLibrary, setCrumbs } from "$lib/stores/app.svelte";

  onMount(() => {
    setCrumbs([{ label: "Favorites" }]);
    loadLibrary(true);
  });

  const favorites = $derived(library.courses.filter((c) => c.is_favorite));
</script>

<div class="p-6 max-w-7xl mx-auto space-y-6">
  <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
    <Star size={18} /> Favorites
  </h2>

  {#if favorites.length === 0}
    <p class="text-body-sm text-on-surface-variant py-16 text-center">
      No favorites yet. Star a course from its page to pin it here.
    </p>
  {:else}
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4 gap-5">
      {#each favorites as course (course.id)}
        <CourseCard {course} />
      {/each}
    </div>
  {/if}
</div>
