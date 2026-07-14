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
    PanelRight,
    ChevronDown,
    ChevronRight,
    CircleCheck,
    Circle,
    X,
    Bookmark as BookmarkIcon,
    BookmarkPlus,
    Trash2,
    Keyboard,
    Paperclip,
    FileText,
    FileArchive,
    FileCode,
    File,
  } from "@lucide/svelte";
  import { api } from "$lib/api";
  import { setCrumbs, setImmersive, ui } from "$lib/stores/app.svelte";
  import { formatClock } from "$lib/format";
  import type {
    Attachment,
    Bookmark,
    CourseDetail,
    Lecture,
    LectureView,
    MediaTracks,
    PlayerState,
    TrackInfo,
  } from "$lib/types";

  const SPEEDS = [0.5, 0.75, 1, 1.25, 1.5, 1.75, 2];
  const panelItem =
    "w-full flex items-center gap-3 px-6 py-2 text-body-md text-on-surface hover:bg-surface-container text-left";

  const SHORTCUTS: { group: string; items: [string, string][] }[] = [
    {
      group: "Playback",
      items: [
        ["Space / K", "Play / pause"],
        ["J / L", "Back / forward 10s"],
        ["← / →", "Back / forward 5s"],
        ["↑ / ↓", "Volume up / down"],
        ["M", "Mute"],
        ["C", "Toggle subtitles"],
        ["< / >", "Speed down / up"],
        ["0 – 9", "Jump to 0–90%"],
        ["Home / End", "Start / end"],
      ],
    },
    {
      group: "Navigation",
      items: [
        ["N / ⇧N", "Next / previous lecture"],
        ["F", "Fullscreen"],
        ["Esc", "Exit / back"],
      ],
    },
    {
      group: "Panels",
      items: [
        ["P", "Course content"],
        ["R", "Resources"],
        ["B", "Bookmark here"],
        ["?", "This cheat sheet"],
      ],
    },
  ];

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
  // A deep-link target from /watch/<id>?t=<sec>, applied once the file loads.
  let pendingSeek = $state<number | null>(null);
  let tracks = $state<MediaTracks>({ audio: [], subtitle: [], chapters: [] });
  let tracksFor = $state<string | null>(null);
  let openMenu = $state<"sub" | "audio" | "chapters" | "bookmarks" | null>(null);
  let course = $state<CourseDetail | null>(null);
  let attachments = $state<Attachment[]>([]);
  let sidebarTab = $state<"content" | "resources">("content");
  let showPlaylist = $state(false);
  let showShortcuts = $state(false);
  const KIND_ICON: Record<string, any> = {
    pdf: FileText,
    archive: FileArchive,
    code: FileCode,
    html: FileCode,
    text: FileText,
  };
  let controlsHidden = $state(false);
  let hideTimer: ReturnType<typeof setTimeout> | undefined;
  let expandedSections = $state<Set<string>>(new Set());
  let bookmarks = $state<Bookmark[]>([]);
  let bookmarksFor = $state<string | null>(null);
  let newBookmarkLabel = $state("");

  let unlisten: UnlistenFn[] = [];
  let observer: ResizeObserver | null = null;

  const sliderValue = $derived(seeking ? seekValue : state.position);

  // The next playable lecture in the course, for the Up Next control.
  const upNext = $derived.by(() => {
    if (!course) return null;
    const flat = course.sections.flatMap((s) => s.lectures);
    const idx = flat.findIndex((l) => l.id === state.lecture_id);
    if (idx < 0) return null;
    for (let i = idx + 1; i < flat.length; i++) {
      if (flat[i].playable) return flat[i];
    }
    return null;
  });

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
      const t = Number($page.url.searchParams.get("t"));
      pendingSeek = Number.isFinite(t) && t > 0 ? t : null;
      await reportRectSoon();
      const st = await api.playerState();
      if (st) state = st;
      lecture = await api.lectureGet(id);
      updateCrumbs();
    } catch (e: any) {
      error = e?.message ?? String(e);
    }
  }

  // Apply a deep-link seek (?t=) once the file has loaded (duration known).
  $effect(() => {
    if (pendingSeek != null && state.duration > 0) {
      const target = pendingSeek;
      pendingSeek = null;
      api.playerSeek(target).catch(() => {});
    }
  });

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

  // Guards load() against re-running when `available` flips null→true (which
  // would double-open the player + double-apply a ?t= seek).
  let loadedKey = "";
  // Pending timeouts, cleared on unmount so they don't fire post-navigation.
  const timers = new Set<ReturnType<typeof setTimeout>>();
  function later(fn: () => void, ms: number) {
    const id = setTimeout(() => {
      timers.delete(id);
      fn();
    }, ms);
    timers.add(id);
  }

  // (Re)load whenever the route's lecture id (or ?t=) changes.
  $effect(() => {
    const id = $page.params.lectureId;
    const t = $page.url.searchParams.get("t") ?? "";
    const key = `${id}?${t}`;
    if (id && available !== false && key !== loadedKey) {
      loadedKey = key;
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
      later(refetchTracks, 400);
      later(refetchTracks, 1200);
    }
  });
  function refetchTracks() {
    api.playerTracks().then((t) => (tracks = t)).catch(() => {});
  }

  // Bookmarks: reload whenever the active lecture changes.
  $effect(() => {
    const lid = state.lecture_id;
    if (lid && bookmarksFor !== lid) {
      bookmarksFor = lid;
      refetchBookmarks(lid);
    }
  });
  function refetchBookmarks(lid: string | null = state.lecture_id) {
    if (!lid) return;
    api.listBookmarks(lid).then((b) => (bookmarks = b)).catch(() => {});
  }
  async function addBookmark() {
    const lid = state.lecture_id;
    if (!lid) return;
    const label = newBookmarkLabel.trim() || null;
    try {
      await api.addBookmark(lid, state.position, label);
      newBookmarkLabel = "";
      refetchBookmarks(lid);
    } catch {
      /* ignore — list stays as-is */
    }
  }
  async function removeBookmark(id: string) {
    await api.deleteBookmark(id).catch(() => {});
    refetchBookmarks();
  }
  function jumpToBookmark(b: Bookmark) {
    api.playerSeek(b.position_seconds).catch(() => {});
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

  // Course-content sidebar: fetch the curriculum and follow the current lecture.
  // Refetch when the course or the active lecture changes (so completion ticks
  // stay current) — but not on every player tick.
  let courseFetchKey = "";
  $effect(() => {
    const cid = lecture?.course_id;
    const lid = state.lecture_id;
    if (!cid) return;
    const key = `${cid}:${lid ?? ""}`;
    if (key === courseFetchKey) return;
    courseFetchKey = key;
    api.getCourse(cid).then((c) => (course = c)).catch(() => {});
  });

  // Course resources for the sidebar's Resources tab (per course).
  let attachFetchKey = "";
  $effect(() => {
    const cid = lecture?.course_id;
    if (!cid || cid === attachFetchKey) return;
    attachFetchKey = cid;
    api.getCourseAttachments(cid).then((a) => (attachments = a)).catch(() => {});
  });

  function sectionProgress(lectures: Lecture[]): { done: number; total: number } {
    return { done: lectures.filter((l) => l.completed).length, total: lectures.length };
  }
  $effect(() => {
    const lid = state.lecture_id;
    if (course && lid) {
      const sec = course.sections.find((s) => s.lectures.some((l) => l.id === lid));
      if (sec && !expandedSections.has(sec.id)) {
        expandedSections = new Set(expandedSections).add(sec.id);
      }
    }
  });

  function togglePlaylist() {
    showPlaylist = !showPlaylist;
    // Re-sync the mpv window to the resized pane a few times as the layout settles.
    reportRectSoon();
    later(reportRect, 80);
    later(reportRect, 250);
  }
  function showSidebar(tab: "content" | "resources") {
    if (showPlaylist && sidebarTab === tab) {
      togglePlaylist(); // same tab already open → close
      return;
    }
    sidebarTab = tab;
    if (!showPlaylist) togglePlaylist();
  }
  function openResource(a: Attachment) {
    api.openResource(a.file_path).catch(() => {});
  }
  function toggleSection(id: string) {
    const next = new Set(expandedSections);
    next.has(id) ? next.delete(id) : next.add(id);
    expandedSections = next;
  }
  function jumpTo(l: Lecture) {
    if (l.playable) goto(`/watch/${l.id}`);
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
  function toggleMenu(m: "sub" | "audio" | "chapters" | "bookmarks") {
    openMenu = openMenu === m ? null : m;
    // The panel resizes the video pane; re-sync the mpv window to it.
    reportRectSoon();
  }

  onDestroy(() => {
    unlisten.forEach((u) => u());
    observer?.disconnect();
    timers.forEach(clearTimeout);
    clearTimeout(hideTimer);
    setImmersive(false);
    getCurrentWindow().setFullscreen(false).catch(() => {});
    // Grab a resume frame for the Continue Watching entry, then stop. The
    // backend player persists past this component, so the awaited grab still
    // captures the current frame before playback is torn down.
    const cid = lecture?.course_id;
    const grab = cid
      ? api.playerGrabResumeFrame(cid).catch(() => {})
      : Promise.resolve();
    grab.finally(() => api.playerStop().catch(() => {}));
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

  // Auto-hide the control bar in fullscreen so the video is truly full-height;
  // hiding it reflows the pane (ResizeObserver re-syncs the mpv window). Mouse
  // over the video goes to the native window, so we reveal via a bottom hover
  // strip and any keypress.
  function revealControls() {
    controlsHidden = false;
    clearTimeout(hideTimer);
    if (ui.immersive) {
      hideTimer = setTimeout(() => (controlsHidden = true), 2500);
    }
  }
  $effect(() => {
    if (ui.immersive) {
      revealControls();
    } else {
      controlsHidden = false;
      clearTimeout(hideTimer);
    }
  });

  function goBack() {
    // Esc / back button: leave immersive first if active, else return to the
    // course view.
    if (ui.immersive) {
      toggleImmersive();
      return;
    }
    const cid = lecture?.course_id;
    goto(cid ? `/course/${cid}` : "/");
  }

  function cycleSpeed(dir: number) {
    let i = SPEEDS.indexOf(state.speed);
    if (i < 0) {
      i = SPEEDS.reduce(
        (best, s, idx) =>
          Math.abs(s - state.speed) < Math.abs(SPEEDS[best] - state.speed) ? idx : best,
        0,
      );
    }
    const next = Math.min(SPEEDS.length - 1, Math.max(0, i + dir));
    api.playerSetSpeed(SPEEDS[next]).catch(() => {});
  }
  function toggleSubtitles() {
    if (state.sid != null) pickSub(null);
    else if (tracks.subtitle.length) pickSub(tracks.subtitle[0].id);
  }
  function seekFraction(f: number) {
    if (state.duration > 0) api.playerSeek(state.duration * f).catch(() => {});
  }
  async function quickBookmark() {
    const lid = state.lecture_id;
    if (!lid) return;
    await api.addBookmark(lid, state.position, null).catch(() => {});
    refetchBookmarks(lid);
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
    revealControls(); // any shortcut brings the controls back in fullscreen

    let handled = true;
    switch (e.key) {
      case " ":
      case "k":
        api.playerTogglePause().catch(() => {});
        break;
      case "j":
        relativeSeek(-10);
        break;
      case "l":
        relativeSeek(10);
        break;
      case "ArrowRight":
        relativeSeek(5);
        break;
      case "ArrowLeft":
        relativeSeek(-5);
        break;
      case "ArrowUp":
        api.playerSetVolume(Math.min(100, (state.muted ? 0 : state.volume) + 5)).catch(() => {});
        if (state.muted) api.playerSetMuted(false).catch(() => {});
        break;
      case "ArrowDown":
        api.playerSetVolume(Math.max(0, state.volume - 5)).catch(() => {});
        break;
      case "m":
        toggleMute();
        break;
      case "f":
        toggleImmersive();
        break;
      case "c":
        toggleSubtitles();
        break;
      case ",":
      case "<":
        cycleSpeed(-1);
        break;
      case ".":
      case ">":
        cycleSpeed(1);
        break;
      case "Home":
        seekFraction(0);
        break;
      case "End":
        seekFraction(0.999);
        break;
      case "p":
        showSidebar("content");
        break;
      case "r":
        showSidebar("resources");
        break;
      case "b":
        quickBookmark();
        break;
      case "n":
        api.playerNext().catch(() => {});
        break;
      case "N":
        api.playerPrev().catch(() => {});
        break;
      case "?":
        showShortcuts = !showShortcuts;
        break;
      case "Escape":
        if (showShortcuts) showShortcuts = false;
        else goBack();
        break;
      default:
        if (e.key >= "0" && e.key <= "9") seekFraction(Number(e.key) / 10);
        else handled = false;
    }
    if (handled) {
      e.preventDefault();
      // A control keeps focus after a mouse click; without this, pressing a
      // shortcut key would flash that control's active/highlight state.
      if (tag === "BUTTON" || tag === "SELECT" || type === "range") el?.blur();
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
    {#if error}
      <div class="bg-error/10 border-b border-error/30 text-error text-body-sm px-4 py-2">{error}</div>
    {/if}

    <div class="flex-1 flex min-h-0">
      <!-- Video area (shrinks when the course-content sidebar opens) -->
      <div class="flex-1 flex flex-col min-w-0">
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
          {:else if openMenu === "bookmarks"}
            <div class="flex items-center gap-2 px-4 py-2 border-b border-outline-variant">
              <input
                type="text"
                bind:value={newBookmarkLabel}
                onkeydown={(e) => e.key === "Enter" && addBookmark()}
                placeholder="Label (optional)"
                class="flex-1 min-w-0 bg-background border border-outline-variant rounded px-2 py-1 text-body-sm text-on-surface outline-none focus:border-accent-blue"
              />
              <button
                onclick={addBookmark}
                class="flex items-center gap-1.5 px-3 py-1 rounded bg-primary-container text-on-primary-container text-label-md hover:bg-inverse-primary transition-colors shrink-0"
              >
                <BookmarkPlus size={15} /> Add at {formatClock(state.position)}
              </button>
            </div>
            {#if bookmarks.length === 0}
              <div class="px-6 py-3 text-body-sm text-on-surface-variant">No bookmarks yet.</div>
            {:else}
              {#each bookmarks as b (b.id)}
                <div class="flex items-center gap-3 px-6 py-1.5 hover:bg-surface-container">
                  <button
                    onclick={() => jumpToBookmark(b)}
                    class="flex items-center gap-3 flex-1 min-w-0 text-left"
                  >
                    <span class="text-label-sm text-on-surface-variant tabular-nums w-14 shrink-0">
                      {formatClock(b.position_seconds)}
                    </span>
                    <span class="flex-1 truncate text-body-md text-on-surface">
                      {b.label ?? "Bookmark"}
                    </span>
                  </button>
                  <button
                    onclick={() => removeBookmark(b.id)}
                    class="p-1 rounded text-on-surface-variant hover:text-error hover:bg-surface-container-highest transition-colors shrink-0"
                    aria-label="Delete bookmark"
                    title="Delete bookmark"
                  >
                    <Trash2 size={15} />
                  </button>
                </div>
              {/each}
            {/if}
          {/if}
        </div>
      </div>
    {/if}

    <!-- Control bar (docked below the video — never overlaps the native surface).
         Auto-hides in fullscreen so the video is truly full-height. -->
    {#if !controlsHidden}
      <div
        class="shrink-0 bg-surface border-t border-outline-variant px-4 py-3 flex flex-col gap-2"
        role="presentation"
        onmousemove={revealControls}
      >
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
            onclick={goBack}
            class="p-2 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
            aria-label="Back to course"
            title="Back to course (Esc)"
          >
            <ArrowLeft size={18} />
          </button>
          <div class="w-px h-5 bg-outline-variant mx-1"></div>
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

        <!-- Up next -->
        {#if upNext}
          <button
            onclick={() => api.playerNext()}
            class="hidden lg:flex items-center gap-2 min-w-0 flex-1 mx-4 px-3 py-1 rounded hover:bg-surface-container-highest transition-colors text-left"
            title={`Up next: ${upNext.title}`}
          >
            <SkipForward size={14} class="text-on-surface-variant shrink-0" />
            <span class="min-w-0 flex-1">
              <span class="block text-label-sm text-on-surface-variant leading-tight">Up next</span>
              <span class="block truncate text-body-sm text-on-surface leading-tight">
                {upNext.title}
              </span>
            </span>
            {#if upNext.duration}
              <span class="text-label-sm text-on-surface-variant tabular-nums shrink-0">
                {formatClock(upNext.duration)}
              </span>
            {/if}
          </button>
        {/if}

        <div class="flex items-center gap-2">
          <button
            onclick={() => toggleMenu("bookmarks")}
            class="p-2 rounded transition-colors hover:bg-surface-container-highest hover:text-on-surface
              {openMenu === 'bookmarks' ? 'bg-surface-container-highest text-on-surface' : 'text-on-surface-variant'}"
            title="Bookmarks"
            aria-label="Bookmarks"
          >
            <BookmarkIcon size={18} />
          </button>
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
            onclick={() => (showShortcuts = true)}
            class="p-2 rounded text-on-surface-variant hover:bg-surface-container-highest hover:text-on-surface transition-colors"
            title="Keyboard shortcuts (?)"
            aria-label="Keyboard shortcuts"
          >
            <Keyboard size={18} />
          </button>
          <button
            onclick={() => showSidebar("content")}
            class="p-2 rounded transition-colors hover:bg-surface-container-highest hover:text-on-surface
              {showPlaylist ? 'bg-surface-container-highest text-on-surface' : 'text-on-surface-variant'}"
            title="Course content (P) · Resources (R)"
            aria-label="Course content"
          >
            <PanelRight size={18} />
          </button>
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
    {:else if ui.immersive}
      <!-- Thin hover strip to bring the auto-hidden controls back in fullscreen -->
      <div class="shrink-0 h-2" role="presentation" onmousemove={revealControls}></div>
    {/if}
      </div>
      <!-- Course content sidebar (Udemy-style: jump around the course here).
           Always rendered; width toggles so the flex row reliably reflows. -->
      <aside
          class="shrink-0 bg-surface-container-low flex flex-col overflow-hidden transition-[width] duration-150
            {showPlaylist ? 'w-80 border-l border-outline-variant' : 'w-0'}"
        >
          <div
            class="h-12 shrink-0 flex items-center justify-between pl-2 pr-2 border-b border-outline-variant"
          >
            <div class="flex items-center gap-1">
              <button
                onclick={() => (sidebarTab = "content")}
                class="px-3 py-1.5 rounded text-label-md transition-colors
                  {sidebarTab === 'content'
                  ? 'bg-surface-container-highest text-on-surface'
                  : 'text-on-surface-variant hover:text-on-surface'}"
              >
                Content
              </button>
              <button
                onclick={() => (sidebarTab = "resources")}
                class="px-3 py-1.5 rounded text-label-md transition-colors
                  {sidebarTab === 'resources'
                  ? 'bg-surface-container-highest text-on-surface'
                  : 'text-on-surface-variant hover:text-on-surface'}"
              >
                Resources{#if attachments.length} · {attachments.length}{/if}
              </button>
            </div>
            <button
              onclick={togglePlaylist}
              class="p-1.5 rounded hover:bg-surface-container-highest text-on-surface-variant hover:text-on-surface transition-colors"
              aria-label="Close panel"
            >
              <X size={16} />
            </button>
          </div>
          <div class="flex-1 overflow-y-auto">
            {#if sidebarTab === "resources"}
              {#if attachments.length === 0}
                <div class="p-4 text-body-sm text-on-surface-variant">
                  No resources in this course.
                </div>
              {:else}
                <ul class="divide-y divide-outline-variant">
                  {#each attachments as a (a.id)}
                    {@const Icon = KIND_ICON[a.kind ?? ""] ?? File}
                    <li>
                      <button
                        onclick={() => openResource(a)}
                        class="w-full flex items-center gap-3 px-3 py-2 text-left hover:bg-surface-container transition-colors"
                        title="Open with default app"
                      >
                        <Icon size={16} class="text-on-surface-variant shrink-0" />
                        <span class="flex-1 min-w-0 truncate text-body-sm text-on-surface">{a.name}</span>
                        {#if a.kind}
                          <span class="text-label-sm text-on-surface-variant shrink-0 uppercase">
                            {a.kind}
                          </span>
                        {/if}
                      </button>
                    </li>
                  {/each}
                </ul>
              {/if}
            {:else if course}
              {#each course.sections as section (section.id)}
                {@const prog = sectionProgress(section.lectures)}
                <div>
                  <button
                    onclick={() => toggleSection(section.id)}
                    class="w-full flex items-center justify-between gap-2 px-3 py-2 hover:bg-surface-container transition-colors text-left"
                  >
                    <span class="flex items-center gap-2 min-w-0">
                      {#if expandedSections.has(section.id)}
                        <ChevronDown size={16} class="text-on-surface-variant shrink-0" />
                      {:else}
                        <ChevronRight size={16} class="text-on-surface-variant shrink-0" />
                      {/if}
                      <span class="text-body-md text-on-surface truncate">{section.title}</span>
                    </span>
                    <span class="text-label-sm text-on-surface-variant shrink-0">
                      {prog.done}/{prog.total}
                    </span>
                  </button>
                  {#if expandedSections.has(section.id)}
                    <ul>
                      {#each section.lectures as l (l.id)}
                        {@const current = l.id === state.lecture_id}
                        <li>
                          <button
                            onclick={() => jumpTo(l)}
                            disabled={!l.playable}
                            class="w-full flex items-center gap-2 pl-8 pr-3 py-1.5 text-left transition-colors
                              {current ? 'bg-primary-container/15' : 'hover:bg-surface-container'}
                              {l.playable ? '' : 'opacity-60 cursor-not-allowed'}"
                          >
                            {#if l.completed}
                              <CircleCheck size={15} class="text-secondary-container shrink-0" />
                            {:else}
                              <Circle size={15} class="text-outline shrink-0" />
                            {/if}
                            <span
                              class="flex-1 truncate text-body-sm
                                {current
                                ? 'text-primary'
                                : l.completed
                                  ? 'text-on-surface-variant'
                                  : 'text-on-surface'}"
                            >
                              {l.title}
                            </span>
                            {#if l.duration}
                              <span class="text-label-sm text-on-surface-variant tabular-nums shrink-0">
                                {formatClock(l.duration)}
                              </span>
                            {/if}
                          </button>
                        </li>
                      {/each}
                    </ul>
                  {/if}
                </div>
              {/each}
            {:else}
              <div class="p-4 text-body-sm text-on-surface-variant">Loading…</div>
            {/if}
          </div>
        </aside>
    </div>

    <!-- Keyboard shortcut cheat sheet -->
    {#if showShortcuts}
      <div
        class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4"
        role="presentation"
        onclick={() => (showShortcuts = false)}
      >
        <div
          class="w-full max-w-2xl bg-surface-container rounded-xl border border-outline-variant p-6"
          role="dialog"
          aria-modal="true"
          aria-label="Keyboard shortcuts"
          onclick={(e) => e.stopPropagation()}
        >
          <div class="flex items-center justify-between mb-4">
            <h3 class="flex items-center gap-2 text-headline-sm text-on-surface">
              <Keyboard size={18} /> Keyboard shortcuts
            </h3>
            <button
              onclick={() => (showShortcuts = false)}
              class="p-1.5 rounded text-on-surface-variant hover:text-on-surface hover:bg-surface-container-highest transition-colors"
              aria-label="Close"
            >
              <X size={18} />
            </button>
          </div>
          <div class="grid grid-cols-1 sm:grid-cols-3 gap-x-6 gap-y-4">
            {#each SHORTCUTS as col (col.group)}
              <div>
                <p class="text-label-md text-on-surface-variant uppercase tracking-wide mb-2">
                  {col.group}
                </p>
                <ul class="space-y-1.5">
                  {#each col.items as [key, desc] (key)}
                    <li class="flex items-center justify-between gap-3">
                      <span class="text-body-sm text-on-surface-variant">{desc}</span>
                      <kbd
                        class="shrink-0 text-label-sm text-on-surface bg-surface-container-highest border border-outline-variant rounded px-1.5 py-0.5 tabular-nums"
                      >
                        {key}
                      </kbd>
                    </li>
                  {/each}
                </ul>
              </div>
            {/each}
          </div>
        </div>
      </div>
    {/if}
  </div>
{/if}
