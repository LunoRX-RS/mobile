import type { Anchor } from "./lyricsEstimate";
import { MAX_ANCHORS } from "./lyricsEstimate";

const PREFIX = "lunorx.lyricAnchors.";
const INDEX = "lunorx.lyricAnchorsIndex";
const MAX_TRACKS = 30;
export const ESTIMATE_KEY = "lunorx.lyricsEstimate";

const memory = new Map<string, Anchor[]>();

function valid(v: unknown): Anchor[] {
  if (!Array.isArray(v)) return [];
  return v.filter((a): a is Anchor => !!a && typeof a === "object" && Number.isInteger((a as Anchor).lineIndex) && (a as Anchor).lineIndex >= 0
    && Number.isFinite((a as Anchor).ms) && (a as Anchor).ms >= 0).map((a) => ({ lineIndex: a.lineIndex, ms: a.ms })).slice(-MAX_ANCHORS);
}

/** Corrections belong to one text, so the key carries the source: `<trackId>:<sourceId>`. */
export const anchorKey = (trackId: string, sourceId: string) => `${trackId}:${sourceId}`;

export function loadAnchors(trackId: string): Anchor[] {
  const hit = memory.get(trackId);
  if (hit) return hit;
  try {
    const raw = window.localStorage.getItem(PREFIX + trackId);
    const list = raw ? valid(JSON.parse(raw)) : [];
    memory.set(trackId, list);
    return list;
  } catch { return []; }
}

export function saveAnchors(trackId: string, anchors: Anchor[]): void {
  memory.set(trackId, anchors);
  try {
    const parsed: unknown = JSON.parse(window.localStorage.getItem(INDEX) ?? "[]");
    const ids = (Array.isArray(parsed) ? parsed.filter((x): x is string => typeof x === "string") : []).filter((id) => id !== trackId);
    if (anchors.length === 0) {
      window.localStorage.removeItem(PREFIX + trackId);
    } else {
      window.localStorage.setItem(PREFIX + trackId, JSON.stringify(anchors));
      ids.push(trackId);
    }
    for (const old of ids.splice(0, Math.max(0, ids.length - MAX_TRACKS))) { window.localStorage.removeItem(PREFIX + old); memory.delete(old); }
    window.localStorage.setItem(INDEX, JSON.stringify(ids));
  } catch { /* storage unavailable: corrections last for this session only */ }
}

export function readEstimateEnabled(): boolean {
  try { return window.localStorage.getItem(ESTIMATE_KEY) !== "off"; } catch { return true; }
}
export function writeEstimateEnabled(on: boolean): void {
  try { window.localStorage.setItem(ESTIMATE_KEY, on ? "on" : "off"); } catch { /* ignore */ }
}

export const WORDS_KEY = "lunorx.lyricsWords";
export function readWordsEnabled(): boolean {
  try { return window.localStorage.getItem(WORDS_KEY) !== "off"; } catch { return true; }
}
export function writeWordsEnabled(on: boolean): void {
  try { window.localStorage.setItem(WORDS_KEY, on ? "on" : "off"); } catch { /* ignore */ }
}

export const GUESS_KEY = "lunorx.lyricsGuessWords";
export function readGuessEnabled(): boolean {
  try { return window.localStorage.getItem(GUESS_KEY) !== "off"; } catch { return true; }
}
export function writeGuessEnabled(on: boolean): void {
  try { window.localStorage.setItem(GUESS_KEY, on ? "on" : "off"); } catch { /* ignore */ }
}
