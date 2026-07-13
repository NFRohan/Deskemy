<script lang="ts">
  import { page } from "$app/stores";
  import { MonitorPlay, ArrowLeft } from "@lucide/svelte";
  import { setCrumbs } from "$lib/stores/app.svelte";

  $effect(() => {
    setCrumbs([{ label: "Library", href: "/" }, { label: "Player" }]);
  });
</script>

<div class="flex flex-col items-center justify-center h-full text-center gap-4 p-8">
  <div class="w-16 h-16 rounded-xl bg-surface-container flex items-center justify-center">
    <MonitorPlay size={30} class="text-outline" />
  </div>
  <h2 class="text-headline-md text-on-surface">Player arrives in M3</h2>
  <p class="text-body-sm text-on-surface-variant max-w-sm">
    The embedded mpv player (resume, speed, subtitles, chapters) is the next milestone. This route
    is wired up and ready for lecture <span class="text-primary">{$page.params.lectureId}</span>.
  </p>
  <a
    href="/"
    class="inline-flex items-center gap-2 text-label-md text-on-surface-variant hover:text-on-surface transition-colors"
  >
    <ArrowLeft size={16} /> Back to library
  </a>
</div>
