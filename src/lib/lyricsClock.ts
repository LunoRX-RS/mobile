"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { usePlayback } from "./playback";

/** A line becomes active this long before its timestamp so the highlight lands as the words start. */
export const LOOKAHEAD_MS = 150;
// Core reports about once a second. If reports stop while "playing", hold instead of running ahead.
const MAX_EXTRAPOLATE_MS = 2000;

/** Last line whose startMs <= positionMs + LOOKAHEAD_MS, or -1 before the first line. Lines must be sorted. */
export function activeIndex(lines: readonly { startMs: number }[], positionMs: number): number {
  const at = positionMs + LOOKAHEAD_MS;
  let low = 0;
  let high = lines.length - 1;
  let found = -1;
  while (low <= high) {
    const mid = (low + high) >> 1;
    if (lines[mid].startMs <= at) { found = mid; low = mid + 1; }
    else high = mid - 1;
  }
  return found;
}

/** Instrumental gap before a line: more than 6 s after the previous line starts, or the first line starts after 3 s. */
const GAP_MIN_MS = 6000;
const INTRO_GAP_MS = 3000;
/** The dots appear this long after the previous line starts, so they do not sit on top of words still being sung. */
const GAP_AFTER_MS = 2500;
/** Progress of the dots comes in this many steps (state changes at most this often per gap). */
export const GAP_STEPS = 12;

/** gaps[i] is true when a gap row belongs above line i. Real timestamps only: never call it for estimated lines. */
export function gapRows(lines: readonly { startMs: number }[]): boolean[] {
  return lines.map((l, i) => (i === 0 ? l.startMs > INTRO_GAP_MS : l.startMs - lines[i - 1].startMs > GAP_MIN_MS));
}

/** Progress 0..GAP_STEPS toward the next line while its gap row is live, else -1. `posMs` is the same clock as activeIndex uses. */
export function gapStep(lines: readonly { startMs: number }[], rows: readonly boolean[], index: number, posMs: number): number {
  const next = index + 1;
  if (next >= lines.length || !rows[next]) return -1;
  const from = next === 0 ? 0 : lines[index].startMs + GAP_AFTER_MS;
  if (posMs < from) return -1;
  return Math.round(Math.min(1, (posMs - from) / (lines[next].startMs - from)) * GAP_STEPS);
}

interface Sync { ms: number; at: number; playing: boolean }

/**
 * Returns a stable getter for the current playback position in ms. Between core's reports it advances
 * with performance.now() while the state is "playing"; otherwise it holds the reported value.
 * Every progress report and every seek (a position jump) re-syncs it. Nothing here runs a timer.
 */
export function useLyricsClock(enabled: boolean): (at?: number) => number {
  const sync = useRef<Sync>({ ms: 0, at: 0, playing: false });
  /** `time` is a performance.now() timeline value (default: now); the Lyric Maker passes event.timeStamp. */
  const getPosition = useCallback((time: number = performance.now()) => {
    const { ms, at, playing } = sync.current;
    if (!playing) return ms;
    const { durationMs } = usePlayback.getState();
    const next = ms + Math.min(Math.max(time - at, -MAX_EXTRAPOLATE_MS), MAX_EXTRAPOLATE_MS);
    return durationMs > 0 ? Math.min(next, durationMs) : next;
  }, []);
  useEffect(() => {
    if (!enabled) return;
    const first = usePlayback.getState();
    sync.current = { ms: first.positionMs, at: performance.now(), playing: first.player.state === "playing" };
    return usePlayback.subscribe((s, prev) => {
      const playing = s.player.state === "playing";
      if (s.positionMs !== prev.positionMs) {
        sync.current = { ms: s.positionMs, at: performance.now(), playing };
      } else if (s.player.state !== prev.player.state) {
        // State change without a new position: continue from where the clock already is.
        sync.current = { ms: getPosition(), at: performance.now(), playing };
      }
    });
  }, [enabled, getPosition]);
  return getPosition;
}

/**
 * Index of the active line, or -1, the word being sung in it (`wordAt`, optional; -1 when unused) and the gap progress
 * (`gapAt`, optional; -1 when unused). State changes only when one of the three changes, so a long list does not re-render every frame. The rAF loop runs only while enabled,
 * the player is playing and the document is visible; paused or seeking while paused is handled from store updates.
 */
export function useActiveLine(
  lines: readonly { startMs: number }[], enabled: boolean, wordAt?: (index: number, positionMs: number) => number, gapAt?: (index: number, positionMs: number) => number,
): { index: number; word: number; gap: number } {
  const getPosition = useLyricsClock(enabled);
  const playing = usePlayback((s) => s.player.state === "playing");
  const [at, setAt] = useState({ index: -1, word: -1, gap: -1 });
  useEffect(() => {
    if (!enabled) { setAt((p) => (p.index === -1 && p.word === -1 && p.gap === -1 ? p : { index: -1, word: -1, gap: -1 })); return; }
    const tick = () => {
      const pos = getPosition();
      const index = activeIndex(lines, pos);
      const word = wordAt && index >= 0 ? wordAt(index, pos + LOOKAHEAD_MS) : -1;
      const gap = gapAt ? gapAt(index, pos + LOOKAHEAD_MS) : -1;
      // One state object, one render: a line change, its first word and its gap state always land together.
      setAt((p) => (p.index === index && p.word === word && p.gap === gap ? p : { index, word, gap }));
    };
    tick();
    const unsubscribe = usePlayback.subscribe(tick);
    let frame = 0;
    const loop = () => { tick(); frame = requestAnimationFrame(loop); };
    const start = () => { if (!frame && playing && !document.hidden) frame = requestAnimationFrame(loop); };
    const stop = () => { if (frame) cancelAnimationFrame(frame); frame = 0; };
    const onVisibility = () => { if (document.hidden) stop(); else { tick(); start(); } };
    document.addEventListener("visibilitychange", onVisibility);
    start();
    return () => { stop(); unsubscribe(); document.removeEventListener("visibilitychange", onVisibility); };
  }, [lines, enabled, playing, getPosition, wordAt, gapAt]);
  return at;
}
