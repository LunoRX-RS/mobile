"use client";

import { create } from "zustand";
import { statsLocal } from "../services/tauri/stats";
import {
  remoteSend, remoteStart, remoteStop, subscribeRemoteMessages, subscribeRemoteStatus, type LinkStatus,
} from "../services/tauri/remote";
import { playerSkip } from "../services/tauri/player";
import { ACCOUNT_CHANGED } from "../services/tauri/sync";
import {
  playTrackHere, seekPlayback, setPlayRedirect, setPlaybackVolume, togglePlayback, usePlayback,
} from "./playback";

// Cross-device remote control. This device publishes its player state and obeys commands from the account's other
// devices; it can also pick one of them as `target` and drive it. Frames follow backend docs/remote-control.md.

export interface RemoteTrack { provider: string; providerTrackId: string; title: string; artists: string[]; durationMs: number }
export interface RemotePlayerState {
  status: "playing" | "paused" | "buffering" | "stopped";
  track: RemoteTrack | null;
  positionMs: number;
  volume: number | null;
  /** When this client received the state: position is extrapolated from here. */
  at: number;
}
export interface RemoteDevice { deviceId: string; name: string; kind: "computer" | "phone"; online: boolean; state: RemotePlayerState | null }

interface RemoteUi {
  enabled: boolean;
  link: LinkStatus | "off";
  self: string | null;
  devices: Record<string, RemoteDevice>;
  /** The device this one is driving, or null when it plays on its own. */
  target: string | null;
  notice: string | null;
  setEnabled: (on: boolean) => void;
  setTarget: (id: string | null) => void;
}

const KEY = "lunorx.remote";
function readEnabled(): boolean {
  try { return window.localStorage.getItem(KEY) === "on"; } catch { return false; }
}

export const useRemote = create<RemoteUi>((set) => ({
  enabled: false, link: "off", self: null, devices: {}, target: null, notice: null,
  setEnabled: (enabled) => {
    try { window.localStorage.setItem(KEY, enabled ? "on" : "off"); } catch { /* The choice lasts until restart. */ }
    set(enabled ? { enabled } : { enabled, target: null, devices: {}, link: "off" });
  },
  setTarget: (target) => set({ target, notice: null }),
}));

/** Position of a remote device now: playing devices keep moving since the last state arrived. */
export function remotePosition(state: RemotePlayerState, now = Date.now()): number {
  const moved = state.status === "playing" ? now - state.at : 0;
  const end = state.track?.durationMs || Infinity;
  return Math.min(end, state.positionMs + moved);
}

export type RemoteCommand =
  | { action: "play" | "pause" | "toggle" | "next" | "previous" }
  | { action: "seek"; positionMs: number }
  | { action: "volume"; percent: number }
  | { action: "playTrack"; provider: string; providerTrackId: string; title?: string; artists?: string[] };

export function sendRemoteCommand(command: RemoteCommand) {
  const to = useRemote.getState().target;
  if (to) void remoteSend({ type: "command", to, command }).catch(() => {});
}

const YOUTUBE_ID = /^[A-Za-z0-9_-]{11}$/;

async function apply(command: RemoteCommand) {
  const { player } = usePlayback.getState();
  const playing = player.state === "playing" || player.state === "buffering";
  switch (command.action) {
    case "play": if (!playing) await togglePlayback(); break;
    case "pause": if (playing) await togglePlayback(); break;
    case "toggle": await togglePlayback(); break;
    case "next": await playerSkip(true); break;
    case "previous": await playerSkip(false); break;
    case "seek": await seekPlayback(command.positionMs / 1000); break;
    case "volume": await setPlaybackVolume(command.percent); break;
    case "playTrack":
      // Only YouTube plays natively; any other provider or a malformed id is ignored.
      if (command.provider !== "YouTube" || !YOUTUBE_ID.test(command.providerTrackId)) break;
      await playTrackHere({
        provider: "YouTube", providerTrackId: command.providerTrackId, title: command.title ?? "",
        artists: command.artists ?? [], canonicalUrl: `https://www.youtube.com/watch?v=${command.providerTrackId}`,
      });
      break;
  }
}

function isCommand(value: unknown): value is RemoteCommand {
  const c = value as { action?: unknown } | null;
  return typeof c?.action === "string";
}

const STATUSES = ["playing", "paused", "buffering", "stopped"];
const num = (v: unknown, max: number) => (typeof v === "number" && Number.isFinite(v) ? Math.min(max, Math.max(0, v)) : 0);
const clip = (v: unknown, max: number) => (typeof v === "string" ? v.slice(0, max) : "");

// Frames from other devices are data, not trusted: every field is re-checked before it reaches the UI.
function toState(raw: unknown): RemotePlayerState | null {
  const s = raw as Record<string, unknown> | null;
  if (!s || typeof s.status !== "string" || !STATUSES.includes(s.status)) return null;
  const t = s.track as Record<string, unknown> | null;
  const track: RemoteTrack | null = t && typeof t.providerTrackId === "string" && YOUTUBE_ID.test(t.providerTrackId)
    ? {
        provider: "YouTube", providerTrackId: t.providerTrackId, title: clip(t.title, 200),
        artists: Array.isArray(t.artists) ? t.artists.slice(0, 5).map((a) => clip(a, 100)) : [],
        durationMs: num(t.durationMs, 86_400_000),
      }
    : null;
  return {
    status: s.status as RemotePlayerState["status"], track, positionMs: num(s.positionMs, 86_400_000),
    volume: typeof s.volume === "number" ? num(s.volume, 100) : null, at: Date.now(),
  };
}

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const kindOf = (v: unknown): RemoteDevice["kind"] => (v === "phone" ? "phone" : "computer");

function handle(text: string) {
  let msg: Record<string, unknown>;
  try { msg = JSON.parse(text) as Record<string, unknown>; } catch { return; }
  const set = useRemote.setState;
  const devices = { ...useRemote.getState().devices };
  switch (msg.type) {
    case "devices": {
      const next: Record<string, RemoteDevice> = {};
      const list = Array.isArray(msg.devices) ? (msg.devices as Record<string, unknown>[]) : [];
      for (const d of list.slice(0, 20)) {
        const id = d.deviceId;
        if (typeof id !== "string" || !UUID.test(id) || id === useRemote.getState().self) continue;
        next[id] = { deviceId: id, name: clip(d.name, 64), kind: kindOf(d.kind), online: d.online === true, state: toState(d.state) };
      }
      set({ devices: next });
      break;
    }
    case "presence": {
      const id = msg.deviceId;
      if (typeof id !== "string" || !UUID.test(id) || id === useRemote.getState().self) break;
      const old = devices[id];
      devices[id] = { deviceId: id, name: clip(msg.name, 64), kind: kindOf(msg.kind), online: msg.online === true, state: msg.online === true ? old?.state ?? null : null };
      set({ devices });
      break;
    }
    case "state": {
      const id = typeof msg.deviceId === "string" ? msg.deviceId : "";
      if (devices[id]) set({ devices: { ...devices, [id]: { ...devices[id], online: true, state: toState(msg.state) } } });
      break;
    }
    case "deviceRemoved": {
      delete devices[typeof msg.deviceId === "string" ? msg.deviceId : ""];
      set({ devices, target: useRemote.getState().target === msg.deviceId ? null : useRemote.getState().target });
      break;
    }
    case "command":
      if (isCommand(msg.command)) void apply(msg.command).catch(() => {});
      break;
    case "error":
      set({ notice: msg.code === "DEVICE_OFFLINE" ? "That device is offline." : "The command was not accepted." });
      break;
  }
}

function deviceName(id: string): string {
  const ua = navigator.userAgent;
  const os = /Mac/.test(ua) ? "Mac" : /Windows/.test(ua) ? "Windows PC" : /Android/.test(ua) ? "Android" : /iPhone|iPad/.test(ua) ? "iPhone" : "Linux PC";
  return `LunoRX · ${os} · ${id.slice(0, 4)}`;
}

function snapshot() {
  const s = usePlayback.getState();
  const t = s.requested ?? s.track;
  const info = s.info?.providerTrackId === t?.providerTrackId ? s.info : null;
  const status = s.player.state === "playing" ? "playing"
    : s.player.state === "paused" ? "paused"
    : ["buffering", "resolving", "preparing"].includes(s.player.state) ? "buffering" : "stopped";
  return {
    status,
    positionMs: Math.max(0, Math.round(s.positionMs)),
    volume: Math.min(100, Math.max(0, Math.round(s.volume))),
    track: t && YOUTUBE_ID.test(t.providerTrackId) ? {
      provider: "YouTube", providerTrackId: t.providerTrackId,
      title: (info?.title ?? t.title ?? "").slice(0, 200),
      artists: (info?.artists ?? t.artists ?? []).slice(0, 5).map((a) => a.slice(0, 100)),
      durationMs: Math.min(86_400_000, Math.round(s.durationMs)),
    } : null,
  };
}

/** Opens the link and keeps this device's state published. Returns the cleanup. */
function connect(): () => void {
  let active = true;
  const cleanups: (() => void)[] = [];
  const publish = () => { void remoteSend({ type: "state", state: snapshot() }).catch(() => {}); };
  let debounce: number | undefined;
  const soon = () => { window.clearTimeout(debounce); debounce = window.setTimeout(publish, 400); };
  void (async () => {
    try {
      const { deviceId } = await statsLocal();
      if (!active) return;
      useRemote.setState({ self: deviceId });
      const stops = await Promise.all([
        subscribeRemoteMessages(handle),
        subscribeRemoteStatus((link) => {
          useRemote.setState({ link });
          if (link === "online") publish();
          // Never show devices of a link that is gone or of the previous account.
          else useRemote.setState({ devices: {}, target: null });
        }),
      ]);
      if (!active) { stops.forEach((stop) => stop()); return; }
      cleanups.push(...stops);
      cleanups.push(usePlayback.subscribe((s, prev) => {
        const id = (x: typeof s) => (x.requested ?? x.track)?.providerTrackId;
        if (s.player.state !== prev.player.state || id(s) !== id(prev) || s.volume !== prev.volume) soon();
      }));
      const beat = window.setInterval(() => { if (usePlayback.getState().player.state === "playing") publish(); }, 5000);
      cleanups.push(() => window.clearInterval(beat));
      await remoteStart(deviceId, deviceName(deviceId));
    } catch { useRemote.setState({ link: "offline" }); }
  })();
  return () => {
    active = false;
    window.clearTimeout(debounce);
    cleanups.forEach((stop) => stop());
    void remoteStop().catch(() => {});
  };
}

/** Call once on mount: follows the on/off choice and account changes, and routes picked tracks to the target. */
export function initRemote(): () => void {
  useRemote.setState({ enabled: readEnabled() });
  setPlayRedirect((track) => {
    const target = useRemote.getState().target;
    if (!target) return false;
    sendRemoteCommand({ action: "playTrack", provider: track.provider, providerTrackId: track.providerTrackId, title: track.title, artists: track.artists });
    return true;
  });
  let stop: (() => void) | null = null;
  const sync = () => {
    stop?.();
    stop = useRemote.getState().enabled ? connect() : null;
  };
  let wasEnabled = useRemote.getState().enabled;
  sync();
  const unsubscribe = useRemote.subscribe((s) => {
    if (s.enabled !== wasEnabled) { wasEnabled = s.enabled; sync(); }
  });
  // A new sign-in needs a fresh link; a sign-out ends the old one when its token disappears.
  window.addEventListener(ACCOUNT_CHANGED, sync);
  return () => {
    unsubscribe();
    window.removeEventListener(ACCOUNT_CHANGED, sync);
    setPlayRedirect(null);
    stop?.();
  };
}
