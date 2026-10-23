"use client";

import { createPortal } from "react-dom";
import { memo, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { FilePenLine, FileUp, Info, SlidersHorizontal } from "lucide-react";
import { GAP_STEPS, gapRows, gapStep, useActiveLine, useLyricsClock } from "../lib/lyricsClock";
import { usePlayback } from "../lib/playback";
import { loadAnchors, readEstimateEnabled, readGuessEnabled, readWordsEnabled, saveAnchors, writeEstimateEnabled, writeGuessEnabled, writeWordsEnabled } from "../lib/lyricAnchors";
import { guessWords, hasWordTiming, shiftLines, wordIndexAt, wordUnits } from "../lib/lyricsTiming";
import { addAnchor, estimateLines, type Anchor } from "../lib/lyricsEstimate";
import type { LyricLineDto } from "../services/tauri/player";
import { useZoneStyle } from "../lib/textStyle";
import { FxText } from "./FxText";

// Auto-follow keeps the active line at this fraction of the visible height.
const FOLLOW_AT = 0.32;
// After the last wheel, touch or scroll key, auto-follow waits this long.
const FOLLOW_PAUSE_MS = 4000;
// Matches the mask in globals.css (.ln-lyr-scroll): lines inside the top or bottom fade count as off screen.
const FADE = 0.22;
// Smooth scrolls start at most this often; the latest target wins. A target further than 1.5 viewports away jumps.
const SCROLL_GAP_MS = 260;
const JUMP_VIEWPORTS = 1.5;
const SCROLL_KEYS = new Set(["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End", "Tab"]);

const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Where the chrome pill (counter, options) and the notes under the artist line live. Set by Now Playing; null until mounted. */
export interface LyricsSlots { chrome: HTMLElement | null; notes: HTMLElement | null; /** Saves a chosen .lrc file as the track's own lyrics. Resolves the number of lines saved. */ onImport?: (file: File) => Promise<number>; /** Opens the Lyric Maker. Shown in the options popover when set. */ onMake?: () => void }
const Slot = ({ at, children }: { at: HTMLElement | null; children: ReactNode }) => (at ? createPortal(children, at) : null);

const ESTIMATE_NOTE = "This song has no timestamps. The highlight follows an estimate and can be a few seconds off.";
const ESTIMATE_HINT = "Tap the line being sung to correct the highlight.";
const PLAIN_NOTE = "Not timed: the current line is unavailable.";

/** Source label, then its credit when it says more than the label ("Source: LyricFind"). The credit is also the label's hover text. */
const sourceParts = (label: string | null, attribution: string | null | undefined): { key: string; node: ReactNode }[] => [
  ...(label ? [{ key: "src", node: <span title={attribution ?? undefined}>{label}</span> }] : []),
  ...(attribution && attribution !== label && !/^(Lyrics from |My LRC)/.test(attribution) ? [{ key: "credit", node: attribution }] : []),
];

/**
 * The one quiet notes line under the artist: source, then state, then honesty notes, separated with a middle dot.
 * Each part is { key, node }; a part with a title (the estimate flag) carries the full sentence for hover and screen readers.
 */
function MetaLine({ parts }: { parts: { key: string; node: ReactNode }[] }) {
  return (
    <p className="ln-np-note ln-np-meta">
      {parts.map((p, i) => <span key={p.key}>{i > 0 && " · "}{p.node}</span>)}
    </p>
  );
}
// The hint goes away after the first correction in this session.
let correctedThisSession = false;

/** `word` is the index being sung when the line has real word timings and is active: -1 before the first word, the word count after the last. */
const Line = memo(function Line({ index, line, active, disabled, label, onPick, words, word, gap, fill }: {
  index: number; line: LyricLineDto; active: boolean; disabled: boolean; label: string; onPick: (index: number) => void; words: boolean; word: number;
  gap: "none" | "closed" | "open" | "live"; fill: number;
}) {
  const units = useMemo(() => (words && line.words ? wordUnits(line) : null), [words, line]);
  const fx = useZoneStyle("lyrics").effect;
  const playing = usePlayback((s) => s.player.state === "playing");
  return (
    <li data-active={active} data-enter={index < 8 ? index : undefined} className="ln-lyr-item">
      {gap !== "none" && (
        <div aria-hidden="true" data-gap={gap === "closed" ? undefined : gap} style={gap === "live" ? ({ "--p": fill / GAP_STEPS } as React.CSSProperties) : undefined} className="ln-lyr-gap">
          <i /><i /><i />
        </div>)}
      <button type="button" disabled={disabled} aria-label={label} aria-current={active ? "true" : undefined}
        data-words={units ? "true" : undefined} onClick={() => onPick(index)} className="ln-lyr-line">
        {units ? units.map((u, k) => (u.word < 0 ? u.text
          : <span key={k} data-w={u.word < word ? "sung" : u.word === word ? "cur" : "next"} className="ln-lyr-w">{active ? <FxText text={u.text} effect={fx} playing={playing} base={k} /> : u.text}</span>)) : active ? <FxText key={line.text} text={line.text} effect={fx} playing={playing} /> : line.text}
      </button>
    </li>
  );
});

export function Switch({ on, onChange, children, name }: { on: boolean; onChange: (on: boolean) => void; children: ReactNode; name?: string }) {
  return (
    <button type="button" role="switch" aria-checked={on} aria-label={name} onClick={() => onChange(!on)} className="ln-lyr-switch">
      <span aria-hidden="true" className="ln-lyr-switch-track" />
      {children}
    </button>
  );
}

function FollowSwitch({ on, onChange }: { on: boolean; onChange: (on: boolean) => void }) {
  return <Switch on={on} onChange={onChange} name="Follow along (estimated)">Follow along</Switch>;
}

/** Small popover for the switches, so the header stays one row. Closes on Escape (without closing Now Playing) and on outside press. */
function OptionsMenu({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState(false);
  const wrap = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => { if (!wrap.current?.contains(event.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [open]);
  return (
    <div ref={wrap} className="relative"
      onKeyDown={(event) => { if (open && event.key === "Escape") { event.preventDefault(); event.stopPropagation(); setOpen(false); button.current?.focus(); } }}>
      <button ref={button} type="button" aria-label="Lyrics options" aria-expanded={open} aria-controls="lyr-options" title="Lyrics options"
        onClick={() => setOpen(!open)} className="ln-lyr-opt"><SlidersHorizontal size={14} aria-hidden="true" /></button>
      {open && <div id="lyr-options" role="group" aria-label="Lyrics options" className="ln-lyr-menu">{children}</div>}
    </div>
  );
}

/** "Make lyrics file" row for the options popover. */
const MakeItem = ({ onMake }: { onMake?: () => void }) => (onMake ? (
  <button type="button" onClick={onMake} className="ln-lyr-switch"><FilePenLine size={14} aria-hidden="true" />Make lyrics file</button>) : null);

/** "Import .lrc file" row: reads a file from this computer in the webview and hands it to the app. */
function ImportItem({ onImport }: { onImport?: (file: File) => Promise<number> }) {
  const input = useRef<HTMLInputElement>(null);
  const [note, setNote] = useState("");
  if (!onImport) return null;
  return (
    <>
      <input ref={input} type="file" accept=".lrc,.txt,text/plain" hidden aria-label="Choose a .lrc file"
        onChange={(e) => {
          const file = e.target.files?.[0];
          e.target.value = "";
          if (!file) return;
          setNote("Importing…");
          onImport(file).then((n) => setNote(`Imported ${n} lines`), () => setNote("Not a valid .lrc file"));
        }} />
      <button type="button" onClick={() => input.current?.click()} className="ln-lyr-switch"><FileUp size={14} aria-hidden="true" />{note || "Import .lrc file"}</button>
    </>
  );
}

/** Controls for estimated follow mode (see UntimedLyrics). */
interface EstimateUi { onMark: (index: number, ms: number) => void; onReset: (() => void) | null; hint: boolean; toggle: ReactNode }

/**
 * Lyrics with a current line. Timed lines come from real timestamps and a click seeks to them. In estimated mode
 * (`estimate` set) the lines are a guess, a click marks the line being sung instead of seeking, and the UI says so.
 * Words are highlighted only when the source delivers real word timings (`line.words`); other lines light as a whole.
 * `offsetMs` moves every line and word (positive = later); a click seeks to the moved time.
 */
export function SyncedLyrics({ lines: given, offsetMs = 0, source, attribution, canSeek, onSeek, estimate, slots, layout, note }: {
  lines: readonly LyricLineDto[]; offsetMs?: number; source: string | null; attribution?: string | null; canSeek: boolean; onSeek: (startMs: number) => void; estimate?: EstimateUi;
  slots: LyricsSlots; layout: "full" | "split"; note?: string | null;
}) {
  const shifted = useMemo(() => shiftLines(given, offsetMs), [given, offsetMs]);
  const [wordsOn, setWordsOn] = useState(readWordsEnabled);
  const [guessOn, setGuessOn] = useState(readGuessEnabled);
  const realWords = useMemo(() => hasWordTiming(shifted), [shifted]);
  // Guessed words only fill in when the source has no real word timings, and the UI says so.
  const guessing = guessOn && !realWords && !estimate;
  const lines = useMemo(() => (guessing ? guessWords(shifted) : shifted), [guessing, shifted]);
  const showWords = guessing || (wordsOn && realWords);
  const wordAt = useMemo(() => (showWords ? (i: number, pos: number) => { const w = lines[i].words; return w && w.length > 0 ? wordIndexAt(w, pos) : -1; } : undefined), [showWords, lines]);
  // Gap rows come from real timestamps only; estimated lines have no real gaps.
  const rows = useMemo(() => (estimate ? null : gapRows(lines)), [lines, !!estimate]);
  const gapAt = useMemo(() => (rows ? (i: number, pos: number) => gapStep(lines, rows, i, pos) : undefined), [lines, rows]);
  const { index, word, gap } = useActiveLine(lines, true, wordAt, gapAt);
  const getPosition = useLyricsClock(!!estimate);
  const box = useRef<HTMLDivElement>(null);
  const paused = useRef(false);
  const timer = useRef(0);
  const placed = useRef(false);
  const [followPaused, setFollowPaused] = useState(false);
  const [offscreen, setOffscreen] = useState(false);

  const lastSmooth = useRef(0);
  const coalesce = useRef(0);
  const scrollNow = useCallback((smooth: boolean) => {
    const b = box.current;
    if (!b) return;
    const el = b.querySelector<HTMLElement>('[aria-current="true"]');
    const top = Math.max(0, el ? el.offsetTop + el.offsetHeight / 2 - b.clientHeight * FOLLOW_AT : 0);
    const near = Math.abs(top - b.scrollTop) < b.clientHeight * JUMP_VIEWPORTS;
    if (smooth) lastSmooth.current = performance.now();
    b.scrollTo({ top, behavior: smooth && near && !reducedMotion() ? "smooth" : "auto" });
  }, []);
  // Rapid line changes (or a seek through many lines) must not queue smooth scrolls: one per SCROLL_GAP_MS, aimed at the latest target.
  const scrollToActive = useCallback((smooth: boolean) => {
    window.clearTimeout(coalesce.current);
    const wait = lastSmooth.current + SCROLL_GAP_MS - performance.now();
    if (smooth && wait > 0) coalesce.current = window.setTimeout(() => scrollNow(true), wait);
    else scrollNow(smooth);
  }, [scrollNow]);

  const measure = useCallback(() => {
    const b = box.current?.getBoundingClientRect();
    const r = box.current?.querySelector<HTMLElement>('[aria-current="true"]')?.getBoundingClientRect();
    const fade = b ? b.height * FADE : 0;
    setOffscreen(!!b && !!r && (r.bottom < b.top + fade || r.top > b.bottom - fade));
  }, []);

  const follow = useCallback(() => {
    window.clearTimeout(timer.current);
    paused.current = false;
    setFollowPaused(false);
    scrollToActive(true);
  }, [scrollToActive]);

  const userScrolled = useCallback(() => {
    paused.current = true;
    setFollowPaused(true);
    window.clearTimeout(timer.current);
    timer.current = window.setTimeout(follow, FOLLOW_PAUSE_MS);
  }, [follow]);

  useEffect(() => () => { window.clearTimeout(timer.current); window.clearTimeout(coalesce.current); }, []);

  useEffect(() => {
    if (!paused.current) scrollToActive(placed.current);
    else measure();
    placed.current = true;
  }, [index, scrollToActive, measure]);

  // Rows get a stable click handler so a word change re-renders only the active row.
  const latest = useRef({ estimate, onSeek, lines, getPosition, follow });
  latest.current = { estimate, onSeek, lines, getPosition, follow };
  const pick = useCallback((i: number) => {
    const c = latest.current;
    c.follow();
    if (c.estimate) c.estimate.onMark(i, Math.round(c.getPosition()));
    else c.onSeek(c.lines[i].startMs);
  }, []);

  const parts = [
    ...sourceParts(source, attribution),
    ...(estimate
      ? [{ key: "est", node: (
          <span title={`${ESTIMATE_NOTE} ${ESTIMATE_HINT}`} className="ln-np-flag">
            estimated timing<Info size={13} aria-hidden="true" /><span className="sr-only">. {ESTIMATE_NOTE}</span>
          </span>) }]
      : [{ key: "sync", node: source === "My LRC" ? "made by you" : realWords && showWords ? "word by word" : guessing ? "words guessed" : "synced" }]),
  ];
  return (
    <div data-layout={layout} className="ln-lyr-region">
      <div ref={box} data-synced="true" className="ln-lyr-scroll"
        onWheel={userScrolled} onTouchMove={userScrolled}
        onKeyDown={(event) => { if (SCROLL_KEYS.has(event.key)) userScrolled(); }}
        onPointerDown={(event) => { if (event.target === event.currentTarget) userScrolled(); }}
        onScroll={() => { if (paused.current) measure(); }}>
        <div aria-hidden="true" className="ln-lyr-lead" />
        <ol className="ln-lyr-list" data-intro={index < 0 ? "true" : undefined}>
          {lines.map((line, i) => {
            const active = i === index;
            const g = !rows?.[i] ? "none" : i <= index ? "open" : i === index + 1 && gap >= 0 ? "live" : "closed";
            return estimate
              ? <Line key={i} index={i} line={line} active={active} disabled={false} label={`Mark "${line.text}" as the current line`}
                  onPick={pick} words={active && showWords} word={active ? word : -1} gap="none" fill={0} />
              : <Line key={i} index={i} line={line} active={active} disabled={!canSeek} label={`Jump to ${line.text}`}
                  onPick={pick} words={active && showWords} word={active ? word : -1} gap={g} fill={g === "live" ? gap : 0} />;
          })}
        </ol>
        <div aria-hidden="true" className="ln-lyr-end" />
      </div>
      {followPaused && offscreen && <button type="button" onClick={follow} className="ln-lyr-back">Back to current line</button>}
      <Slot at={slots.chrome}>
        <OptionsMenu>
          {estimate?.toggle}
          {estimate && (estimate.onReset ? <button type="button" onClick={estimate.onReset} className="ln-lyr-reset mt-1">Reset corrections</button>
            : estimate.hint ? <p className="max-w-[190px] py-1 text-[12px] leading-4 text-secondary-text">{ESTIMATE_HINT}</p> : null)}
          {realWords && <Switch on={wordsOn} onChange={(v) => { setWordsOn(v); writeWordsEnabled(v); }}>Highlight words</Switch>}
          {!realWords && !estimate && <Switch on={guessOn} onChange={(v) => { setGuessOn(v); writeGuessEnabled(v); }}>Guess words</Switch>}
          <MakeItem onMake={slots.onMake} /><ImportItem onImport={slots.onImport} />
        </OptionsMenu>
      </Slot>
      <Slot at={slots.notes}>
        {note && <p role="status" className="ln-np-note text-primary-text">{note}</p>}
        {parts.length > 0 && <MetaLine parts={parts} />}
      </Slot>
    </div>
  );
}

/** Lyrics without timestamps. Plain text only: no active line, and the note says why. */
export function PlainLyrics({ text, source, attribution, toggle, slots, layout, note }: { text: string; source: string | null; attribution?: string | null; toggle?: ReactNode; slots: LyricsSlots; layout: "full" | "split"; note?: string | null }) {
  return (
    <div data-layout={layout} className="ln-lyr-region">
      <div className="ln-lyr-scroll ln-lyr-plain">
        <p className="ln-lyr-text">{text}</p>
        <div aria-hidden="true" className="ln-lyr-tail" />
      </div>
      <Slot at={slots.chrome}>{(toggle || slots.onMake || slots.onImport) && <OptionsMenu>{toggle}<MakeItem onMake={slots.onMake} /><ImportItem onImport={slots.onImport} /></OptionsMenu>}</Slot>
      <Slot at={slots.notes}>
        {note && <p role="status" className="ln-np-note text-primary-text">{note}</p>}
        <MetaLine parts={[...sourceParts(source, attribution),
          { key: "plain", node: <span title={PLAIN_NOTE}>not timed<span className="sr-only">. {PLAIN_NOTE}</span></span> }]} />
      </Slot>
    </div>
  );
}

/**
 * Lyrics without timestamps. With "Follow along (estimated)" on and a known duration, the text is spread over the
 * song and followed like timed lyrics, labelled as an estimate and correctable by tapping the line being sung.
 * Otherwise it is the plain text with the "not timed" note.
 */
export function UntimedLyrics({ anchorId, text, source, attribution, instrumental, slots, layout, note }: {
  anchorId: string; text: string; source: string | null; attribution?: string | null; instrumental: boolean; slots: LyricsSlots; layout: "full" | "split"; note?: string | null;
}) {
  const durationMs = usePlayback((s) => s.durationMs);
  const [on, setOn] = useState(readEstimateEnabled);
  const [anchors, setAnchors] = useState<Anchor[]>(() => loadAnchors(anchorId));
  const plain = useMemo(() => text.split(/\r?\n/), [text]);
  const lines = useMemo(() => (on && !instrumental ? estimateLines(plain, durationMs, anchors) : null), [on, instrumental, plain, durationMs, anchors]);
  const canEstimate = !instrumental && durationMs > 0;
  const toggle = <FollowSwitch on={on} onChange={(v) => { setOn(v); writeEstimateEnabled(v); }} />;
  if (!lines || lines.length === 0) {
    return <PlainLyrics text={text} source={source} attribution={attribution} slots={slots} layout={layout} note={note} toggle={canEstimate ? toggle : undefined} />;
  }
  const mark = (i: number, ms: number) => {
    const next = addAnchor(anchors, { lineIndex: i, ms }, lines.length, durationMs);
    correctedThisSession = true;
    setAnchors(next);
    saveAnchors(anchorId, next);
  };
  const reset = () => { setAnchors([]); saveAnchors(anchorId, []); };
  return <SyncedLyrics lines={lines} source={source} attribution={attribution} canSeek={false} onSeek={() => undefined} slots={slots} layout={layout} note={note}
    estimate={{ onMark: mark, onReset: anchors.length ? reset : null, hint: !correctedThisSession && anchors.length === 0, toggle }} />;
}
