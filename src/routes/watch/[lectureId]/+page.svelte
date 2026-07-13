<script lang="ts">
  import { onMount, onDestroy, tick } from "svelte";
  import { page } from "$app/stores";
  import { goto } from "$app/navigation";
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import {
    Play,
    Pause,
    SkipBack,
    SkipForward,
    ArrowLeft,
    Maximize,
    Minimize,
    MonitorPlay,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs, setImmersive, ui } from "$lib/stores/app.svelte";
  import { formatClock } from "$lib/format";
  import type { LectureView, PlayerState } from "$lib/types";

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];

  let paneEl = $state<HTMLDivElement | null>(null);
  let state = $state<PlayerState>({
    lecture_id: null,
    position: 0,
    duration: 0,
    paused: true,
    speed: 1,
    eof: false,
  });
  let lecture = $state<LectureView | null>(null);
  let available = $state<boolean | null>(null);
  let error = $state<string | null>(null);
  let seeking = $state(false);
  let seekValue = $state(0);
  let speedSel = $state(1);

  let unlisten: UnlistenFn[] = [];
  let observer: ResizeObserver | null = null;

  const sliderValue = $derived(seeking ? seekValue : state.position);

  // Keep the speed selector in sync with the backend player.
  $effect(() => {
    if (SPEEDS.includes(state.speed)) speedSel = state.speed;
  });

  function reportRect() {
    if (!paneEl) return;
    const r = paneEl.getBoundingClientRect();
    // Send CSS pixels; the backend converts using the window scale factor.
    api.playerSetRect(r.left, r.top, r.width, r.height).catch(() => {});
  }

  async function reportRectSoon() {
    await tick();
    requestAnimationFrame(() => requestAnimationFrame(reportRect));
  }

  function updateCrumbs() {
    if (!lecture) return;
    setCrumbs([
      { label: "Library", href: "/" },
      { label: lecture.course_title, href: `/course/${lecture.course_id}` },
      { label: lecture.title },
    ]);
  }

  async function load(id: string) {
    error = null;
    try {
      await api.playerOpen(id);
      await reportRectSoon();
      const st = await api.playerState();
      if (st) state = st;
      lecture = await api.lectureGet(id);
      updateCrumbs();
    } catch (e: any) {
      error = e?.message ?? String(e);
    }
  }

  onMount(async () => {
    available = await api.playerAvailable().catch(() => false);

    unlisten.push(
      await listen<PlayerState>("player:state", (e) => {
        if (!seeking) state = e.payload;
        else state = { ...e.payload, position: state.position };
      }),
    );

    if (paneEl) {
      observer = new ResizeObserver(() => reportRect());
      observer.observe(paneEl);
    }
  });

  // (Re)load whenever the route's lecture id changes.
  $effect(() => {
    const id = $page.params.lectureId;
    if (id && available !== false) {
      load(id);
    }
  });

  // Follow autoplay: when the backend advances to a new lecture, refresh header.
  $effect(() => {
    const lid = state.lecture_id;
    if (lid && lid !== lecture?.id) {
      api.lectureGet(lid).then((l) => {
        lecture = l;
        updateCrumbs();
      });
    }
  });

  onDestroy(() => {
    unlisten.forEach((u) => u());
    observer?.disconnect();
    setImmersive(false);
    getCurrentWindow().setFullscreen(false).catch(() => {});
    api.playerStop().catch(() => {});
  });

  function onSeekInput(e: Event) {
    seeking = true;
    seekValue = +(e.target as HTMLInputElement).value;
  }
  function onSeekCommit() {
    api.playerSeek(seekValue).catch(() => {});
    seeking = false;
  }
  async function toggleImmersive() {
    const on = !ui.immersive;
    setImmersive(on); // hide sidebar/titlebar so the video fills the window
    try {
      await getCurrentWindow().setFullscreen(on); // + take the whole display
    } catch {
      /* window may not support it; immersive still applies */
    }
    reportRectSoon();
  }
  function relativeSeek(delta: number) {
    api.playerSeek(Math.max(0, state.position + delta)).catch(() => {});
  }

  function onKey(e: KeyboardEvent) {
    const tag = (e.target as HTMLElement)?.tagName;
    if (tag === "INPUT" || tag === "SELECT" || tag === "TEXTAREA") return;
    switch (e.key) {
      case " ":
        e.preventDefault();
        api.playerTogglePause().catch(() => {});
        break;
      case "ArrowRight":
        relativeSeek(5);
        break;
      case "ArrowLeft":
        relativeSeek(-5);
        break;
      case "f":
        toggleImmersive();
        break;
      case "Escape":
        if (ui.immersive) toggleImmersive();
        break;
      case "n":
        api.playerNext().catch(() => {});
        break;
    }
  }
</script>

<svelte:window onkeydown={onKey} onresize={reportRect} />

{#if available === false}
  <div class="flex flex-col items-center justify-center h-full text-center gap-4 p-8">
    <div class="w-16 h-16 rounded-xl bg-surface-container flex items-center justify-center">
      <MonitorPlay size={30} class="text-outline" />
    </div>
    <h2 class="text-headline-md text-on-surface">mpv is required to play videos</h2>
    <p class="text-body-sm text-on-surface-variant max-w-md">
      Deskemy plays media with libmpv. Install <span class="text-primary">mpv</span> (which provides
      <code>libmpv-2.dll</code>) and make sure it's on your PATH, or set the
      <code>DESKEMY_LIBMPV</code> environment variable to the DLL path, then reopen this lecture.
    </p>
    <a
      href="/"
      class="inline-flex items-center gap-2 text-label-md text-on-surface-variant hover:text-on-surface transition-colors"
    >
      <ArrowLeft size={16} /> Back to library
    </a>
  </div>
{:else}
  <div class="flex flex-col h-full bg-black">
    {#if !ui.immersive}
      <header class="h-14 shrink-0 flex items-center gap-3 px-4 bg-surface border-b border-outline-variant">
        <button
          onclick={() => lecture && goto(`/course/${lecture.course_id}`)}
          class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
          aria-label="Back to course"
        >
          <ArrowLeft size={18} />
        </button>
        <div class="min-w-0">
          <h1 class="text-headline-sm text-on-surface truncate">{lecture?.title ?? "…"}</h1>
          {#if lecture}
            <p class="text-label-sm text-on-surface-variant truncate">
              {lecture.course_title} · {lecture.section_title}
            </p>
          {/if}
        </div>
      </header>
    {/if}

    {#if error}
      <div class="bg-error/10 border-b border-error/30 text-error text-body-sm px-4 py-2">{error}</div>
    {/if}

    <!-- mpv renders into a native child window positioned over this pane -->
    <div bind:this={paneEl} class="flex-1 min-h-0 relative bg-black"></div>

    <!-- Control bar (docked below the video — never overlaps the native surface) -->
    <div class="shrink-0 bg-surface border-t border-outline-variant px-4 py-3 flex flex-col gap-2">
      <div class="flex items-center gap-3">
        <span class="text-label-sm text-on-surface-variant tabular-nums w-12 text-right">
          {formatClock(state.position)}
        </span>
        <input
          type="range"
          min="0"
          max={state.duration || 0}
          step="0.1"
          value={sliderValue}
          oninput={onSeekInput}
          onchange={onSeekCommit}
          class="flex-1 accent-accent-blue cursor-pointer"
        />
        <span class="text-label-sm text-on-surface-variant tabular-nums w-12">
          {formatClock(state.duration)}
        </span>
      </div>

      <div class="flex items-center justify-between">
        <div class="flex items-center gap-1">
          <button
            onclick={() => api.playerPrev()}
            class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
            aria-label="Previous lecture"
          >
            <SkipBack size={18} />
          </button>
          <button
            onclick={() => api.playerTogglePause()}
            class="p-2.5 rounded-full bg-primary-container text-on-primary-container hover:bg-inverse-primary transition-colors"
            aria-label={state.paused ? "Play" : "Pause"}
          >
            {#if state.paused}
              <Play size={20} fill="currentColor" />
            {:else}
              <Pause size={20} fill="currentColor" />
            {/if}
          </button>
          <button
            onclick={() => api.playerNext()}
            class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
            aria-label="Next lecture"
          >
            <SkipForward size={18} />
          </button>
        </div>

        <div class="flex items-center gap-3">
          <select
            bind:value={speedSel}
            onchange={() => api.playerSetSpeed(speedSel)}
            class="bg-background border border-outline-variant rounded text-label-md text-on-surface px-2 py-1 outline-none focus:border-accent-blue"
            aria-label="Playback speed"
          >
            {#each SPEEDS as s (s)}
              <option value={s}>{s}×</option>
            {/each}
          </select>
          <button
            onclick={toggleImmersive}
            class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
            aria-label="Toggle immersive"
          >
            {#if ui.immersive}
              <Minimize size={18} />
            {:else}
              <Maximize size={18} />
            {/if}
          </button>
        </div>
      </div>
    </div>
  </div>
{/if}
