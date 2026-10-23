/** Estimated line timing for lyrics that have no timestamps. A heuristic: the real timing is unknown. */

export interface Anchor { lineIndex: number; ms: number }
export interface EstimatedLine { startMs: number; text: string }

export const MIN_LINE_MS = 1200;
export const MAX_ANCHORS = 40;
const MIN_WEIGHT = 10;
const PAUSE_WEIGHT = 14;
const THAI_MARK_WEIGHT = 0.5;

const clamp = (v: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, v));
const MARK = /^\p{M}$/u;

let segmenter: Intl.Segmenter | null | undefined;
function graphemes(text: string): string[] {
  if (segmenter === undefined) segmenter = typeof Intl !== "undefined" && typeof Intl.Segmenter === "function" ? new Intl.Segmenter(undefined, { granularity: "grapheme" }) : null;
  return segmenter ? Array.from(segmenter.segment(text), (s) => s.segment) : Array.from(text);
}

/** Visible length of a line: one per grapheme, 0.5 per combining mark (Thai vowel and tone marks), spaces free. */
function lineWeight(text: string): number {
  let w = 0;
  for (const g of graphemes(text)) {
    if (/^\s+$/u.test(g)) continue;
    const cps = Array.from(g);
    w += MARK.test(cps[0]) ? THAI_MARK_WEIGHT : 1;
    for (let i = 1; i < cps.length; i++) if (MARK.test(cps[i])) w += THAI_MARK_WEIGHT;
  }
  return Math.max(w, MIN_WEIGHT);
}

/**
 * Keeps anchors that agree with each other. `anchors` is ordered oldest to newest; when two disagree
 * (same line, or times in the wrong order, or closer than MIN_LINE_MS per line) the older one is dropped.
 * Result is sorted by line index.
 */
export function sanitizeAnchors(anchors: readonly Anchor[], lineCount: number, durationMs = Infinity): Anchor[] {
  const kept: Anchor[] = [];
  for (let k = anchors.length - 1; k >= 0; k--) {
    const a = anchors[k];
    if (!a || !Number.isInteger(a.lineIndex) || a.lineIndex < 0 || a.lineIndex >= lineCount) continue;
    if (!Number.isFinite(a.ms) || a.ms < 0 || a.ms > durationMs) continue;
    const ok = kept.every((b) => a.lineIndex < b.lineIndex
      ? b.ms - a.ms >= MIN_LINE_MS * (b.lineIndex - a.lineIndex)
      : a.lineIndex > b.lineIndex && a.ms - b.ms >= MIN_LINE_MS * (a.lineIndex - b.lineIndex));
    if (ok) kept.push(a);
  }
  return kept.sort((x, y) => x.lineIndex - y.lineIndex);
}

/** Adds a correction as the newest anchor. Older anchors that contradict it are dropped. Capped at MAX_ANCHORS. */
export function addAnchor(anchors: readonly Anchor[], anchor: Anchor, lineCount: number, durationMs = Infinity): Anchor[] {
  return sanitizeAnchors([...anchors.slice(-(MAX_ANCHORS - 1)), anchor], lineCount, durationMs);
}

/**
 * Splits `plainLines` (blank lines are stanza breaks) over the song. Returns null when the duration is unknown.
 * Window: [intro, duration - outro], intro = clamp(2%, 1.5 s, 6 s), outro = clamp(4%, 3 s, 12 s); line 0 counts as current from 0 s. Each line gets a
 * minimum of MIN_LINE_MS and the rest of its segment in proportion to its weight plus a pause weight for a stanza
 * break after it. Anchors pin lines exactly; the lines between two anchors are redistributed the same way.
 */
export function estimateLines(plainLines: readonly string[], durationMs: number, anchors: readonly Anchor[] = []): EstimatedLine[] | null {
  if (!Number.isFinite(durationMs) || durationMs <= 0) return null;
  const text: string[] = [];
  const cost: number[] = []; // line weight plus the pause weight of a stanza break right after it
  let breakPending = false;
  for (const raw of plainLines) {
    const t = raw.trim();
    if (!t) { breakPending = true; continue; }
    if (breakPending && cost.length) cost[cost.length - 1] += PAUSE_WEIGHT;
    breakPending = false;
    text.push(t);
    cost.push(lineWeight(t));
  }
  const n = text.length;
  if (n === 0) return [];

  // Early is the safer guess: most songs start singing within a few seconds, and a lit first line beats a dead intro.
  let w0 = clamp(durationMs * 0.02, 1500, 6000);
  let w1 = durationMs - clamp(durationMs * 0.04, 3000, 12000);
  if (w1 - w0 < n * MIN_LINE_MS) { w0 = 0; w1 = durationMs; } // too short for the window: spread over the whole song

  const starts = new Array<number>(n).fill(0);
  // Lines from..to-1 between times t and u; line `from` starts exactly at t.
  const fill = (from: number, to: number, t: number, u: number) => {
    const count = to - from;
    if (count <= 0) return;
    const free = u - t - count * MIN_LINE_MS;
    let total = 0;
    for (let m = from; m < to; m++) total += cost[m];
    let prefix = 0;
    for (let k = from; k < to; k++) {
      starts[k] = free < 0 ? t + ((u - t) * (k - from)) / count : t + MIN_LINE_MS * (k - from) + (free * prefix) / total;
      prefix += cost[k];
    }
  };

  const pins = sanitizeAnchors(anchors, n, durationMs);
  if (pins.length === 0) {
    fill(0, n, w0, w1);
  } else {
    const first = pins[0];
    if (first.lineIndex > 0) fill(0, first.lineIndex, Math.max(0, Math.min(w0, first.ms - MIN_LINE_MS * first.lineIndex)), first.ms);
    for (let i = 0; i < pins.length; i++) {
      const a = pins[i];
      const next = pins[i + 1];
      const end = next ? next.ms : Math.min(durationMs, Math.max(w1, a.ms + MIN_LINE_MS * (n - a.lineIndex)));
      fill(a.lineIndex, next ? next.lineIndex : n, a.ms, end);
      starts[a.lineIndex] = a.ms;
    }
  }

  // The first line is the current line from the start: there is no empty "intro" state for an estimate.
  if (!pins.some((p) => p.lineIndex === 0)) starts[0] = 0;
  const out: EstimatedLine[] = [];
  for (let k = 0; k < n; k++) {
    const s = Math.round(starts[k]);
    out.push({ startMs: k > 0 && s <= out[k - 1].startMs ? out[k - 1].startMs + 1 : s, text: text[k] });
  }
  return out;
}
