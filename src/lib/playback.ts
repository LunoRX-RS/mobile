"use client";

import { create } from "zustand";
import { prefetchImages } from "./prefetch";
import {
  playerCommand, playerLoad, playerSeek, playerSetVolume,
  type LoadedTrack, type PlayerProgressDto, type PlayerStateDto, type TrackInfo,
} from "../services/tauri/player";
import { useUi } from "./store";

interface PlaybackUi {
  info: TrackInfo | null;
  infoState: "idle" | "loading" | "ready" | "error";
  player: PlayerStateDto;
  track: LoadedTrack | null;
  requested: LoadedTrack | null;
  positionMs: number;
  durationMs: number;
  volume: number;
  autoplay: import("../services/tauri/player").PlayerProgressDto["autoplay"];
  available: boolean;
  busy: boolean;
  error: string | null;
  /** Last track from a previous session. Shown paused; nothing loads until the user presses play. */
  resume: LastPlayed | null;
  acceptState: (player: PlayerStateDto) => void;
  acceptProgress: (progress: PlayerProgressDto) => void;
}

export interface LastPlayed {
  track: LoadedTrack;
  positionMs: number;
  durationMs: number;
}

const AUDIO_FAILED = "Audio playback failed. Try the track again or open YouTube.";
const DEV_MODE = process.env.NODE_ENV === 'development';

// A view of Core state. This store does not advance playback or own a queue.
export const usePlayback = create<PlaybackUi>((set, get) => ({
  info: null, infoState: "idle",
  player: { version: 1, state: "idle", revision: 0 },
  track: null, requested: null, positionMs: 0, durationMs: 0, volume: 100,
  autoplay: { state: "", nextTitle: null, queue: [] },
  available: false, busy: false, error: null, resume: null,
  acceptState: (player) => {
    if (player.revision >= get().player.revision) {
      set({ player, error: player.state === "error" ? get().error ?? AUDIO_FAILED : get().error });
    }
  },
  acceptProgress: (progress) => {
    prefetchImages(progress.autoplay.queue.slice(0, 6).map((t) => t.artworkUrl));
    if (progress.player.revision >= get().player.revision) {
      // While the slider is being dragged the UI value is newer than core's.
      const volume = volumeInFlight || pendingVolume !== null ? get().volume : progress.volume;
      const changed = !get().busy && progress.track && progress.track.providerTrackId !== (get().requested ?? get().track)?.providerTrackId;
      set({ ...progress, ...(changed ? { requested: null, info: null, infoState: "loading" as const } : {}), volume, error: progress.player.state === "error" ? get().error ?? AUDIO_FAILED : get().error });
    }
  },
}));

function errorText(error: unknown): string {
  const e = error as { category?: string; message?: string } | null;
  const category = e?.category;
  const debug = e?.message ?? "";
  if (category === "storageInUse") return "Media storage is open in another LUNOR-X instance. Close it and restart this app.";
  if (category === "storageUnavailable") return "Media storage could not be opened. Check disk access and free space, then restart this app.";
  if (category === "unsupportedCapability") return "Native playback or media storage is unavailable. Check libmpv and local storage access.";
  if (category === "timeout") return "YouTube or the audio player took too long. Try the track again.";
  if (category === "verificationRequired") return "YouTube asked for verification. Sign in with Google in Connections.";
  if (category === "mediaRefused") return `YouTube refused the audio bytes for this track. Sign in with Google in Connections, then try again.${DEV_MODE ? ` [debug: ${debug}]` : ""}`;
  if (category === "validation") return "This track has an invalid YouTube ID.";
  if (category === "hostRejected") {
    if (debug.includes("age") || debug.includes("Age")) return `This video is age-restricted. Sign in with Google to verify your age.${DEV_MODE ? ` [debug: ${debug}]` : ""}`;
    if (debug.includes("region") || debug.includes("Region")) return `This video is not available in your region.${DEV_MODE ? ` [debug: ${debug}]` : ""}`;
    if (debug.includes("sign.in") || debug.includes("verify")) return `YouTube asked for verification. Sign in with Google in Connections.${DEV_MODE ? ` [debug: ${debug}]` : ""}`;
    return `The audio player refused this track. Try another track.${DEV_MODE ? ` [debug: ${debug}]` : ""}`;
  }
  return `Could not load this track. Try again.${DEV_MODE ? ` [debug: ${JSON.stringify(error)}]` : ""}`;
}

async function run(action: () => Promise<PlayerStateDto>) {
  if (usePlayback.getState().busy) return;
  usePlayback.setState({ busy: true, error: null });
  try { usePlayback.getState().acceptState(await action()); }
  catch (error) { usePlayback.setState({ error: errorText(error) }); }
  finally { usePlayback.setState({ busy: false }); }
}

// While this device controls another one, a track the user picks plays there. The redirect returns true when it took the track.
let redirect: ((track: LoadedTrack) => boolean) | null = null;
export function setPlayRedirect(fn: ((track: LoadedTrack) => boolean) | null) { redirect = fn; }

export async function playTrack(track: LoadedTrack, fromQueue = false, following: LoadedTrack[] = []) {
  if (redirect?.(track)) return;
  return playTrackHere(track, fromQueue, following);
}

/** Always plays on this device, even while controlling another. */
export async function playTrackHere(track: LoadedTrack, fromQueue = false, following: LoadedTrack[] = []) {
  if (usePlayback.getState().busy) return;
  useUi.setState({ playing: false });
  usePlayback.setState({ requested: { ...track }, positionMs: 0, durationMs: 0, resume: null });
  await run(async () => {
    usePlayback.getState().acceptState(await playerLoad(track, fromQueue, following));
    // Core refuses volume until a track is loaded, so the saved level is sent here, before play.
    try { usePlayback.getState().acceptState(await playerSetVolume(usePlayback.getState().volume)); }
    catch { /* Playback still starts; the slider is resent on the next change. */ }
    return playerCommand("play");
  });
}

export function togglePlayback() {
  const { player, track, requested } = usePlayback.getState();
  if (player.state === "ended" || player.state === "error" || player.state === "blocked") {
    const retry = requested ?? track;
    if (retry) return playTrack(retry);
    return;
  }
  return run(() => playerCommand(player.state === "playing" || player.state === "buffering"
    ? "pause" : player.state === "paused" ? "resume" : "play"));
}

export function stopPlayback() {
  return run(async () => {
    const state = await playerCommand("stop");
    usePlayback.setState({ requested: null, track: null, positionMs: 0, durationMs: 0 });
    writeLast(null);
    return state;
  });
}

// Seeking is a drag or a tap on a line: show the new position at once and send only the latest target,
// one request at a time. It must not take the busy lock: that dropped taps and disabled the bar mid-drag.
let seekInFlight = false;
let pendingSeek: number | null = null;

export function seekPlayback(seconds: number) {
  const target = Math.max(0, seconds);
  pendingSeek = target;
  usePlayback.setState({ positionMs: Math.round(target * 1000) });
  if (seekInFlight) return Promise.resolve();
  seekInFlight = true;
  return (async () => {
    try {
      while (pendingSeek !== null) {
        const value = pendingSeek;
        pendingSeek = null;
        try { usePlayback.getState().acceptState(await playerSeek(value * 1000)); }
        catch (error) { pendingSeek = null; usePlayback.setState({ error: errorText(error) }); }
      }
    } finally {
      seekInFlight = false;
    }
  })();
}

const VOLUME_KEY = "lunorx.volume";
function readVolume(): number | null {
  try {
    const raw = window.localStorage.getItem(VOLUME_KEY);
    const n = raw === null ? NaN : Number(raw);
    return Number.isFinite(n) && n >= 0 && n <= 100 ? Math.round(n) : null;
  } catch {
    return null;
  }
}
function writeVolume(value: number) {
  try { window.localStorage.setItem(VOLUME_KEY, String(value)); }
  catch { /* Not saved: volume resets to 100 on next start. */ }
}

// Volume is a drag: update the UI at once and send only the latest value, one request at a time.
// It must not take the busy lock; that disabled the slider mid-drag and dropped later values.
let volumeInFlight = false;
let pendingVolume: number | null = null;

export function setPlaybackVolume(volume: number) {
  const next = Math.round(Math.min(100, Math.max(0, volume)));
  usePlayback.setState({ volume: next });
  writeVolume(next);
  pendingVolume = next;
  if (volumeInFlight) return Promise.resolve();
  volumeInFlight = true;
  return (async () => {
    try {
      while (pendingVolume !== null) {
        const value = pendingVolume;
        pendingVolume = null;
        try { usePlayback.getState().acceptState(await playerSetVolume(value)); }
        catch (error) { pendingVolume = null; usePlayback.setState({ error: errorText(error) }); }
      }
    } finally {
      volumeInFlight = false;
    }
  })();
}


// ---- Last played: remember the track and the minute across app restarts. ----
// localStorage can throw or be absent (blocked site data), so every access is guarded.
// Only the track reference and position are stored. No audio, no stream URL, no credentials.
const LAST_KEY = "lunorx.lastPlayed";
const SAVE_EVERY_MS = 2000;

function isLastPlayed(value: unknown): value is LastPlayed {
  const v = value as Partial<LastPlayed> | null;
  const t = v?.track;
  return (
    typeof t?.providerTrackId === "string" && /^[A-Za-z0-9_-]{11}$/.test(t.providerTrackId) &&
    typeof t.title === "string" && typeof t.provider === "string" && typeof t.canonicalUrl === "string" &&
    typeof v?.positionMs === "number" && Number.isFinite(v.positionMs) && v.positionMs >= 0 &&
    typeof v?.durationMs === "number" && Number.isFinite(v.durationMs) && v.durationMs >= 0
  );
}

function readLast(): LastPlayed | null {
  try {
    const raw = window.localStorage.getItem(LAST_KEY);
    const parsed: unknown = raw ? JSON.parse(raw) : null;
    return isLastPlayed(parsed) ? parsed : null;
  } catch {
    return null;
  }
}

function writeLast(value: LastPlayed | null) {
  try {
    if (value) window.localStorage.setItem(LAST_KEY, JSON.stringify(value));
    else window.localStorage.removeItem(LAST_KEY);
  } catch {
    // Not saved: the app still works, it just will not remember the position.
  }
}

/** Call once on mount. Restores the saved track as a paused 'resume' card and keeps it saved while playing. */
export function initLastPlayed(): () => void {
  const savedVolume = readVolume();
  if (savedVolume !== null) usePlayback.setState({ volume: savedVolume });
  const first = usePlayback.getState();
  if (!first.track && !first.requested) {
    const saved = readLast();
    if (saved) usePlayback.setState({ resume: saved });
  }
  let lastWrite = 0;
  const save = (force: boolean) => {
    const s = usePlayback.getState();
    const track = s.track ?? s.requested;
    if (!track || s.player.state === "idle" || s.player.state === "ended") return;
    const now = Date.now();
    if (!force && now - lastWrite < SAVE_EVERY_MS) return;
    lastWrite = now;
    writeLast({ track, positionMs: Math.round(s.positionMs), durationMs: Math.round(s.durationMs) });
  };
  const unsubscribe = usePlayback.subscribe((s) => {
    if ((s.track || s.requested) && s.resume) usePlayback.setState({ resume: null });
    if (s.player.state === "ended") { writeLast(null); return; }
    save(false);
  });
  const onHide = () => save(true);
  window.addEventListener("pagehide", onHide);
  document.addEventListener("visibilitychange", onHide);
  return () => {
    unsubscribe();
    window.removeEventListener("pagehide", onHide);
    document.removeEventListener("visibilitychange", onHide);
  };
}

export function clearResume() {
  usePlayback.setState({ resume: null });
  writeLast(null);
}

/** Load the saved track, then seek to the saved minute. Nothing plays until the user asks. */
export async function resumeLast() {
  const saved = usePlayback.getState().resume;
  if (!saved || usePlayback.getState().busy) return;
  await playTrack(saved.track);
  if (saved.positionMs > 1000 && usePlayback.getState().player.state === "playing") {
    await seekPlayback(saved.positionMs / 1000);
  }
}
