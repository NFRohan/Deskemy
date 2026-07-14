<script lang="ts">
  import { onMount } from "svelte";
  import { goto } from "$app/navigation";
  import { Route, Plus, LoaderCircle, X } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs } from "$lib/stores/app.svelte";
  import { pct } from "$lib/format";
  import type { TrackSummary } from "$lib/types";

  let tracks = $state<TrackSummary[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);

  let creating = $state(false);
  let newName = $state("");
  let newDesc = $state("");
  let saving = $state(false);

  async function load() {
    loading = true;
    error = null;
    try {
      tracks = await api.listTracks();
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    setCrumbs([{ label: "Career Tracks" }]);
    load();
  });

  function openCreate() {
    newName = "";
    newDesc = "";
    creating = true;
  }

  async function create() {
    const name = newName.trim();
    if (!name || saving) return;
    saving = true;
    try {
      const id = await api.createTrack(name, newDesc.trim() || null);
      creating = false;
      goto(`/tracks/${id}`);
    } catch (e: any) {
      error = e?.message ?? String(e);
    } finally {
      saving = false;
    }
  }
</script>

<div class="p-6 max-w-5xl mx-auto space-y-6">
  <div class="flex items-center justify-between gap-4">
    <h2 class="flex items-center gap-2 text-headline-sm text-on-surface">
      <Route size={18} /> Career Tracks
    </h2>
    <button
      onclick={openCreate}
      class="inline-flex items-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-3 py-2 rounded hover:bg-inverse-primary transition-colors"
    >
      <Plus size={16} /> New track
    </button>
  </div>

  {#if loading}
    <div class="flex items-center gap-2 text-body-sm text-on-surface-variant py-16 justify-center">
      <LoaderCircle size={18} class="animate-spin" /> Loading…
    </div>
  {:else if error}
    <p class="text-body-sm text-error py-16 text-center">{error}</p>
  {:else if tracks.length === 0}
    <div class="text-center py-16 space-y-3">
      <p class="text-body-md text-on-surface-variant">
        No tracks yet. Group courses into an ordered learning path — like "Platform Engineering":
        Linux → Docker → Kubernetes.
      </p>
      <button
        onclick={openCreate}
        class="inline-flex items-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-4 py-2 rounded hover:bg-inverse-primary transition-colors"
      >
        <Plus size={16} /> New track
      </button>
    </div>
  {:else}
    <div class="grid gap-3 sm:grid-cols-2">
      {#each tracks as t (t.id)}
        {@const p = pct(t.completed_lectures, t.total_lectures)}
        <a
          href={`/tracks/${t.id}`}
          class="block bg-surface-container-low border border-outline-variant rounded-xl p-4 hover:bg-surface-container transition-colors"
        >
          <div class="flex items-start justify-between gap-3">
            <h3 class="text-headline-sm text-on-surface min-w-0 truncate">{t.name}</h3>
            <span class="text-label-md text-on-surface-variant tabular-nums shrink-0">{p}%</span>
          </div>
          {#if t.description}
            <p class="text-body-sm text-on-surface-variant mt-1 line-clamp-2">{t.description}</p>
          {/if}
          <p class="text-label-sm text-on-surface-variant mt-2 tabular-nums">
            {t.course_count} course{t.course_count === 1 ? "" : "s"} · {t.completed_lectures} / {t.total_lectures}
            lectures
          </p>
          <div class="mt-2 h-1.5 rounded-full bg-surface-container-highest overflow-hidden">
            <div class="h-full bg-primary rounded-full" style="width: {p}%"></div>
          </div>
        </a>
      {/each}
    </div>
  {/if}
</div>

{#if creating}
  <div
    class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
    onclick={(e) => e.target === e.currentTarget && (creating = false)}
    role="presentation"
  >
    <div class="w-full max-w-md bg-surface-container rounded-xl border border-outline-variant p-5 space-y-4">
      <div class="flex items-center justify-between">
        <h3 class="text-headline-sm text-on-surface">New career track</h3>
        <button
          onclick={() => (creating = false)}
          class="p-1 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest"
          aria-label="Close"
        >
          <X size={18} />
        </button>
      </div>
      <div class="space-y-1">
        <label for="track-name" class="text-label-md text-on-surface-variant">Name</label>
        <input
          id="track-name"
          bind:value={newName}
          onkeydown={(e) => e.key === "Enter" && create()}
          placeholder="e.g. Platform Engineering"
          class="w-full bg-surface-container-low border border-outline-variant rounded-lg px-3 py-2 text-body-md text-on-surface focus:outline-none focus:border-primary"
        />
      </div>
      <div class="space-y-1">
        <label for="track-desc" class="text-label-md text-on-surface-variant">Description (optional)</label>
        <textarea
          id="track-desc"
          bind:value={newDesc}
          rows="2"
          class="w-full bg-surface-container-low border border-outline-variant rounded-lg px-3 py-2 text-body-md text-on-surface focus:outline-none focus:border-primary resize-none"
        ></textarea>
      </div>
      <div class="flex justify-end gap-2">
        <button
          onclick={() => (creating = false)}
          class="text-label-md text-on-surface px-3 py-2 rounded hover:bg-surface-container-highest transition-colors"
        >
          Cancel
        </button>
        <button
          onclick={create}
          disabled={!newName.trim() || saving}
          class="inline-flex items-center gap-1.5 text-label-md bg-primary-container text-on-primary-container px-4 py-2 rounded hover:bg-inverse-primary transition-colors disabled:opacity-60"
        >
          {#if saving}<LoaderCircle size={15} class="animate-spin" />{/if} Create
        </button>
      </div>
    </div>
  </div>
{/if}
