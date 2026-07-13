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
    Captions,
    Languages,
    List,
    Check,
    Volume2,
    Volume1,
    VolumeX,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs, setImmersive, ui } from "$lib/stores/app.svelte";
  import { formatClock } from "$lib/format";
  import type { LectureView, MediaTracks, PlayerState, TrackInfo } from "$lib/types";

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
  const panelItem =
    "w-full flex items-center gap-3 px-6 py-2 text-body-md text-on-surface hover:bg-surface-container text-left";

  let paneEl = $state<HTMLDivElement | null>(null);
  let state = $state<PlayerState>({
    lecture_id: null,
    position: 0,
    duration: 0,
    paused: true,
    speed: 1,
    eof: false,
    sid: null,
    aid: null,
    chapter: -1,
    volume: 100,
    muted: false,
  });
  let lecture = $state<LectureView | null>(null);
  let available = $state<boolean | null>(null);
  let error = $state<string | null>(null);
  let seeking = $state(false);
  let seekValue = $state(0);
  let speedSel = $state(1);
  let tracks = $state<MediaTracks>({ audio: [], subtitle: [], chapters: [] });
  let tracksFor = $state<string | null>(null);
  let openMenu = $state<"sub" | "audio" | "chapters" | null>(null);

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

  // Load track/chapter lists once the file is open (duration known). External
  // subtitles can attach slightly after load, so re-poll a couple of times.
  $effect(() => {
    const lid = state.lecture_id;
    if (lid && state.duration > 0 && tracksFor !== lid) {
      tracksFor = lid;
      refetchTracks();
      setTimeout(refetchTracks, 400);
      setTimeout(refetchTracks, 1200);
    }
  });
  function refetchTracks() {
    api.playerTracks().then((t) => (tracks = t)).catch(() => {});
  }

  const volValue = $derived(state.muted ? 0 : state.volume);
  function toggleMute() {
    api.playerSetMuted(!state.muted).catch(() => {});
  }
  function onVolume(e: Event) {
    const v = +(e.target as HTMLInputElement).value;
    api.playerSetVolume(v).catch(() => {});
    if (v > 0 && state.muted) api.playerSetMuted(false).catch(() => {});
    if (v === 0 && !state.muted) api.playerSetMuted(true).catch(() => {});
  }

  function trackLabel(t: TrackInfo): string {
    // External subs (sidecar .srt): show the full file name so different
    // languages are distinguishable.
    if (t.filename) return t.lang ? `${t.lang} · ${t.filename}` : t.filename;
    const parts: string[] = [];
    if (t.lang) parts.push(t.lang);
    if (t.title) parts.push(t.title);
    if (parts.length === 0) parts.push(`Track ${t.id}`);
    return parts.join(" · ");
  }

  function pickSub(sid: number | null) {
    api.playerSetSubtitle(sid).catch(() => {});
  }
  function pickAudio(aid: number) {
    api.playerSetAudio(aid).catch(() => {});
  }
  function pickChapter(index: number) {
    api.playerSetChapter(index).catch(() => {});
  }
  function toggleMenu(m: "sub" | "audio" | "chapters") {
    openMenu = openMenu === m ? null : m;
    // The panel resizes the video pane; re-sync the mpv window to it.
    reportRectSoon();
  }

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
    const el = e.target as HTMLElement | null;
    const tag = el?.tagName;
    const type = (el as HTMLInputElement | null)?.type;
    // Only ignore shortcuts while typing in a text field — range sliders,
    // buttons and selects must not swallow the player keys.
    const typing =
      tag === "TEXTAREA" ||
      !!el?.isContentEditable ||
      (tag === "INPUT" && !["range", "checkbox", "radio", "button"].includes(type ?? ""));
    if (typing) return;

    switch (e.key) {
      case " ":
        e.preventDefault();
        api.playerTogglePause().catch(() => {});
        break;
      case "ArrowRight":
        e.preventDefault();
        relativeSeek(5);
        break;
      case "ArrowLeft":
        e.preventDefault();
        relativeSeek(-5);
        break;
      case "ArrowUp":
        e.preventDefault();
        api.playerSetVolume(Math.min(100, (state.muted ? 0 : state.volume) + 5)).catch(() => {});
        if (state.muted) api.playerSetMuted(false).catch(() => {});
        break;
      case "ArrowDown":
        e.preventDefault();
        api.playerSetVolume(Math.max(0, state.volume - 5)).catch(() => {});
        break;
      case "m":
        toggleMute();
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

  // Clear focus from a control after use so the keyboard shortcuts keep working.
  function blurSelf(e: Event) {
    (e.currentTarget as HTMLElement | null)?.blur();
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

    <!-- Track/chapter panel: pushes the video up rather than overlapping the
         native surface, so playback continues uninterrupted (just resized). -->
    {#if openMenu}
      <div class="shrink-0 bg-surface border-t border-outline-variant">
        <div class="max-h-56 overflow-y-auto py-1">
          {#if openMenu === "sub"}
            <button onclick={() => pickSub(null)} class={panelItem}>
              <Captions size={16} class="text-on-surface-variant shrink-0" />
              <span class="flex-1">Subtitles off</span>
              {#if state.sid == null}<Check size={16} class="text-primary shrink-0" />{/if}
            </button>
            {#each tracks.subtitle as t (t.id)}
              <button onclick={() => pickSub(t.id)} class={panelItem}>
                <Captions size={16} class="text-on-surface-variant shrink-0" />
                <span class="flex-1 truncate">{trackLabel(t)}</span>
                {#if state.sid === t.id}<Check size={16} class="text-primary shrink-0" />{/if}
              </button>
            {/each}
          {:else if openMenu === "audio"}
            {#each tracks.audio as t (t.id)}
              <button onclick={() => pickAudio(t.id)} class={panelItem}>
                <Languages size={16} class="text-on-surface-variant shrink-0" />
                <span class="flex-1 truncate">{trackLabel(t)}</span>
                {#if state.aid === t.id}<Check size={16} class="text-primary shrink-0" />{/if}
              </button>
            {/each}
          {:else if openMenu === "chapters"}
            {#each tracks.chapters as c (c.index)}
              <button onclick={() => pickChapter(c.index)} class={panelItem}>
                <span class="text-label-sm text-on-surface-variant tabular-nums w-14 shrink-0">
                  {formatClock(c.time)}
                </span>
                <span class="flex-1 truncate">{c.title ?? `Chapter ${c.index + 1}`}</span>
                {#if state.chapter === c.index}<Check size={16} class="text-primary shrink-0" />{/if}
              </button>
            {/each}
          {/if}
        </div>
      </div>
    {/if}

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
          onpointerup={blurSelf}
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

          <!-- Volume (YouTube-style: click icon to mute, drag slider to set) -->
          <div class="flex items-center gap-1 pl-1">
            <button
              onclick={toggleMute}
              class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
              aria-label={state.muted ? "Unmute" : "Mute"}
              title={state.muted ? "Unmute" : "Mute"}
            >
              {#if state.muted || state.volume === 0}
                <VolumeX size={18} />
              {:else if state.volume < 50}
                <Volume1 size={18} />
              {:else}
                <Volume2 size={18} />
              {/if}
            </button>
            <input
              type="range"
              min="0"
              max="100"
              step="1"
              value={volValue}
              oninput={onVolume}
              onpointerup={blurSelf}
              class="w-20 accent-accent-blue cursor-pointer"
              aria-label="Volume"
            />
          </div>
        </div>

        <div class="flex items-center gap-2">
          {#if tracks.chapters.length > 0}
            <button
              onclick={() => toggleMenu("chapters")}
              class="p-2 rounded transition-colors hover:bg-surface-container-highest hover:text-on-surface
                {openMenu === 'chapters' ? 'bg-surface-container-highest text-on-surface' : 'text-on-surface-variant'}"
              title="Chapters"
              aria-label="Chapters"
            >
              <List size={18} />
            </button>
          {/if}
          {#if tracks.audio.length > 1}
            <button
              onclick={() => toggleMenu("audio")}
              class="p-2 rounded transition-colors hover:bg-surface-container-highest hover:text-on-surface
                {openMenu === 'audio' ? 'bg-surface-container-highest text-on-surface' : 'text-on-surface-variant'}"
              title="Audio track"
              aria-label="Audio track"
            >
              <Languages size={18} />
            </button>
          {/if}
          {#if tracks.subtitle.length > 0}
            <button
              onclick={() => toggleMenu("sub")}
              class="p-2 rounded transition-colors hover:bg-surface-container-highest
                {openMenu === 'sub' ? 'bg-surface-container-highest' : ''}
                {state.sid != null ? 'text-primary' : 'text-on-surface-variant hover:text-on-surface'}"
              title="Subtitles"
              aria-label="Subtitles"
            >
              <Captions size={18} />
            </button>
          {/if}

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
