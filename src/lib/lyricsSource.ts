"use client";

import { useSyncExternalStore } from "react";
import type { LyricLineDto, LyricsSourceDto, TrackInfo } from "../services/tauri/player";
import { hasWordTiming } from "./lyricsTiming";

/** "auto" or a source id. Ids come from core, so any well-formed id is accepted. */
export type LyricsSourcePref = string;
export const LYRICS_SOURCE_KEY = "lunorx.lyricsSource";

const isPref = (v: unknown): v is LyricsSourcePref => typeof v === "string" && /^[A-Za-z0-9_-]{1,32}$/.test(v);

function read(): LyricsSourcePref {
  try { const v = window.localStorage.getItem(LYRICS_SOURCE_KEY); return isPref(v) ? v : "auto"; } catch { return "auto"; }
}

// Module-level store: every component and the Settings select read the same value, and other tabs arrive by the storage event.
let current: LyricsSourcePref | null = null;
const listeners = new Set<() => void>();
const get = () => (current ??= read());
const emit = () => listeners.forEach((l) => l());

export function setLyricsSource(next: LyricsSourcePref): void {
  current = next;
  try { window.localStorage.setItem(LYRICS_SOURCE_KEY, next); } catch { /* storage unavailable: the choice lasts for this session */ }
  emit();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  const onStorage = (event: StorageEvent) => {
    if (event.key === LYRICS_SOURCE_KEY || event.key === null) { current = read(); emit(); }
  };
  window.addEventListener("storage", onStorage);
  return () => { listeners.delete(listener); window.removeEventListener("storage", onStorage); };
}

export function useLyricsSource(): [LyricsSourcePref, (next: LyricsSourcePref) => void] {
  const pref = useSyncExternalStore(subscribe, get, () => "auto" as const);
  return [pref, setLyricsSource];
}

/** Sources as core lists them. Older payloads without `lyricsSources` are rebuilt from the legacy fields. Entries without an id are dropped. */
export function sourcesOf(info: TrackInfo): LyricsSourceDto[] {
  if (Array.isArray(info.lyricsSources) && info.lyricsSources.length > 0) return info.lyricsSources.filter((s) => typeof s?.id === "string" && !!s.id);
  const timed = info.lyricsSynced && info.lyricsLines && info.lyricsLines.length > 0 ? info.lyricsLines : null;
  const yt = info.lyricsProvider === "YouTube Music" && !!info.lyrics;
  const lr = info.lyricsProvider === "LRCLIB" && (!!info.lyrics || !!timed);
  const none = (id: string, label: string, status: LyricsSourceDto["status"]): LyricsSourceDto =>
    ({ id, label, available: false, text: null, lines: null, synced: false, attribution: null, status });
  return [
    yt ? { id: "youtubeMusic", label: "YouTube Music", available: true, text: info.lyrics, lines: null, synced: false, attribution: info.lyricsSource, status: "available" }
      : none("youtubeMusic", "YouTube Music", info.lyricsProvider ? "unavailable" : info.lyricsStatus),
    lr ? { id: "lrclib", label: "LRCLIB", available: true, text: info.lyrics ?? timed?.map((l) => l.text).join("\n") ?? null, lines: timed, synced: !!timed, attribution: info.lyricsSource, status: "available" }
      : none("lrclib", "LRCLIB", info.lyricsProvider ? "unavailable" : info.lyricsStatus),
  ];
}

export const labelOf = (s: Pick<LyricsSourceDto, "id" | "label">): string => s.label || s.id;

/** Timed lines to follow, or null for plain text. */
export const timedOf = (s: LyricsSourceDto) => (s.synced && s.lines && s.lines.length > 0 ? s.lines : null);
/** True only when the lines carry real word timings. */
export const wordTimedOf = (s: LyricsSourceDto) => hasWordTiming(timedOf(s));
const hasText = (s: LyricsSourceDto | undefined): boolean => !!s && s.available && (!!s.text || !!timedOf(s));
/** Real word timings beat line timings beat plain text. */
const rank = (s: LyricsSourceDto) => (wordTimedOf(s) ? 3 : timedOf(s) ? 2 : 1);

/** My LRC first, then the user's Auto order; sources not in the order keep the order they arrived in. */
export function orderSources(sources: readonly LyricsSourceDto[], order: readonly string[]): LyricsSourceDto[] {
  const at = (s: LyricsSourceDto) => { const i = order.indexOf(s.id); return i < 0 ? order.length : i; };
  return sources.map((s, n) => ({ s, n })).sort((a, b) => (+(b.s.id === "mine") - +(a.s.id === "mine")) || at(a.s) - at(b.s) || a.n - b.n).map((x) => x.s);
}

export interface LyricsChoice {
  source: LyricsSourceDto | null;
  /** What Auto picks, whatever the preference is. */
  auto: LyricsSourceDto | null;
  /** Timed lines to follow, or null for plain text. */
  lines: readonly LyricLineDto[] | null;
  /** Shown when the preferred source could not be used. Never silent. */
  note: string | null;
}

/** Auto: My LRC first, then the best of the user's order: real word timing, then line timing, then plain text. A specific choice wins when available; otherwise Auto plus a note. */
export function chooseLyrics(pref: LyricsSourcePref, sources: readonly LyricsSourceDto[], order: readonly string[] = []): LyricsChoice {
  const ranked = orderSources(sources, order).filter(hasText);
  const mine = ranked.find((s) => s.id === "mine" && timedOf(s));
  const auto = mine ?? ranked.reduce<LyricsSourceDto | null>((best, s) => (!best || rank(s) > rank(best) ? s : best), null);
  const wanted = pref === "auto" ? undefined : sources.find((s) => s.id === pref);
  if (wanted && hasText(wanted)) return { source: wanted, auto, lines: timedOf(wanted), note: null };
  let note: string | null = null;
  if (pref !== "auto" && auto) {
    const name = wanted ? labelOf(wanted) : pref === "mine" ? "My LRC" : "That source";
    const why = pref === "mine" ? "You have not made lyrics for this track yet" : wanted?.status === "failed" || wanted?.status === "unreachable" ? `${name} could not be loaded for this track`
      : wanted?.status === "disabled" ? `${name} is turned off` : wanted?.status === "instrumental" ? `${name} lists this track as instrumental` : `${name} had no lyrics for this track`;
    note = `${why}. Showing ${labelOf(auto)}.`;
  }
  return { source: auto, auto, lines: auto ? timedOf(auto) : null, note };
}

export interface SourceStatus {
  /** Can be chosen as the lyrics. */
  usable: boolean;
  /** Short badges for usable rows, e.g. ["Synced", "Word"]. */
  badges: string[];
  /** Plain status text for the row (empty when badges say it). */
  text: string;
  /** Full sentence for title and accessible name. */
  reason: string | null;
  /** Offer "Try again". */
  retry: boolean;
}

/** What a source row says, from the entry alone. */
export function sourceStatus(s: LyricsSourceDto): SourceStatus {
  if (hasText(s)) {
    if (s.status === "timingMismatch") return { usable: true, badges: [], text: "Text only", reason: "Length differs, so only plain text is available", retry: false };
    return { usable: true, badges: wordTimedOf(s) ? ["Synced", "Word"] : timedOf(s) ? ["Synced"] : ["Text"], text: "", reason: null, retry: false };
  }
  switch (s.status) {
    case "failed": return { usable: false, badges: [], text: "Failed", reason: "Could not be loaded", retry: true };
    case "unreachable": return { usable: false, badges: [], text: "Unreachable", reason: "The service did not answer", retry: true };
    case "disabled": return { usable: false, badges: [], text: "Turned off", reason: "Turned off in Settings", retry: false };
    case "instrumental": return { usable: false, badges: [], text: "Instrumental", reason: "Instrumental track", retry: false };
    case "timingMismatch": return { usable: false, badges: [], text: "Text only", reason: "Length differs", retry: false };
    case "unavailable": return { usable: false, badges: [], text: "Not available", reason: "Not available for this track", retry: false };
    default: return { usable: false, badges: [], text: "Not found", reason: s.id === "mine" ? "You have not made lyrics for this track yet" : "No lyrics for this track", retry: false };
  }
}

/** Auto order for the Settings list: saved order first (known ids only, no repeats), then any source not in it. */
export function normalizeOrder(order: readonly string[], sources: readonly { id: string }[]): string[] {
  const known = new Set(sources.map((s) => s.id));
  const out = order.filter((id, i) => known.has(id) && order.indexOf(id) === i);
  for (const s of sources) if (!out.includes(s.id)) out.push(s.id);
  return out;
}

/** Moves `id` by `delta` places, stopping at the ends. Returns the same array when nothing moves. */
export function moveId(order: readonly string[], id: string, delta: number): readonly string[] {
  const from = order.indexOf(id);
  const to = Math.max(0, Math.min(order.length - 1, from + delta));
  if (from < 0 || to === from) return order;
  const next = order.slice();
  next.splice(to, 0, ...next.splice(from, 1));
  return next;
}
