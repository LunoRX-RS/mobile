import type { LyricLineDto, LyricWordDto } from "../services/tauri/player";

// Pure helpers for real timestamps: the per-track timing offset and real word timings. Only guessWords estimates, and callers must label it.

export const OFFSET_STEP_MS = 50;
export const OFFSET_MAX_MS = 5000;

/** Snaps to 0.05 s steps within +-5 s. */
export const clampOffset = (ms: number): number =>
  Number.isFinite(ms) ? Math.max(-OFFSET_MAX_MS, Math.min(OFFSET_MAX_MS, Math.round(ms / OFFSET_STEP_MS) * OFFSET_STEP_MS)) || 0 : 0;

/** "0.00 s", "+0.25 s", "-1.50 s". */
export const formatOffset = (ms: number): string => `${ms > 0 ? "+" : ms < 0 ? "-" : ""}${(Math.abs(ms) / 1000).toFixed(2)} s`;

/** Every line and word start/end moves by `offsetMs` (positive = later). Returns the same array for 0. */
export function shiftLines(lines: readonly LyricLineDto[], offsetMs: number): readonly LyricLineDto[] {
  if (!offsetMs) return lines;
  return lines.map((l) => ({
    startMs: l.startMs + offsetMs,
    text: l.text,
    words: l.words ? l.words.map((w) => ({ startMs: w.startMs + offsetMs, endMs: w.endMs + offsetMs, text: w.text })) : null,
  }));
}

/** True when at least one line carries real word timings. */
export const hasWordTiming = (lines: readonly LyricLineDto[] | null | undefined): boolean => !!lines?.some((l) => !!l.words && l.words.length > 0);

/** One rendered piece of a line: `word` is the index into line.words, or -1 for the text between words. */
export interface WordUnit { text: string; word: number }

/**
 * Pieces of a line for rendering real word timings. Text between words comes from the line itself, so spacing and
 * punctuation render exactly as given. If the words do not match the line text, they are joined with single spaces.
 */
export function wordUnits(line: LyricLineDto): WordUnit[] {
  const words: readonly LyricWordDto[] = line.words ?? [];
  const out: WordUnit[] = [];
  let at = 0;
  for (let i = 0; i < words.length; i++) {
    // Whitespace inside an inline-block span collapses, so spaces a source puts on a word render outside the span.
    const core = words[i].text.trim();
    const k = core ? line.text.indexOf(core, at) : -1;
    if (k < 0) return fallbackUnits(words);
    if (k > at) out.push({ text: line.text.slice(at, k), word: -1 });
    out.push({ text: core, word: i });
    at = k + core.length;
  }
  if (at < line.text.length) out.push({ text: line.text.slice(at), word: -1 });
  return out;
}

function fallbackUnits(words: readonly LyricWordDto[]): WordUnit[] {
  const out: WordUnit[] = [];
  words.forEach((w, i) => {
    if (i > 0) out.push({ text: " ", word: -1 });
    out.push({ text: w.text.trim(), word: i });
  });
  return out;
}

/** The word being sung at `posMs`: -1 before the first word, `words.length` once the last word has ended. */
export function wordIndexAt(words: readonly LyricWordDto[], posMs: number): number {
  let found = -1;
  for (let i = 0; i < words.length; i++) {
    if (words[i].startMs <= posMs) found = i;
    else break;
  }
  return found === words.length - 1 && found >= 0 && posMs >= words[found].endMs ? words.length : found;
}

const OFFSET_PREFIX = "lunorx.lyricsOffset.";
export const offsetKey = (trackId: string, sourceId: string) => `${OFFSET_PREFIX}${trackId}:${sourceId}`;

export function readOffset(key: string): number {
  try { return clampOffset(Number(window.localStorage.getItem(key))); } catch { return 0; }
}

/** 0 removes the key. Storage failure keeps the offset for this session only. */
export function writeOffset(key: string, ms: number): void {
  try { if (ms) window.localStorage.setItem(key, String(ms)); else window.localStorage.removeItem(key); } catch { /* storage unavailable */ }
}

const GUESS_MAX_LINE_MS = 8000;
const GUESS_LAST_LINE_MS = 4000;
const GUESS_TAIL_MS = 150;

/** Words of a line for guessing: real words for spaced text, Intl.Segmenter words for Thai/CJK when the platform has it. */
function splitWords(text: string): string[] {
  const seg = typeof Intl !== "undefined" && "Segmenter" in Intl
    ? Array.from(new Intl.Segmenter(undefined, { granularity: "word" }).segment(text)).filter((s) => s.isWordLike).map((s) => s.segment)
    : [];
  return seg.length > 0 ? seg : text.split(/\s+/).filter(Boolean);
}

/**
 * GUESSED word timings for lines that only have a start time: each line's span (until the next line, capped) is shared
 * between its words by length. A guess, not a measurement: callers must label it. Lines that already have words are kept.
 */
export function guessWords(lines: readonly LyricLineDto[]): readonly LyricLineDto[] {
  return lines.map((l, i) => {
    if (l.words && l.words.length > 0) return l;
    const parts = splitWords(l.text);
    if (parts.length === 0) return l;
    const next = lines[i + 1]?.startMs;
    const span = Math.max(300, Math.min(GUESS_MAX_LINE_MS, (next ?? l.startMs + GUESS_LAST_LINE_MS) - l.startMs - GUESS_TAIL_MS));
    const weights = parts.map((p) => Math.max(1, Array.from(p).length));
    const total = weights.reduce((a, b) => a + b, 0);
    let at = l.startMs;
    const words: LyricWordDto[] = parts.map((text, k) => {
      const start = at;
      at += (span * weights[k]) / total;
      return { startMs: Math.round(start), endMs: Math.round(at), text };
    });
    return { ...l, words };
  });
}
