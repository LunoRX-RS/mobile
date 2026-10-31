"use client";
import { useLayout } from "../../lib/layout";
import { ArtistLinks, LikeButton } from "../LibraryActions";
import NativeQueue from "../../features/queue/NativeQueue";
import { AnimatePresence, motion, useDragControls } from "motion/react";
import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type ReactNode, type RefObject } from "react";
import { AlignLeft, Check, ChevronDown, ChevronLeft, Disc3, FilePenLine, Heart, HeartOff, ListEnd, ListMusic, ListPlus, Menu, MicVocal, Moon, Shuffle, Timer, User, Maximize2, Minimize2, PanelLeft, Pause, Play, SkipBack, SkipForward, Volume1, Volume2, VolumeX } from "lucide-react";
import { useUi } from "../../lib/store";
import { usePlayback, togglePlayback, seekPlayback, setPlaybackVolume } from "../../lib/playback";
import { playerQueueEdit, playerSkip } from "../../services/tauri/player";
import { Drawer } from "../Drawer";
import { backStep } from "../../lib/backNav";
import { toggleSaved, trackKey, useLibrary } from "../../lib/library";
import { addToPlaylist, listPlaylists } from "../../lib/playlistActions";
import type { LocalPlaylistDto } from "../../services/tauri/sync";
import { formatRemaining } from "../../lib/sleepTimerFormat";
import { setSleepEndOfTrack, setSleepTimer, useSleepTimer } from "../../lib/sleepTimer";
import { RepeatButton, AutoplayButton } from "../PlayModeButtons";
import { useMotion } from "../../lib/motion";
import { TrackCover } from "../TrackCover";
import { Button, IconButton, Scrubber, cx } from "../ui";
import { formatDuration } from "../../lib/media";
import { SyncedLyrics, UntimedLyrics, type LyricsSlots } from "../Lyrics";
import { LyricsSourceControl } from "../LyricsSourceSheet";
import { anchorKey } from "../../lib/lyricAnchors";
import { chooseLyrics, sourcesOf, useLyricsSource } from "../../lib/lyricsSource";
import { useLyricsStore } from "../../lib/lyricsStore";
import { clampOffset, offsetKey, readOffset, writeOffset } from "../../lib/lyricsTiming";
import { readLyricsLayout, writeLyricsLayout, type LyricsLayout } from "../../lib/lyricsLayout";
import { FxText } from "../FxText";
import { useZoneStyle } from "../../lib/textStyle";
import { useFullscreen } from "../../lib/windowFullscreen";
import { LyricMaker } from "../LyricMaker";
import { mineSource, useMyLrc, useMyLrcStore } from "../../lib/myLrc";
import { track as trackUse } from "../../lib/telemetry";
import { parseLrc } from "../../lib/lrcParse";
import { lrcSave } from "../../services/tauri/lrc";

// Full window only: the controls and the close button fade after this long without input while playing.
const IDLE_MS = 3000;

/**
 * True after IDLE_MS without pointer, key, touch or wheel input on `root`. `hold` can veto (focus or a popover is open).
 * Never idle while `active` is false (paused, not in Full window) or under reduced motion.
 */
function useIdle(active: boolean, root: RefObject<HTMLElement | null>, hold: () => boolean): boolean {
  const { reduce } = useMotion();
  const [idle, setIdle] = useState(false);
  useEffect(() => {
    setIdle(false);
    const el = root.current;
    if (!active || reduce || !el) return;
    let timer = 0;
    let x = -1;
    let y = -1;
    const arm = () => { window.clearTimeout(timer); timer = window.setTimeout(() => { if (hold()) arm(); else setIdle(true); }, IDLE_MS); };
    const wake = () => { setIdle(false); arm(); };
    // The browser can replay a pointermove at the same spot after the lines scroll under a still cursor; that is not input.
    const move = (event: PointerEvent) => { if (event.clientX === x && event.clientY === y) return; x = event.clientX; y = event.clientY; wake(); };
    const others = ["pointerdown", "keydown", "touchstart", "wheel"] as const;
    el.addEventListener("pointermove", move);
    others.forEach((name) => el.addEventListener(name, wake, { passive: true }));
    arm();
    return () => { window.clearTimeout(timer); el.removeEventListener("pointermove", move); others.forEach((name) => el.removeEventListener(name, wake)); };
  }, [active, reduce, root, hold]);
  return idle;
}

const LAYOUTS = [
  { id: "full", label: "Full window lyrics", Icon: AlignLeft },
  { id: "split", label: "Artwork and lyrics", Icon: PanelLeft },
] as const;

function LayoutToggle({ value, onChange }: { value: LyricsLayout; onChange: (next: LyricsLayout) => void }) {
  const group = useRef<HTMLDivElement>(null);
  const onKeyDown = (event: KeyboardEvent) => {
    const step = event.key === "ArrowRight" || event.key === "ArrowDown" ? 1 : event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 0;
    if (!step) return;
    event.preventDefault();
    const next = LAYOUTS[(LAYOUTS.findIndex((l) => l.id === value) + step + LAYOUTS.length) % LAYOUTS.length].id;
    onChange(next);
    group.current?.querySelector<HTMLElement>(`[data-layout-opt="${next}"]`)?.focus();
  };
  return (
    <div ref={group} role="radiogroup" aria-label="Lyrics layout" onKeyDown={onKeyDown} className="ln-lyr-seg">
      {LAYOUTS.map(({ id, label, Icon }) => (
        <button key={id} type="button" role="radio" data-layout-opt={id} aria-checked={value === id} aria-label={label} title={label}
          tabIndex={value === id ? 0 : -1} onClick={() => onChange(id)} data-icon="" className="ln-lyr-seg-btn size-8 justify-center !px-0">
          <Icon size={16} aria-hidden="true" />
        </button>
      ))}
    </div>
  );
}

type LyricsGap = "loading" | "none" | "instrumental" | "failed";

/** Loading, no lyrics, instrumental and failed. With `cover`, the artwork leads (Full window); in Split the artwork is already on the left. */
function LyricsGap({ kind, cover }: { kind: LyricsGap; cover: ReactNode }) {
  if (kind === "loading") {
    return (
      <div className="ln-lyr-region flex items-start pt-[34vh] text-[length:var(--lyr-size)]">
        <p role="status" className="sr-only">Loading lyrics</p>
        <div aria-hidden="true" className="ln-np-skeleton mx-auto flex w-[min(22em,100%)] flex-col gap-[0.5em] px-[clamp(24px,4vw,72px)]">
          {[0, 1, 2, 3, 4, 5].map((i) => <div key={i} className="lx-shimmer" />)}
        </div>
      </div>
    );
  }
  const text = kind === "failed" ? "Lyrics could not be loaded" : kind === "instrumental" ? "Instrumental · no lyrics" : "No lyrics found for this track";
  const hint = kind === "failed" ? "Playback can continue." : kind === "instrumental" ? "This track has no words to follow." : "Checked every source that is turned on.";
  return (
    <div className="flex h-full flex-col items-center justify-center gap-6 px-8 pb-24 text-center">
      {cover}
      <div role={kind === "failed" ? "alert" : "status"}>
        <p className="font-display text-[22px] font-semibold tracking-[-0.01em]">{text}</p>
        <p className="mt-1 text-[14px] text-secondary-text">{hint}</p>
      </div>
    </div>
  );
}

const isTextTarget = (t: EventTarget | null) => {
  const el = t as HTMLElement | null;
  if (!el) return false;
  if (el.isContentEditable || el.tagName === "TEXTAREA" || el.tagName === "SELECT") return true;
  return el.tagName === "INPUT" && !["range", "checkbox", "radio", "button"].includes((el as HTMLInputElement).type);
};

export function NativeNowPlaying() {
  const phone = useLayout() === "phone";
  const s = usePlayback();
  const open = useUi((state) => state.nowPlayingOpen);
  const lyricsOpen = useUi((state) => state.lyricsOpen);
  const dialog = useRef<HTMLDialogElement>(null);
  const stage = useRef<HTMLDivElement>(null);
  const pill = useRef<HTMLDivElement>(null);
  const track = s.requested ?? s.track;
  const info = s.info?.providerTrackId === track?.providerTrackId ? s.info : null;
  const title = info?.title ?? track?.title ?? "YouTube";
  const playing = s.player.state === "playing" || s.player.state === "buffering";
  const titleFx = useZoneStyle("title").effect;
  const canSeek = !s.busy && s.durationMs > 0 && ["ready", "playing", "paused", "buffering"].includes(s.player.state);
  const [pref, setPref] = useLyricsSource();
  const myLrc = useMyLrc(track?.providerTrackId);
  const lyricsSettings = useLyricsStore((st) => st.settings);
  const overrides = useLyricsStore((st) => (track ? st.overrides[track.providerTrackId] : undefined));
  // Entries replaced by Try again, a hand-picked match or an unbind win over what Track info delivered.
  const sources = useMemo(() => {
    if (!info) return [];
    const base = sourcesOf(info).map((x) => overrides?.[x.id] ?? x);
    const extra = Object.values(overrides ?? {}).filter((x) => !base.some((b) => b.id === x.id));
    const mine = mineSource(myLrc);
    return [...base, ...extra, ...(mine.available ? [mine] : [])];
  }, [info, myLrc, overrides]);
  const [maker, setMaker] = useState(false);
  // The queue opens over the lyrics, inside the dialog: anything else would sit behind the modal.
  const queueView = useUi((state) => state.npQueue);
  const setQueueView = (value: boolean | ((v: boolean) => boolean)) => useUi.setState((state) => ({ npQueue: typeof value === "function" ? value(state.npQueue) : value }));
  useEffect(() => { if (!open) setMaker(false); }, [open]);
  // Only real LRCLIB timestamps drive a current line; the chosen source decides which text and lines show.
  const choice = useMemo(() => chooseLyrics(pref, sources, lyricsSettings?.order), [pref, sources, lyricsSettings]);
  const { source: picked, lines: timed, note } = choice;
  const text = picked?.text ?? null;
  const lyricKey = `${track?.providerTrackId}:${picked?.id}`;
  // Timing offset: per track and source, only for real timestamps.
  const offKey = track && picked && timed ? offsetKey(track.providerTrackId, picked.id) : null;
  const [offState, setOffState] = useState<{ key: string | null; ms: number }>({ key: null, ms: 0 });
  const offsetMs = offKey ? (offState.key === offKey ? offState.ms : readOffset(offKey)) : 0;
  const changeOffset = (ms: number) => { if (!offKey) return; const next = clampOffset(ms); setOffState({ key: offKey, ms: next }); writeOffset(offKey, next); };
  const openLyricsSettings = () => {
    try { window.localStorage.setItem("lunorx.settings.category", "playback"); } catch { /* storage unavailable: Settings opens on its last category */ }
    useUi.getState().setNowPlaying(false);
    useUi.getState().navigate("settings");
    // An already-open Settings jumps at once; a new one reads the saved category, then scrolls to the group.
    window.setTimeout(() => window.dispatchEvent(new CustomEvent("lunorx:settings-open", { detail: { cat: "playback", group: "settings-lyrics" } })), 150);
  };
  const [layout, setLayoutState] = useState<LyricsLayout>(readLyricsLayout);
  const setLayout = (next: LyricsLayout) => { setLayoutState(next); writeLyricsLayout(next); };
  const [chrome, setChrome] = useState<HTMLElement | null>(null);
  const [notes, setNotes] = useState<HTMLElement | null>(null);
  const trackId = track?.providerTrackId;
  const trackTitle = info?.title ?? track?.title ?? "";
  const trackArtists = info?.artists ?? track?.artists ?? [];
  const slots = useMemo<LyricsSlots>(() => ({
    chrome, notes, onMake: () => setMaker(true),
    onImport: async (file) => {
      if (!trackId || file.size > 1024 * 1024) throw new Error("file");
      const lines = parseLrc(await file.text());
      if (lines.length === 0) throw new Error("empty");
      const args = { providerTrackId: trackId, title: trackTitle, artists: trackArtists, durationMs: usePlayback.getState().durationMs, lines };
      const saved = await lrcSave(args);
      useMyLrcStore.getState().set(trackId, { exists: true, path: saved.path, lines });
      trackUse("feature.lrc-import");
      return lines.length;
    },
  }), [chrome, notes, trackId, trackTitle]);
  const full = lyricsOpen && (phone || layout === "full");
  const fullscreen = useFullscreen(open);
  const hold = useCallback(() => {
    const at = document.activeElement;
    return [pill.current].some((el) => !!el && ((!!at && el.contains(at) && at.matches(":focus-visible")) || !!el.querySelector('[aria-expanded="true"]')));
  }, []);
  const idle = useIdle(open && full && s.player.state === "playing", stage, hold);
  useEffect(() => {
    if (open && !dialog.current?.open) {
      dialog.current?.showModal();
      dialog.current?.focus(); // keep the dialog itself focused: no control shows a focus ring until the user presses Tab
    }
    else if (!open && !phone) dialog.current?.close(); // on a phone the sheet closes the dialog once it has slid away
  }, [open]);
  const sheetDrag = useDragControls();
  const panel = useUi((u) => u.npPanel);
  const [menuView, setMenuView] = useState<"menu" | "playlists">("menu");
  const [playlists, setPlaylists] = useState<LocalPlaylistDto[] | null>(null);
  const timer = useSleepTimer();
  const [, tick] = useState(0);
  const likedNow = useLibrary((st) => !!track && st.tracks.some((t) => trackKey(t) === trackKey({ provider: track.provider, providerTrackId: track.providerTrackId })));
  useEffect(() => { if (panel !== "timer" || !timer.endsAt) return; const id = window.setInterval(() => tick((n) => n + 1), 1000); return () => window.clearInterval(id); }, [panel, timer.endsAt]);
  useEffect(() => { if (panel === "menu" && menuView === "playlists" && playlists === null) listPlaylists().then(setPlaylists).catch(() => setPlaylists([])); }, [panel, menuView, playlists]);
  useEffect(() => { if (panel !== "menu") setMenuView("menu"); }, [panel]);
  if (!track) return null;
  const artwork = info?.artworkUrl ?? track.artworkUrl;
  const artists = info?.artists ?? track.artists ?? [];
  const cover = (className: string) => (
    <TrackCover key={track.providerTrackId} url={artwork} videoId={track.providerTrackId} title={title} className={className} />
  );
  const gap: LyricsGap | null = s.infoState === "error" ? "failed" : !info ? "loading"
    : picked ? null : info.lyricsStatus === "instrumental" ? "instrumental" : info.lyricsStatus === "failed" ? "failed" : "none";
  const lyrics = (kind: "full" | "split") => (
    <section aria-label="Lyrics" className={cx("min-h-0", kind === "full" ? "absolute inset-0" : "h-full")}>
      {gap ? <LyricsGap kind={gap} cover={kind === "full" ? cover("lx-fade-in aspect-square w-[min(34vh,320px)] rounded-[18px] shadow-[0_24px_80px_rgba(0,0,0,0.6)]") : null} />
        : open && timed ? <SyncedLyrics key={lyricKey} lines={timed} offsetMs={offsetMs} source={picked?.label ?? null} attribution={picked?.attribution} canSeek={canSeek} onSeek={(ms) => void seekPlayback(ms / 1000)} slots={slots} layout={kind} note={note} />
        : open && text && picked ? <UntimedLyrics key={lyricKey} anchorId={anchorKey(track.providerTrackId, picked.id)} text={text} source={picked.label} attribution={picked.attribution} instrumental={info?.lyricsStatus === "instrumental"} slots={slots} layout={kind} note={note} />
        : null}
    </section>
  );
  const notesSlot = <div ref={setNotes} className="ln-np-notes empty:hidden" />;
  const likeTrack = { ...track, title, artists, artistRefs: info?.artistRefs ?? track.artistRefs, artworkUrl: artwork ?? null, durationMs: s.durationMs || null };
  const artistLine = <ArtistLinks track={{ provider: track.provider, artists, artistRefs: info?.artistRefs ?? track.artistRefs }} />;
  const eq = s.player.state === "playing" && <span className="lx-eq" data-live="true" aria-label="Playing"><i /><i /><i /></span>;
  const bigInfo = (
    <div className="flex w-full max-w-[440px] flex-col gap-5">
      {cover(cx("lx-fade-in aspect-square w-full rounded-[18px] shadow-[0_24px_80px_rgba(0,0,0,0.6)] transition-transform duration-[320ms] motion-reduce:transition-none", playing ? "scale-100" : "scale-[0.96]"))}
      <div>
        <h2 className="ln-np-title font-display text-[28px] leading-9 font-semibold tracking-[-0.02em]"><FxText key={title} text={title} effect={titleFx} playing={playing} /></h2>
        <p className="mt-1 text-[17px] text-secondary-text">{artistLine}</p>
        <LikeButton track={likeTrack} />
        {playing && <span className="lx-eq mt-4" data-live={s.player.state === "playing"} aria-label="Playing"><i /><i /><i /></span>}
        {lyricsOpen && notesSlot}
      </div>
    </div>
  );
  // The traffic lights overlay the top-left (always windowed, on hover in full screen); keep content clear of them.
  const inset = phone ? "pl-4" : "pl-[100px]";
  const topRegion = full ? (
    <div className="absolute inset-0">
      {lyrics("full")}
      <div data-tauri-drag-region className="absolute inset-x-0 top-0 h-11" />
      <div className={cx("pointer-events-none absolute left-0 top-0 z-10 pr-6", inset)}>
        {/* 44px row so the cover and text centre on the traffic lights (tauri.conf.json trafficLightPosition y:16 + 6). */}
        <div className="pointer-events-auto flex h-14 items-center gap-3.5 mt-5">
          {cover("lx-fade-in size-12 shrink-0 rounded-[8px]")}
          <div className="min-w-0 max-w-[min(40vw,520px)]">
            <div className="flex items-center gap-1">
              <h2 className="ln-np-title truncate font-display text-[18px] leading-6 font-semibold"><FxText key={title} text={title} effect={titleFx} playing={playing} /></h2>
              <LikeButton track={likeTrack} />
            </div>
            <p className="flex items-center gap-2 truncate text-[14px] leading-5 text-secondary-text">{artistLine}{eq}</p>
          </div>
        </div>
        <div className="pointer-events-auto max-w-[min(40vw,520px)]">{notesSlot}</div>
      </div>
    </div>
  ) : (
    <div className="flex min-h-0 flex-1 flex-col">
      <div data-tauri-drag-region className={cx("flex shrink-0 items-center", phone ? "h-4" : "h-[60px]", inset)}></div>
      {lyricsOpen ? (
        <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,420px)_minmax(0,1fr)] grid-rows-[minmax(0,1fr)] items-center gap-14">
          {bigInfo}
          {lyrics("split")}
        </div>
      ) : <div className="flex min-h-0 flex-1 items-center justify-center">{bigInfo}</div>}
    </div>
  );
  const seekable = !!s.durationMs && ["ready", "playing", "paused", "buffering"].includes(s.player.state);
  const stageEl = (
      <div ref={stage} inert={maker} data-layout={full ? "full" : "page"} data-idle={idle}
        className={cx("ln-np-stage relative h-full", maker && "invisible", phone ? "[--np-inset:16px]" : "[--np-inset:100px]", !full && (phone ? "mx-auto flex max-w-content flex-col px-5 pb-4" : "mx-auto flex max-w-content flex-col px-10 pb-8"))}>
        {topRegion}
        <div className={cx(full ? "pointer-events-none absolute inset-x-0 bottom-3 z-10 flex justify-center px-3" : "mt-3 flex justify-center")}>
          <div ref={pill} className="ln-np-pill ln-np-chrome pointer-events-auto" role="group" aria-label="Playback controls">
            <div className="flex items-center gap-3 text-[11px] tabular-nums text-secondary-text">
              <span>{formatDuration(s.positionMs / 1000)}</span>
              <Scrubber label="Seek YouTube" max={Math.max(1, s.durationMs / 1000)} value={s.positionMs / 1000}
                disabled={!seekable} onCommit={(value) => void seekPlayback(value)} className="flex-1" />
              <span>{formatDuration(s.durationMs / 1000)}</span>
              <span className={cx("ml-2 items-center gap-1.5", phone ? "hidden" : "inline-flex")}>
                {s.volume === 0 ? <VolumeX size={16} aria-hidden="true" /> : s.volume < 50 ? <Volume1 size={16} aria-hidden="true" /> : <Volume2 size={16} aria-hidden="true" />}
                <Scrubber label="Volume" max={100} value={s.volume} disabled={!seekable} onChange={(value) => void setPlaybackVolume(value)} className="w-24" />
              </span>
            </div>
            <div className="flex items-center justify-center">
              <div className="flex items-center gap-1">
                <RepeatButton className="size-9" />
                <IconButton label="Previous track" disabled={s.busy || !seekable} onClick={() => void playerSkip(false)} className="size-10"><SkipBack size={22} fill="currentColor" /></IconButton>
                <IconButton label={playing ? "Pause YouTube" : "Play YouTube"} disabled={s.busy} onClick={() => void togglePlayback()} className="size-11">
                  {playing ? <Pause size={28} fill="currentColor" /> : <Play size={28} fill="currentColor" />}
                </IconButton>
                <IconButton label="Next track" disabled={s.busy || !seekable} onClick={() => void playerSkip(true)} className="size-10"><SkipForward size={22} fill="currentColor" /></IconButton>
                <AutoplayButton className="size-9" />
              </div>
            </div>
            <div className="flex items-center justify-between gap-3">
              <div className="flex min-w-0 items-center justify-start">
                {lyricsOpen && (
                  <LyricsSourceControl track={{ providerTrackId: track.providerTrackId, title, artists, durationMs: s.durationMs }} sources={sources} pref={pref} choice={choice}
                    info={s.infoState === "error" ? "error" : info ? "ready" : "loading"} onPick={setPref} onEntry={(entry) => useLyricsStore.getState().setOverride(track.providerTrackId, entry)}
                    offsetApplies={!!offKey} offsetMs={offsetMs} onOffset={changeOffset} onOpenSettings={openLyricsSettings} />)}
              </div>
              <div className="flex items-center justify-end gap-1">
                {lyricsOpen && <span ref={setChrome} className="inline-flex items-center gap-1" />}
                {lyricsOpen && !phone && <LayoutToggle value={layout} onChange={setLayout} />}
                {fullscreen.supported && (
                  <IconButton label={fullscreen.active ? "Exit full screen" : "Enter full screen"} onClick={fullscreen.toggle} className="size-9">
                    {fullscreen.active ? <Minimize2 size={18} /> : <Maximize2 size={18} />}
                  </IconButton>)}
                {!phone && <IconButton label="Make lyrics file" active={maker} aria-pressed={maker} onClick={() => setMaker(true)} className="size-9"><FilePenLine size={20} /></IconButton>}
                <IconButton label="Queue" active={phone ? panel === "queue" : queueView} aria-pressed={phone ? panel === "queue" : queueView} onClick={() => (phone ? useUi.setState({ npPanel: "queue" }) : setQueueView((v) => !v))} className="size-9"><ListMusic size={20} /></IconButton>
                <IconButton label="Show lyrics" active={lyricsOpen} aria-pressed={lyricsOpen} onClick={() => useUi.getState().setLyrics(!lyricsOpen)} className="size-9"><MicVocal size={20} /></IconButton>
                <IconButton label="Collapse Now Playing" onClick={() => useUi.getState().setNowPlaying(false)} className="size-9"><ChevronDown size={20} /></IconButton>
              </div>
            </div>
            {s.error && <p role="alert" className="text-center text-[12px] text-critical-coral">{s.error}</p>}
          </div>
        </div>
      </div>
  );
  const queueOverlay = (
    queueView && (
        <div role="region" aria-label="Queue" className="absolute inset-0 z-30 flex flex-col bg-night-canvas/95 backdrop-blur-xl">
          <div className="flex shrink-0 items-center justify-between px-4 pt-3 pb-1">
            <p className="font-display text-[18px] font-semibold">Up next</p>
            <Button onClick={() => setQueueView(false)} className="ln-touch">Back to lyrics</Button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-6"><NativeQueue /></div>
        </div>
      )
  );
  if (phone) {
    const closePanel = () => useUi.setState({ npPanel: null });
    const openPanel = (next: "menu" | "queue" | "timer") => useUi.setState({ npPanel: next });
    const durationMs = s.durationMs || null;
    const episode = track.provider === "Podcast"; // songs only: likes, playlists, the song queue, album and artist pages
    const asQueued = { provider: "YouTube" as const, id: track.providerTrackId, title, artists, artworkUrl: artwork ?? null, durationMs };
    const addToEndOfQueue = async () => {
      closePanel();
      const queue = usePlayback.getState().autoplay.queue;
      const present = queue.some((t) => t.id === asQueued.id);
      try {
        await playerQueueEdit({ op: "next", track: asQueued });
        await playerQueueEdit({ op: "move", from: 0, to: present ? Math.max(0, queue.length - 1) : queue.length });
      } catch { /* the queue is unchanged */ }
    };
    const artistRef = info?.artistRefs?.[0] ?? track.artistRefs?.[0] ?? (artists[0] ? { provider: track.provider, id: null, name: artists[0] } : null);
    const album = info?.album;
    const row = "flex min-h-14 w-full items-center gap-4 rounded-[12px] px-3 text-left text-[16px] active:bg-frost-200 disabled:opacity-40";
    const minutesLeft = timer.endsAt ? formatRemaining(timer.endsAt - Date.now()) : null;
    const timerActive = timer.endsAt !== null || timer.endOfTrack !== null;
    const choose = (apply: () => void) => { apply(); closePanel(); };
    return (
      <dialog ref={dialog} tabIndex={-1} aria-label={`Now Playing: ${title}`}
        onCancel={(event) => { event.preventDefault(); backStep(); }}
        className="lx-np fixed inset-0 m-0 h-dvh max-h-none w-screen max-w-none overflow-hidden border-0 bg-transparent p-0 text-primary-text outline-none backdrop:bg-black/60">
        <AnimatePresence onExitComplete={() => dialog.current?.close()}>
          {open && (
            <motion.div key="sheet" initial={{ y: "100%" }} animate={{ y: 0 }} exit={{ y: "100%" }}
              transition={{ type: "spring", stiffness: 300, damping: 34, mass: 0.9 }}
              drag="y" dragControls={sheetDrag} dragListener={false} dragConstraints={{ top: 0, bottom: 0 }} dragElastic={{ top: 0, bottom: 0.55 }}
              onDragEnd={(_, info) => { if (info.offset.y > 130 || info.velocity.y > 600) useUi.getState().setNowPlaying(false); }}
              className={cx("ln-safe-top absolute inset-0 flex flex-col overflow-hidden bg-night-canvas shadow-[0_-12px_48px_rgba(0,0,0,0.55)]", !lyricsOpen && "rounded-t-[22px]")}>
              <div key={track.providerTrackId} aria-hidden="true" className="lx-ambient-in pointer-events-none absolute inset-0 overflow-hidden">
                <div data-playing={playing} className="ln-native-ambient absolute -inset-24 scale-125 opacity-45 blur-[90px] saturate-150 motion-reduce:animate-none">
                  <TrackCover url={artwork} videoId={track.providerTrackId} className="size-full rounded-none" />
                </div>
                <div className="ln-np-ambient-scrim absolute inset-0" />
              </div>
              {lyricsOpen ? (
                /* Lyrics keep their original full-screen layout and controls. */
                <div className="ln-safe-bottom ln-safe-x relative min-h-0 flex-1">
                  <IconButton label="Collapse Now Playing" onClick={() => useUi.getState().setNowPlaying(false)} className="ln-np-chrome absolute top-2 right-2 z-20 bg-black/30"><ChevronDown size={22} /></IconButton>
                  {stageEl}
                  {queueOverlay}
                </div>
              ) : (
                <div className="relative flex min-h-0 flex-1 flex-col overflow-y-auto overscroll-contain [scrollbar-width:none]">
                  <div className="sticky top-0 z-20 flex h-[60px] shrink-0 items-center pt-3">
                    <IconButton label="Collapse Now Playing" onClick={() => useUi.getState().setNowPlaying(false)} className="ml-2 size-11"><ChevronDown size={26} /></IconButton>
                    {/* Only this strip starts the swipe-down. */}
                    <div onPointerDown={(e) => sheetDrag.start(e)} className="flex h-full flex-1 touch-none items-center justify-center"><span aria-hidden="true" className="h-1.5 w-10 rounded-full bg-white/30" /></div>
                    <IconButton label="More options" onClick={() => openPanel("menu")} className="mr-2 size-11"><Menu size={24} /></IconButton>
                  </div>
                  <section style={{ minHeight: "calc(100% - 60px)" }} className="mx-auto flex w-full max-w-[460px] flex-col justify-center gap-6 px-6 pb-8">
                    {cover(cx("lx-fade-in aspect-square w-full rounded-[18px] shadow-[0_24px_70px_rgba(0,0,0,0.6)] transition-transform duration-[360ms] ease-out motion-reduce:transition-none", playing ? "scale-100" : "scale-[0.94]"))}
                    <div className="flex items-center gap-3">
                      <div className="min-w-0 flex-1">
                        <h2 className="ln-np-title truncate font-display text-[24px] leading-8 font-semibold tracking-[-0.02em]"><FxText key={title} text={title} effect={titleFx} playing={playing} /></h2>
                        <p className="truncate text-[15px] leading-5 text-secondary-text">{artistLine}</p>
                      </div>
                      <LikeButton track={likeTrack} />
                    </div>
                    <div className="flex flex-col gap-1">
                      <Scrubber label="Seek YouTube" max={Math.max(1, s.durationMs / 1000)} value={s.positionMs / 1000} disabled={!seekable} onCommit={(value) => void seekPlayback(value)} className="w-full" />
                      <div className="flex justify-between text-[12px] tabular-nums text-secondary-text"><span>{formatDuration(s.positionMs / 1000)}</span><span>{formatDuration(s.durationMs / 1000)}</span></div>
                    </div>
                    <div className="flex items-center justify-between px-2">
                      <RepeatButton className="size-11" />
                      <IconButton label="Previous track" disabled={s.busy || !seekable} onClick={() => void playerSkip(false)} className="size-12"><SkipBack size={26} fill="currentColor" /></IconButton>
                      <button type="button" aria-label={playing ? "Pause" : "Play"} disabled={s.busy} onClick={() => void togglePlayback()}
                        className="flex size-[68px] items-center justify-center rounded-full bg-primary-text text-night-canvas shadow-[0_8px_28px_rgba(0,0,0,0.45)] transition-transform duration-150 active:scale-95 disabled:opacity-50">
                        {playing ? <Pause size={30} fill="currentColor" /> : <Play size={30} fill="currentColor" className="translate-x-[2px]" />}
                      </button>
                      <IconButton label="Next track" disabled={s.busy || !seekable} onClick={() => void playerSkip(true)} className="size-12"><SkipForward size={26} fill="currentColor" /></IconButton>
                      <AutoplayButton className="size-11" />
                    </div>
                    <div className="flex items-center justify-center gap-10">
                      <IconButton label="Queue" onClick={() => openPanel("queue")} className="size-11"><ListMusic size={22} /></IconButton>
                      <IconButton label="Show lyrics" onClick={() => useUi.getState().setLyrics(true)} className="size-11"><MicVocal size={22} /></IconButton>
                    </div>
                    {s.error && <p role="alert" className="text-center text-[12px] text-critical-coral">{s.error}</p>}
                  </section>
                </div>
              )}

              <Drawer open={panel === "menu"} onClose={closePanel} title={menuView === "menu" ? "More options" : "Add to playlist"}>
                {menuView === "menu" ? (
                  <ul className="flex flex-col">
                    {!episode && <li><button type="button" className={row} onClick={() => { closePanel(); void toggleSaved(likeTrack); }}>{likedNow ? <HeartOff size={22} aria-hidden="true" /> : <Heart size={22} aria-hidden="true" />}{likedNow ? "Remove from liked songs" : "Add to liked songs"}</button></li>}
                    {!episode && <li><button type="button" className={row} onClick={() => setMenuView("playlists")}><ListPlus size={22} aria-hidden="true" />Add to playlist</button></li>}
                    {!episode && <li><button type="button" className={row} onClick={() => void addToEndOfQueue()}><ListEnd size={22} aria-hidden="true" />Add to queue</button></li>}
                    <li><button type="button" className={row} onClick={() => openPanel("queue")}><ListMusic size={22} aria-hidden="true" />Go to queue</button></li>
                    {!episode && <li><button type="button" className={row} disabled={!album?.id} onClick={() => { if (!album?.id) return; closePanel(); useUi.getState().openAlbum({ provider: "YouTube", id: album.id, name: album.name, artworkUrl: artwork ?? null, year: null }); useUi.getState().setNowPlaying(false); }}><Disc3 size={22} aria-hidden="true" /><span className="min-w-0"><span className="block">Go to album</span>{album ? <span className="block truncate text-[13px] text-secondary-text">{album.id ? album.name : "No album page for this song"}</span> : null}</span></button></li>}
                    {!episode && <li><button type="button" className={row} disabled={!artistRef} onClick={() => { if (!artistRef) return; closePanel(); useUi.getState().openArtist(artistRef); }}><User size={22} aria-hidden="true" />Go to artist</button></li>}
                    <li><button type="button" className={row} onClick={() => openPanel("timer")}><Moon size={22} aria-hidden="true" /><span className="min-w-0"><span className="block">Sleep timer</span>{timerActive ? <span className="block text-[13px] text-lumen-lavender">{minutesLeft ? `Pauses in ${minutesLeft}` : "Pauses when this song ends"}</span> : null}</span></button></li>
                  </ul>
                ) : (
                  <div className="flex flex-col">
                    <button type="button" className={row} onClick={() => setMenuView("menu")}><ChevronLeft size={22} aria-hidden="true" />Back</button>
                    {playlists === null ? <p role="status" className="px-3 py-4 text-[14px] text-secondary-text">Loading playlists…</p>
                      : playlists.length === 0 ? <p className="px-3 py-4 text-[14px] text-secondary-text">You have no playlists yet. Create one from Library.</p>
                      : <ul>{playlists.map((p) => <li key={p.id}><button type="button" className={row} onClick={() => { closePanel(); void addToPlaylist(p.id, likeTrack).catch(() => {}); }}><ListPlus size={22} aria-hidden="true" /><span className="min-w-0 truncate">{p.name}</span></button></li>)}</ul>}
                  </div>
                )}
              </Drawer>

              <Drawer open={panel === "queue"} onClose={closePanel} title="Up next" height="60"
                footer={<div className="grid grid-cols-2 gap-3">
                  <Button className="ln-touch justify-center" onClick={() => void playerQueueEdit({ op: "shuffle" }).catch(() => {})}><Shuffle size={18} aria-hidden="true" />Shuffle</Button>
                  <Button className="ln-touch justify-center" onClick={() => openPanel("timer")}><Timer size={18} aria-hidden="true" />{timerActive ? (minutesLeft ?? "End of song") : "Timer"}</Button>
                </div>}>
                <NativeQueue bare />
              </Drawer>

              <Drawer open={panel === "timer"} onClose={closePanel} title="Sleep timer" height="60" z="z-50">
                {timerActive && <p role="status" className="px-3 pb-2 text-[14px] text-lumen-lavender">{minutesLeft ? `Pauses in ${minutesLeft}` : "Pauses when this song ends"}</p>}
                <ul className="flex flex-col">
                  <li><button type="button" className={row} onClick={() => choose(setSleepEndOfTrack)}><span className="flex-1">End of song</span>{timer.endOfTrack ? <Check size={20} aria-hidden="true" /> : null}</button></li>
                  {[5, 10, 15, 30, 45, 60].map((m) => (
                    <li key={m}><button type="button" className={row} onClick={() => choose(() => setSleepTimer(m))}><span className="flex-1">{m === 60 ? "1 hour" : `${m} minutes`}</span></button></li>
                  ))}
                  {timerActive && <li><button type="button" className={cx(row, "text-critical-coral")} onClick={() => choose(() => setSleepTimer(null))}>Turn off timer</button></li>}
                </ul>
              </Drawer>
            </motion.div>
          )}
        </AnimatePresence>
      </dialog>
    );
  }
  return (
    <dialog ref={dialog} data-np-layout={full ? "full" : lyricsOpen ? "split" : "page"} tabIndex={-1} aria-label={`Now Playing: ${title}`} onCancel={(event) => { event.preventDefault(); if (queueView) setQueueView(false); else if (maker) setMaker(false); else useUi.getState().setNowPlaying(false); }}
      onKeyDown={(event) => {
        if (event.key.toLowerCase() !== "f" || event.metaKey || event.ctrlKey || event.altKey || isTextTarget(event.target) || !fullscreen.supported) return;
        event.preventDefault();
        fullscreen.toggle();
      }}
      className="lx-np ln-safe-top ln-safe-bottom ln-safe-x outline-none fixed inset-0 m-0 h-dvh max-h-none w-screen max-w-none overflow-hidden border-0 bg-night-canvas p-0 text-primary-text backdrop:bg-black/80">
      <div key={track.providerTrackId} aria-hidden="true" className="lx-ambient-in pointer-events-none absolute inset-0 overflow-hidden">
        <div data-playing={playing} className="ln-native-ambient absolute -inset-24 scale-125 opacity-45 blur-[90px] saturate-150 motion-reduce:animate-none">
          <TrackCover url={artwork} videoId={track.providerTrackId} className="size-full rounded-none" />
        </div>
        <div className="ln-np-ambient-scrim absolute inset-0" />
      </div>
      {phone && (
        <IconButton label="Collapse Now Playing" onClick={() => useUi.getState().setNowPlaying(false)} className="ln-np-chrome absolute top-2 right-2 z-20 bg-black/30">
          <ChevronDown size={22} />
        </IconButton>
      )}
      {stageEl}
      {queueView && (
        <div role="region" aria-label="Queue" className="absolute inset-0 z-30 flex flex-col bg-night-canvas/95 backdrop-blur-xl">
          <div className="flex shrink-0 items-center justify-between px-4 pt-3 pb-1">
            <p className="font-display text-[18px] font-semibold">Up next</p>
            <Button onClick={() => setQueueView(false)} className="ln-touch">Back to lyrics</Button>
          </div>
          <div className="min-h-0 flex-1 overflow-y-auto px-3 pb-6"><NativeQueue /></div>
        </div>
      )}
      {maker && <LyricMaker key={track.providerTrackId} trackId={track.providerTrackId} inset={100} onClose={() => { setMaker(false); dialog.current?.focus(); }} />}
    </dialog>
  );
}
