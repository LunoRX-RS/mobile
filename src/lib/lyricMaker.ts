// Lyric Maker: pure logic. No React, no DOM, no timers. Everything here is unit-tested in Node.
// The tool only records what the user taps; nothing here guesses a line time.

// ---- Constants ----
/** Used until the user calibrates. About a quick listener's key reaction. */
export const DEFAULT_OFFSET_MS = 220;
export const OFFSET_MIN = -500;
export const OFFSET_MAX = 500;
/** Shift+Tab moves the pending stamp earlier by this much. */
export const NUDGE_PENDING_MS = 200;
/** Row nudge buttons and [ ] keys. */
export const NUDGE_ROW_MS = 100;
export const MAX_QUEUE = 3;
export const MAX_HISTORY = 50;
/** Core reports about once a second; never run ahead of the last report by more than this. */
export const MAX_EXTRAPOLATE_MS = 2000;
/** Calibration */
export const CAL_BEEPS = 12;
export const CAL_SKIP = 2;
export const CAL_MIN_DELTA = -100;
export const CAL_MAX_DELTA = 800;
export const CAL_WINDOW_MS = 300;
export const CAL_GOOD = 0.8;
export const CAL_MIN_USED = 5;
/** 1 / 1.4826 scales the median absolute deviation to a standard deviation for normal data. */
export const MAD_SCALE = 1.4826;

export const clampOffset = (ms: number): number =>
  Number.isFinite(ms) ? Math.min(OFFSET_MAX, Math.max(OFFSET_MIN, Math.round(ms))) : DEFAULT_OFFSET_MS;

// ---- Stamp time ----
export interface ClockSync { ms: number; at: number; playing: boolean }

/**
 * Player position at the instant of a key event. `eventTime` is event.timeStamp (same timeline as performance.now()),
 * so the time spent before our handler ran is not added. While paused the held position is used.
 * Returns the position, not yet corrected for the user's delay.
 */
export function positionAt(sync: ClockSync, eventTime: number, durationMs = 0): number {
  if (!sync.playing) return sync.ms;
  const elapsed = Math.min(MAX_EXTRAPOLATE_MS, Math.max(-MAX_EXTRAPOLATE_MS, eventTime - sync.at));
  const pos = Math.max(0, sync.ms + elapsed);
  return durationMs > 0 ? Math.min(pos, durationMs) : pos;
}

/** The line's start: the position when the key went down, minus the user's typical delay, kept inside the song. */
export function applyOffset(positionMs: number, offsetMs: number, durationMs = 0): number {
  const t = Math.round(positionMs - clampOffset(offsetMs));
  return Math.max(0, durationMs > 0 ? Math.min(t, durationMs) : t);
}

export const stampTime = (sync: ClockSync, eventTime: number, offsetMs: number, durationMs = 0): number =>
  applyOffset(positionAt(sync, eventTime, durationMs), offsetMs, durationMs);

// ---- Formatting ----
/** mm:ss.xx with centiseconds, rounded to the nearest. Minutes keep growing past 59 (an LRC has no hours field). */
export function formatStamp(ms: number): string {
  const cs = Math.max(0, Math.round(ms / 10));
  const m = Math.floor(cs / 6000);
  const s = Math.floor((cs % 6000) / 100);
  return `${String(m).padStart(2, "0")}:${String(s).padStart(2, "0")}.${String(cs % 100).padStart(2, "0")}`;
}

export const lrcLine = (startMs: number, text: string): string => `[${formatStamp(startMs)}]${text}`;

export interface TimedLine { startMs: number; text: string }

/** What a saved file looks like (the Rust side writes the real file; this is the preview and the test oracle). */
export function lrcText(lines: readonly TimedLine[], meta?: { title?: string; artists?: readonly string[]; durationMs?: number }): string {
  const head: string[] = [];
  if (meta?.title) head.push(`[ti:${meta.title}]`);
  if (meta?.artists?.length) head.push(`[ar:${meta.artists.join(", ")}]`);
  if (meta?.durationMs && meta.durationMs > 0) head.push(`[length:${formatStamp(meta.durationMs).slice(0, 5)}]`);
  return [...head, ...toSaved(lines).map((l) => lrcLine(l.startMs, l.text))].join("\n");
}

// ---- Lines and the undo reducer ----
export interface MLine { id: number; startMs: number; text: string; /** Index in the script (Tap along), when the line came from it. */ src?: number }

export interface MakerState {
  lines: MLine[];
  nextId: number;
  /** Tap along: the text lines to stamp, and the next one to stamp. */
  script: string[];
  cursor: number;
  past: Snap[];
  future: Snap[];
}
interface Snap { lines: MLine[]; cursor: number; nextId: number }

export const emptyMaker = (): MakerState => ({ lines: [], nextId: 1, script: [], cursor: 0, past: [], future: [] });

export type MakerAction =
  | { type: "load"; lines: readonly TimedLine[] }
  | { type: "commit"; startMs: number; text: string }
  | { type: "setScript"; script: readonly string[] }
  | { type: "stamp"; index: number; startMs: number }
  | { type: "unstamp" }
  | { type: "cursor"; to: number }
  | { type: "edit"; id: number; text: string }
  | { type: "nudge"; id: number; deltaMs: number; durationMs?: number }
  | { type: "remove"; id: number }
  | { type: "undo" }
  | { type: "redo" };

const snap = (s: MakerState): Snap => ({ lines: s.lines, cursor: s.cursor, nextId: s.nextId });

function record(s: MakerState, next: Partial<MakerState>): MakerState {
  return { ...s, ...next, past: [...s.past, snap(s)].slice(-MAX_HISTORY), future: [] };
}

/** Insert after every line that starts at or before startMs, so equal times keep the order they were made in. */
export function insertSorted(lines: readonly MLine[], line: MLine): MLine[] {
  let at = lines.length;
  while (at > 0 && lines[at - 1].startMs > line.startMs) at--;
  return [...lines.slice(0, at), line, ...lines.slice(at)];
}

const sortStable = (lines: readonly MLine[]): MLine[] =>
  lines.map((l, i) => [l, i] as const).sort((a, b) => a[0].startMs - b[0].startMs || a[1] - b[1]).map(([l]) => l);

const clean = (t: string) => t.replace(/\s+/g, " ").trim();

export function makerReducer(s: MakerState, a: MakerAction): MakerState {
  switch (a.type) {
    case "load": {
      const lines = a.lines.map((l, i) => ({ id: i + 1, startMs: Math.max(0, Math.round(l.startMs)), text: clean(l.text) })).filter((l) => l.text);
      return { ...emptyMaker(), lines: sortStable(lines), nextId: lines.length + 1, script: s.script, cursor: s.cursor, };
    }
    case "commit": {
      const text = clean(a.text);
      if (!text) return s;
      const line: MLine = { id: s.nextId, startMs: Math.max(0, Math.round(a.startMs)), text };
      return record(s, { lines: insertSorted(s.lines, line), nextId: s.nextId + 1 });
    }
    case "setScript": {
      const script = a.script.map(clean);
      // Lines that already carry this text keep their place, so a loaded file continues instead of starting over.
      const taken = new Set<number>();
      const lines = s.lines.map((l) => {
        const i = script.findIndex((t, k) => t === l.text && !taken.has(k));
        if (i < 0) return { ...l, src: undefined };
        taken.add(i);
        return { ...l, src: i };
      });
      let cursor = script.findIndex((_, k) => !taken.has(k));
      if (cursor < 0) cursor = script.length;
      return { ...s, script, lines, cursor };
    }
    case "stamp": {
      const text = s.script[a.index];
      if (text === undefined || !text) return s;
      const startMs = Math.max(0, Math.round(a.startMs));
      const without = s.lines.filter((l) => l.src !== a.index);
      const line: MLine = { id: s.nextId, startMs, text, src: a.index };
      let cursor = Math.min(a.index + 1, s.script.length);
      // Skip lines that are blank in the script (stanza breaks): they have nothing to stamp.
      while (cursor < s.script.length && !s.script[cursor]) cursor++;
      return record(s, { lines: insertSorted(without, line), nextId: s.nextId + 1, cursor });
    }
    case "unstamp": {
      let best = -1;
      for (const l of s.lines) if (l.src !== undefined && l.src < s.cursor && l.src > best) best = l.src;
      if (best < 0) return s;
      return record(s, { lines: s.lines.filter((l) => l.src !== best), cursor: best });
    }
    case "cursor":
      return { ...s, cursor: Math.min(s.script.length, Math.max(0, a.to)) };
    case "edit": {
      const text = clean(a.text);
      const cur = s.lines.find((l) => l.id === a.id);
      if (!cur || !text || text === cur.text) return s;
      return record(s, { lines: s.lines.map((l) => (l.id === a.id ? { ...l, text } : l)) });
    }
    case "nudge": {
      const cur = s.lines.find((l) => l.id === a.id);
      if (!cur) return s;
      const max = a.durationMs && a.durationMs > 0 ? a.durationMs : Infinity;
      const startMs = Math.min(max, Math.max(0, Math.round(cur.startMs + a.deltaMs)));
      if (startMs === cur.startMs) return s;
      return record(s, { lines: sortStable(s.lines.map((l) => (l.id === a.id ? { ...l, startMs } : l))) });
    }
    case "remove": {
      const cur = s.lines.find((l) => l.id === a.id);
      if (!cur) return s;
      return record(s, { lines: s.lines.filter((l) => l.id !== a.id), cursor: cur.src !== undefined ? Math.min(s.cursor, cur.src) : s.cursor });
    }
    case "undo": {
      const prev = s.past[s.past.length - 1];
      if (!prev) return s;
      return { ...s, ...prev, past: s.past.slice(0, -1), future: [snap(s), ...s.future].slice(0, MAX_HISTORY) };
    }
    case "redo": {
      const next = s.future[0];
      if (!next) return s;
      return { ...s, ...next, past: [...s.past, snap(s)].slice(-MAX_HISTORY), future: s.future.slice(1) };
    }
  }
}

/** What goes to the file: stamped lines only, sorted by time (stable), text trimmed. */
export function toSaved(lines: readonly TimedLine[]): TimedLine[] {
  return lines.map((l, i) => [l, i] as const)
    .sort((a, b) => a[0].startMs - b[0].startMs || a[1] - b[1])
    .map(([l]) => ({ startMs: Math.round(l.startMs), text: l.text.trim() }))
    .filter((l) => l.text);
}

// ---- Pending stamps (Type along) ----
export interface Stamp { ms: number; /** Made by the first keystroke, so backspacing to empty drops it. */ auto: boolean }

export type PendingAction =
  | { type: "type"; ms: number }
  | { type: "restamp"; ms: number }
  | { type: "nudge"; deltaMs: number }
  | { type: "queue"; ms: number }
  | { type: "empty" }
  | { type: "commit" };

/** Up to MAX_QUEUE stamps, oldest first. The text field always fills the oldest one. */
export function pendingReducer(q: readonly Stamp[], a: PendingAction): Stamp[] {
  switch (a.type) {
    case "type": return q.length === 0 ? [{ ms: a.ms, auto: true }] : [...q];
    case "restamp": return q.length === 0 ? [{ ms: a.ms, auto: false }] : [{ ms: a.ms, auto: false }, ...q.slice(1)];
    case "nudge": return q.length === 0 ? [] : [{ ...q[0], ms: Math.max(0, q[0].ms - a.deltaMs) }, ...q.slice(1)];
    case "queue": return q.length >= MAX_QUEUE ? [...q] : [...q, { ms: a.ms, auto: false }];
    case "empty": return q.length === 1 && q[0].auto ? [] : [...q];
    case "commit": return q.slice(1);
  }
}

/** "2 lines waiting": stamps not yet attached to text. The first stamp belongs to the field once it has text. */
export const waitingCount = (q: readonly Stamp[], fieldHasText: boolean): number => Math.max(0, q.length - (fieldHasText ? 1 : 0));

// ---- Calibration ----
/** Small seeded generator (mulberry32) so a run is repeatable in tests. */
export function seeded(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** Beep start times in ms from the run start: first at `leadMs`, then 1.0 to 1.6 s apart. */
export function beepSchedule(seed: number, count = CAL_BEEPS, leadMs = 1500): number[] {
  const rnd = seeded(seed);
  const out: number[] = [];
  let t = leadMs;
  for (let i = 0; i < count; i++) { out.push(Math.round(t)); t += 1000 + rnd() * 600; }
  return out;
}

export interface OutputClock {
  /** audioContext.currentTime (s) and performance.now() (ms) read together. */
  ctxNow: number; perfNow: number; baseLatency?: number; outputLatency?: number;
  /** audioContext.getOutputTimestamp(), when the browser has it and it is filled in. */
  out?: { contextTime?: number; performanceTime?: number };
}

/** performance.now() timeline of the moment audio-context time `ctxTime` (s) comes out of the speaker. */
export function beepPerfTime(ctxTime: number, c: OutputClock): number {
  const o = c.out;
  if (o && typeof o.contextTime === "number" && typeof o.performanceTime === "number" && o.performanceTime > 0) {
    return o.performanceTime + (ctxTime - o.contextTime) * 1000;
  }
  return c.perfNow + (ctxTime - c.ctxNow) * 1000 + ((c.baseLatency ?? 0) + (c.outputLatency ?? 0)) * 1000;
}

/** Pair each tap with the latest beep that began no more than 100 ms later than the tap and has no tap yet. Extra taps are dropped. */
export function matchTaps(beeps: readonly number[], taps: readonly number[]): (number | null)[] {
  const deltas: (number | null)[] = beeps.map(() => null);
  for (const t of [...taps].sort((a, b) => a - b)) {
    let k = -1;
    for (let i = 0; i < beeps.length; i++) if (beeps[i] <= t - CAL_MIN_DELTA) k = i;
    if (k >= 0 && deltas[k] === null && t - beeps[k] <= CAL_MAX_DELTA) deltas[k] = t - beeps[k];
  }
  return deltas;
}

export const median = (xs: readonly number[]): number => {
  const s = [...xs].sort((a, b) => a - b);
  const m = s.length >> 1;
  return s.length === 0 ? NaN : s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2;
};

export interface CalibrationResult {
  ok: boolean;
  /** Typical delay in ms (median of the taps used), clamped to the stepper range. */
  offsetMs: number;
  /** 1.4826 x MAD of the taps used. */
  jitterMs: number;
  /** Share of the counted beeps whose tap landed within 300 ms of the typical delay (a missed beep counts as outside). */
  consistency: number;
  used: number;
  counted: number;
  good: boolean;
}

/** `deltas`: per beep, tap minus beep time in ms, or null when no tap matched. The first CAL_SKIP beeps are warm-up. */
export function calibrationStats(deltas: readonly (number | null)[]): CalibrationResult {
  const counted = deltas.slice(CAL_SKIP);
  const used = counted.filter((d): d is number => d !== null && d >= CAL_MIN_DELTA && d <= CAL_MAX_DELTA);
  if (used.length < CAL_MIN_USED) return { ok: false, offsetMs: DEFAULT_OFFSET_MS, jitterMs: 0, consistency: 0, used: used.length, counted: counted.length, good: false };
  const med = median(used);
  const jitter = MAD_SCALE * median(used.map((d) => Math.abs(d - med)));
  const consistency = used.filter((d) => Math.abs(d - med) <= CAL_WINDOW_MS).length / Math.max(1, counted.length);
  return { ok: true, offsetMs: clampOffset(med), jitterMs: Math.round(jitter), consistency, used: used.length, counted: counted.length, good: consistency >= CAL_GOOD };
}

export interface StoredCalibration { offsetMs: number; consistency: number | null; jitterMs: number | null; date: string; manual: boolean }

export function parseCalibration(raw: string | null): StoredCalibration | null {
  try {
    const v = raw ? (JSON.parse(raw) as Partial<StoredCalibration>) : null;
    if (!v || typeof v.offsetMs !== "number" || !Number.isFinite(v.offsetMs)) return null;
    const c = typeof v.consistency === "number" && v.consistency >= 0 && v.consistency <= 1 ? v.consistency : null;
    return { offsetMs: clampOffset(v.offsetMs), consistency: c, jitterMs: typeof v.jitterMs === "number" ? v.jitterMs : null, date: typeof v.date === "string" ? v.date : "", manual: v.manual === true };
  } catch { return null; }
}
