"use client";

import { useCallback, useEffect, useMemo, useReducer, useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { Check, FolderOpen, Copy, Keyboard, Pause, Play, Redo2, Rewind, Timer, Trash2, Undo2, Minus, Plus } from "lucide-react";
import { usePlayback, togglePlayback, seekPlayback } from "../lib/playback";
import { useLyricsClock } from "../lib/lyricsClock";
import { useMotion } from "../lib/motion";
import { sourcesOf } from "../lib/lyricsSource";
import { basename, refreshMyLrc, useMyLrcStore } from "../lib/myLrc";
import {
  NUDGE_PENDING_MS, NUDGE_ROW_MS, applyOffset, emptyMaker, formatStamp, makerReducer, pendingReducer, toSaved, waitingCount,
  type MLine, type Stamp,
} from "../lib/lyricMaker";
import { lrcErrorText, lrcLoad, lrcReveal, lrcSave, type LrcSaveArgs } from "../services/tauri/lrc";
import { copyText } from "../lib/clipboard";
import { formatDuration } from "../lib/media";
import { IconButton, Scrubber } from "./ui";
import { TrackCover } from "./TrackCover";
import { CalibrateSheet, calibrationLabel, useCalibration } from "./LyricMakerCalibrate";

type Mode = "type" | "tap";
export const MODE_KEY = "lunorx.lyricMakerMode";
const readMode = (): Mode => { try { return window.localStorage.getItem(MODE_KEY) === "tap" ? "tap" : "type"; } catch { return "type"; } };
const writeMode = (m: Mode) => { try { window.localStorage.setItem(MODE_KEY, m); } catch { /* the choice lasts for this window */ } };

const STAMPABLE = new Set(["playing", "paused", "buffering"]);
const whyNot = (state: string): string =>
  state === "error" ? "the player reported an error" : state === "ended" ? "the song has ended" : state === "idle" ? "no song is playing" : `the player is ${state}`;

type SaveState = { state: "idle" | "saving" | "saved" | "error"; at: number; path: string | null; error: string | null };

const isEditable = (t: EventTarget | null) => {
  const el = t as HTMLElement | null;
  return !!el && (el.isContentEditable || el.tagName === "TEXTAREA" || el.tagName === "SELECT" || el.tagName === "INPUT");
};

/** A button that nudges once on press and repeats while held. Keyboard activation nudges once. */
function Nudge({ label, delta, onNudge, children }: { label: string; delta: number; onNudge: (d: number) => void; children: ReactNode }) {
  const timer = useRef(0);
  const stop = () => { window.clearTimeout(timer.current); window.clearInterval(timer.current); };
  useEffect(() => stop, []);
  return (
    <button type="button" aria-label={label} title={label} className="ln-mk-nudge"
      onPointerDown={(e) => { if (e.button !== 0) return; stop(); onNudge(delta); timer.current = window.setTimeout(() => { timer.current = window.setInterval(() => onNudge(delta), 80); }, 400); }}
      onPointerUp={stop} onPointerLeave={stop} onPointerCancel={stop} onBlur={stop}
      onClick={(e) => { if (e.detail === 0) onNudge(delta); }}>
      {children}
    </button>
  );
}

function Row({ id, startMs, text, current, cursor, editing, onEdit, onDone, onNudge, onRemove, onStampNow, onPick, canStamp }: {
  id?: number; startMs: number | null; text: string; current: boolean; cursor: boolean; editing: boolean; canStamp: boolean;
  onEdit?: () => void; onDone?: (text: string | null) => void; onNudge?: (d: number) => void; onRemove?: () => void; onStampNow?: () => void; onPick?: () => void;
}) {
  const [draft, setDraft] = useState(text);
  useEffect(() => { if (editing) setDraft(text); }, [editing, text]);
  return (
    <li data-row-id={id} data-current={current ? "true" : undefined} data-cursor={cursor ? "true" : undefined} aria-current={current ? "true" : cursor ? "step" : undefined} className="ln-mk-row">
      <span className="ln-mk-time" data-empty={startMs === null ? "true" : undefined}>{startMs === null ? "[--:--.--]" : `[${formatStamp(startMs)}]`}</span>
      {editing ? (
        <input autoFocus value={draft} aria-label="Edit this line" onChange={(e) => setDraft(e.target.value)} className="ln-mk-edit"
          onKeyDown={(e) => {
            if (e.nativeEvent.isComposing || e.keyCode === 229) return;
            if (e.key === "Enter") { e.preventDefault(); e.stopPropagation(); onDone?.(draft); }
            else if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); onDone?.(null); }
          }}
          onBlur={() => onDone?.(draft)} />
      ) : (
        <button type="button" data-focus-row={current || cursor ? "true" : undefined} className="ln-mk-line" onClick={() => (onPick ? onPick() : onEdit?.())}
          onDoubleClick={() => onEdit?.()}
          title={onPick ? "Move the cursor here. Double-click to edit." : "Edit this line"}>{text}</button>
      )}
      <span className="ln-mk-tools">
        {startMs === null && onStampNow && (
          <button type="button" disabled={!canStamp} aria-label={`Stamp "${text}" now`} title="Stamp this line now" onClick={onStampNow} className="ln-mk-nudge ln-mk-nudge-text">Stamp</button>)}
        {startMs !== null && onNudge && (
          <>
            <Nudge label="Earlier by 0.1 s" delta={-NUDGE_ROW_MS} onNudge={onNudge}><Minus size={14} aria-hidden="true" /></Nudge>
            <Nudge label="Later by 0.1 s" delta={NUDGE_ROW_MS} onNudge={onNudge}><Plus size={14} aria-hidden="true" /></Nudge>
          </>)}
        {startMs !== null && onStampNow && (
          <button type="button" disabled={!canStamp} aria-label={`Stamp "${text}" again now`} title="Stamp this line again now" onClick={onStampNow} className="ln-mk-nudge ln-mk-nudge-text">Re-stamp</button>)}
        {startMs !== null && onRemove && (
          <button type="button" aria-label={`Delete "${text}"`} title="Delete this line" onClick={onRemove} className="ln-mk-nudge"><Trash2 size={14} aria-hidden="true" /></button>)}
      </span>
    </li>
  );
}

const KEYS: { title: string; rows: [string, string][] }[] = [
  { title: "Type along", rows: [["First letter of a line", "Stamps its start"], ["Tab", "Stamp again, now"], ["Shift+Tab", "0.2 s earlier"], ["Enter", "Add the line"], ["Cmd/Ctrl+Enter", "Stamp the next line now"], ["Backspace to empty", "Drops the stamp"]] },
  { title: "Tap along", rows: [["Space or Enter", "Stamp the next line"], ["Backspace", "Un-stamp the last line"], ["Up / Down", "Move the cursor"]] },
  { title: "A line in the list", rows: [["[  and  ]", "0.1 s earlier / later"], ["Delete", "Delete the line"], ["Enter", "Edit the text"]] },
  { title: "Everywhere", rows: [["Cmd/Ctrl+Z", "Undo (when the field is empty)"], ["Cmd/Ctrl+Shift+Z", "Redo"], ["Cmd/Ctrl+Shift+B", "Back 5 s"], ["Esc", "Close this, then the maker"]] },
];

/**
 * The Lyric Maker: a full-window layer inside Now Playing. It records only what the user types and taps, saves a .lrc file
 * after every change through core, and never edits anything it did not stamp. Core commands (lrc_*) are the only IO.
 */
export function LyricMaker({ trackId, inset, onClose }: { trackId: string; inset: number; onClose: () => void }) {
  const s = usePlayback();
  const { reduce } = useMotion();
  const track = s.requested ?? s.track;
  const info = s.info?.providerTrackId === trackId ? s.info : null;
  const title = info?.title ?? track?.title ?? "YouTube";
  const artists = info?.artists ?? track?.artists ?? [];
  const artwork = info?.artworkUrl ?? track?.artworkUrl ?? null;
  const durationMs = s.durationMs;
  const state = s.player.state;
  const canStamp = !!track && STAMPABLE.has(state);
  const playing = state === "playing" || state === "buffering";

  const [mode, setModeState] = useState<Mode>(readMode);
  const setMode = (m: Mode) => { setModeState(m); writeMode(m); };
  const [m, dispatch] = useReducer(makerReducer, undefined, emptyMaker);
  const [pending, dispatchP] = useReducer(pendingReducer, [] as Stamp[]);
  const [text, setText] = useState("");
  const [load, setLoad] = useState<"loading" | "ready" | "failed">("loading");
  const [loadedCount, setLoadedCount] = useState(0);
  const [sheet, setSheet] = useState<null | "calibrate" | "paste">(null);
  const [keysOpen, setKeysOpen] = useState(false);
  const [editing, setEditing] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [armedClose, setArmedClose] = useState(false);
  const [paste, setPaste] = useState("");
  const [save, setSave] = useState<SaveState>({ state: "idle", at: 0, path: null, error: null });
  const { cal, offsetMs, apply, setManual } = useCalibration();

  const root = useRef<HTMLDivElement>(null);
  const field = useRef<HTMLInputElement>(null);
  const list = useRef<HTMLOListElement>(null);
  const clockEl = useRef<HTMLOutputElement>(null);
  const composing = useRef(false);
  const getPosition = useLyricsClock(true);

  // ---- Stamping: the best position estimate at the key event, minus the user's delay ----
  const stampAt = useCallback((eventTime: number) => applyOffset(getPosition(eventTime), offsetMs, durationMs), [getPosition, offsetMs, durationMs]);

  // ---- Load an existing file, once ----
  const attempt = useRef(0);
  const loadFile = useCallback(() => {
    const mine = ++attempt.current;
    setLoad("loading");
    lrcLoad(trackId).then((dto) => {
      if (mine !== attempt.current) return;
      dispatch({ type: "load", lines: dto.exists ? dto.lines : [] });
      setLoadedCount(dto.exists ? dto.lines.length : 0);
      if (dto.exists && dto.path) setSave((p) => ({ ...p, path: dto.path }));
      lastSaved.current = JSON.stringify(toSaved(dto.exists ? dto.lines : []));
      setLoad("ready");
    }).catch((e) => {
      if (mine !== attempt.current) return;
      setNotice(lrcErrorText(e).replace("save to", "read from").replace("Writing the file failed", "Reading your file failed"));
      setLoad("failed");
    });
  }, [trackId]);
  useEffect(() => { loadFile(); return () => { attempt.current++; }; }, [loadFile]);

  // ---- Autosave: serialised, latest wins, one request in flight ----
  const lastSaved = useRef("[]");
  const queued = useRef<LrcSaveArgs | null>(null);
  const running = useRef(false);
  const flush = useCallback(async () => {
    if (running.current) return;
    running.current = true;
    try {
      while (queued.current) {
        const args = queued.current;
        queued.current = null;
        setSave((p) => ({ ...p, state: "saving", error: null }));
        try {
          const r = await lrcSave(args);
          const has = args.lines.length > 0;
          useMyLrcStore.getState().set(args.providerTrackId, { exists: has, path: has ? r.path : null, lines: args.lines });
          setSave({ state: "saved", at: Date.now(), path: has ? r.path : null, error: null });
        } catch (e) {
          queued.current ??= args; // keep it for Retry; a newer edit that arrived meanwhile replaces it
          setSave((p) => ({ ...p, state: "error", error: lrcErrorText(e) }));
          return;
        }
      }
    } finally { running.current = false; }
  }, []);
  useEffect(() => {
    if (load !== "ready") return;
    const lines = toSaved(m.lines);
    const key = JSON.stringify(lines);
    if (key === lastSaved.current) return;
    lastSaved.current = key;
    queued.current = { providerTrackId: trackId, title, artists, durationMs: Math.round(durationMs), lines };
    void flush();
  }, [m.lines, load, trackId, title, artists, durationMs, flush]);
  const retry = () => { if (!queued.current) { const lines = toSaved(m.lines); queued.current = { providerTrackId: trackId, title, artists, durationMs: Math.round(durationMs), lines }; } void flush(); };
  useEffect(() => () => { if (!running.current && !queued.current) void refreshMyLrc(trackId); }, [trackId]);

  // ---- Live clock: no timers while paused or hidden ----
  useEffect(() => {
    let frame = 0;
    const paint = () => { if (clockEl.current) clockEl.current.textContent = formatStamp(getPosition()); };
    const loop = () => { paint(); frame = requestAnimationFrame(loop); };
    const stop = () => { if (frame) cancelAnimationFrame(frame); frame = 0; };
    const begin = () => { if (!frame && playing && !document.hidden) frame = requestAnimationFrame(loop); };
    const onVis = () => { if (document.hidden) stop(); else { paint(); begin(); } };
    paint();
    const unsub = usePlayback.subscribe(paint);
    document.addEventListener("visibilitychange", onVis);
    begin();
    return () => { stop(); unsub(); document.removeEventListener("visibilitychange", onVis); };
  }, [getPosition, playing]);

  // ---- Focus: the text field in Type along, the layer itself in Tap along ----
  const focusMain = useCallback(() => {
    const el = mode === "type" ? field.current : root.current;
    if (el && document.activeElement !== el) el.focus({ preventScroll: true });
  }, [mode]);
  useEffect(() => { if (!sheet && editing === null && load === "ready") focusMain(); }, [sheet, editing, load, mode, m.script.length, focusMain]);

  // ---- Keep the working line in view ----
  useEffect(() => {
    const el = list.current?.querySelector<HTMLElement>('[data-focus-row="true"]');
    el?.scrollIntoView({ block: mode === "tap" ? "center" : "end", behavior: reduce ? "auto" : "smooth" });
  }, [m.lines.length, m.cursor, mode, reduce, load]);

  // ---- Script for Tap along ----
  const sourceTexts = useMemo(() => (info ? sourcesOf(info).filter((x) => x.available && x.text).map((x) => ({ label: x.label, text: x.text as string })) : []), [info]);
  const applyScript = (raw: string) => {
    const lines = raw.split(/\r?\n/).map((l) => l.trim());
    while (lines.length && !lines[lines.length - 1]) lines.pop();
    while (lines.length && !lines[0]) lines.shift();
    dispatch({ type: "setScript", script: lines });
    setSheet(null);
  };

  // ---- Type along ----
  const hasText = text.trim() !== "";
  const waiting = waitingCount(pending, hasText);
  const commit = () => {
    const t = text.trim();
    if (!t || load !== "ready") return;
    const at = pending[0]?.ms ?? (canStamp ? stampAt(performance.now()) : null);
    if (at === null) return;
    dispatch({ type: "commit", startMs: at, text: t });
    dispatchP({ type: "commit" });
    setText("");
    setLoadedCount(0);
    setArmedClose(false);
  };
  const isTextKey = (e: KeyboardEvent) => e.key.length === 1 && e.key.trim() !== "" && !e.ctrlKey && !e.metaKey;
  const onFieldKey = (e: KeyboardEvent<HTMLInputElement>) => {
    if (e.nativeEvent.isComposing || e.keyCode === 229 || composing.current) return; // Thai and other IMEs: Enter belongs to the composition
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Enter") {
      e.preventDefault();
      if (mod) { if (canStamp && load === "ready") dispatchP({ type: "queue", ms: stampAt(e.timeStamp) }); } else commit();
    } else if (e.key === "Tab" && !mod && !e.altKey) {
      e.preventDefault(); // Tab keeps the focus in the field
      if (!canStamp || load !== "ready") return;
      if (e.shiftKey) dispatchP({ type: "nudge", deltaMs: NUDGE_PENDING_MS });
      else dispatchP({ type: "restamp", ms: stampAt(e.timeStamp) });
    } else if (mod && e.key.toLowerCase() === "z" && text === "") {
      e.preventDefault();
      e.stopPropagation(); // the layer's own undo must not run a second time
      dispatch({ type: e.shiftKey ? "redo" : "undo" });
    } else if (canStamp && load === "ready" && text === "" && isTextKey(e)) {
      dispatchP({ type: "type", ms: stampAt(e.timeStamp) });
    }
  };
  const onFieldChange = (value: string, eventTime: number) => {
    if (load !== "ready") return;
    if (text === "" && value.trim() !== "" && canStamp) dispatchP({ type: "type", ms: stampAt(eventTime) }); // paste, touch keyboards
    if (value === "" && text !== "") dispatchP({ type: "empty" });
    setText(value);
  };

  // ---- Tap along ----
  const stampCursor = (eventTime: number) => {
    if (!canStamp || load !== "ready" || m.cursor >= m.script.length) return;
    dispatch({ type: "stamp", index: m.cursor, startMs: stampAt(eventTime) });
    setLoadedCount(0);
  };

  // ---- Layer-wide keys ----
  const back5 = () => void seekPlayback(Math.max(0, getPosition() - 5000) / 1000);
  const onRootKey = (e: KeyboardEvent<HTMLDivElement>) => {
    const target = e.target as HTMLElement;
    if (e.key === "Escape") {
      e.preventDefault(); e.stopPropagation(); // never reaches Now Playing's own Escape
      if (sheet) setSheet(null); else if (keysOpen) setKeysOpen(false); else if (editing !== null) setEditing(null); else closeMaker();
      return;
    }
    e.stopPropagation(); // Space, F and the app shortcuts must not reach the layers underneath
    if (sheet) return;
    const mod = e.metaKey || e.ctrlKey;
    if (mod && e.shiftKey && e.key.toLowerCase() === "b") { e.preventDefault(); back5(); return; }
    if (mod && e.key.toLowerCase() === "z" && target.tagName !== "TEXTAREA" && !(target.tagName === "INPUT" && (target as HTMLInputElement).value !== "")) {
      e.preventDefault(); dispatch({ type: e.shiftKey ? "redo" : "undo" }); return;
    }
    const row = target.closest<HTMLElement>("[data-row-id]");
    const rowId = row ? Number(row.dataset.rowId) : null;
    if (rowId !== null && !isEditable(target) && !mod) {
      if (e.key === "[" || e.key === "]") { e.preventDefault(); dispatch({ type: "nudge", id: rowId, deltaMs: e.key === "[" ? -NUDGE_ROW_MS : NUDGE_ROW_MS, durationMs }); return; }
      if (e.key === "Delete") { e.preventDefault(); dispatch({ type: "remove", id: rowId }); focusMain(); return; }
      if (e.key === "ArrowUp" || e.key === "ArrowDown") {
        const rows = Array.from(list.current?.querySelectorAll<HTMLElement>(".ln-mk-line") ?? []);
        const at = rows.indexOf(target);
        if (at >= 0) { e.preventDefault(); rows[Math.min(rows.length - 1, Math.max(0, at + (e.key === "ArrowUp" ? -1 : 1)))]?.focus(); return; }
      }
    }
    if (mode === "tap" && !isEditable(target) && !mod) {
      if (e.key === " " || (e.key === "Enter" && target.tagName !== "BUTTON")) { e.preventDefault(); if (!e.repeat) stampCursor(e.timeStamp); }
      else if (e.key === "Backspace") { e.preventDefault(); dispatch({ type: "unstamp" }); }
      else if (e.key === "ArrowUp" || e.key === "ArrowDown") { e.preventDefault(); dispatch({ type: "cursor", to: m.cursor + (e.key === "ArrowUp" ? -1 : 1) }); }
    }
  };

  const closeMaker = () => {
    if (save.state === "error" && !armedClose) { setArmedClose(true); return; }
    onClose();
  };
  // Focus must not fall out to the dialog: Escape would then close Now Playing instead of the maker.
  const onFocusOut = (e: React.FocusEvent) => {
    if (e.relatedTarget || sheet) return;
    window.setTimeout(() => { if (!root.current?.contains(document.activeElement) && !sheet) focusMain(); }, 0);
  };

  // ---- View ----
  const lines = m.lines;
  const bySrc = useMemo(() => new Map(lines.filter((l) => l.src !== undefined).map((l) => [l.src as number, l])), [lines]);
  const unmatched = lines.filter((l) => l.src === undefined).length;
  const lastId = lines.length ? lines[lines.length - 1].id : -1;
  const savedText = save.state === "saved" ? `Saved ${new Date(save.at).toLocaleTimeString([], { hour12: false })}` : null;
  const nextScript = m.script[m.cursor];
  const done = mode === "tap" && m.script.length > 0 && m.cursor >= m.script.length;
  const stampMsg = !canStamp ? `Stamping is off: ${whyNot(state)}.` : null;
  // Pressing a button must not pull the focus out of the field (or the layer): typing and tapping go on without a click back.
  const stopFocusLoss = (e: React.MouseEvent) => { const t = e.target as HTMLElement; if (t.closest("button") && !t.closest(".ln-mk-scrim")) e.preventDefault(); };
  const edit = (id: number) => (value: string | null) => { setEditing(null); if (value !== null) dispatch({ type: "edit", id, text: value }); };

  return (
    <div ref={root} tabIndex={-1} role="region" aria-label="Lyric Maker" onKeyDown={onRootKey} onBlur={onFocusOut} onMouseDown={stopFocusLoss} data-mode={mode} className="ln-mk" style={{ "--mk-inset": `${inset}px` } as React.CSSProperties}>
      <div inert={!!sheet} className="flex min-h-0 flex-1 flex-col">
        <div data-tauri-drag-region className="h-11 shrink-0" />
        <header className="ln-mk-head">
          <div className="flex min-w-0 items-center gap-3">
            <TrackCover key={trackId} url={artwork} videoId={trackId} title={title} className="size-[52px] rounded-[8px]" />
            <div className="min-w-0">
              <h2 className="truncate font-display text-[17px] leading-6 font-semibold">{title}</h2>
              <p className="truncate text-[14px] leading-5 text-secondary-text">{artists.join(", ")}</p>
              <p className="ln-np-note mt-0.5"><span>{calibrationLabel(cal)}</span></p>
            </div>
          </div>
          <div className="ln-mk-saved">
            {save.state === "error" ? (
              <div role="alert" className="ln-mk-err">
                <span>{save.error}</span>
                <button type="button" onClick={retry} className="ln-mk-btn ln-mk-btn-solid">Retry</button>
              </div>
            ) : (
              <div className="flex items-center gap-2">
                <p role="status" aria-live="polite" className="ln-mk-status">{savedText && <><Check size={14} aria-hidden="true" /> {savedText}</>}</p>
                {save.state === "saving" && <p aria-hidden="true" className="ln-mk-status">Saving…</p>}
                {save.state === "idle" && !save.path && <p className="ln-mk-status">Saves after every line</p>}
              </div>
            )}
            {save.path && (
              <div className="flex items-center gap-1">
                <span className="ln-mk-path" title={save.path}>{basename(save.path)}</span>
                <button type="button" aria-label="Reveal in Finder" title="Reveal in Finder" className="ln-mk-chip"
                  onClick={() => lrcReveal(trackId).catch(() => setNotice("Could not open Finder."))}><FolderOpen size={14} aria-hidden="true" /> Reveal in Finder</button>
                <button type="button" aria-label="Copy path" title="Copy path" className="ln-mk-chip"
                  onClick={() => void copyText(save.path ?? "").then((ok) => setNotice(ok ? "Path copied." : "Could not copy the path."))}><Copy size={14} aria-hidden="true" /> Copy path</button>
              </div>)}
            {notice && <p role="status" className="ln-mk-status">{notice}</p>}
          </div>
        </header>

        <div className="ln-mk-listwrap">
          {load === "loading" && <p role="status" className="ln-mk-empty">Opening your file…</p>}
          {load === "failed" && (
            <div className="ln-mk-empty" role="alert">
              <p>Your saved file could not be read, so saving is paused to keep it safe.</p>
              <div className="mt-3 flex gap-2"><button type="button" onClick={loadFile} className="ln-mk-btn ln-mk-btn-solid">Try again</button>
                <button type="button" onClick={() => { dispatch({ type: "load", lines: [] }); lastSaved.current = "[]"; setNotice(null); setLoad("ready"); }} className="ln-mk-btn">Start a new file</button></div>
            </div>)}
          {load === "ready" && mode === "type" && lines.length === 0 && (
            <p className="ln-mk-empty">Press play, then type each line as you hear it begin. The first letter you type stamps the start.</p>)}
          {load === "ready" && mode === "tap" && m.script.length === 0 && (
            <div className="ln-mk-empty">
              <p>Tap along needs the words. Paste them, or use text from a lyrics source.</p>
              <div className="mt-3 flex flex-wrap gap-2">
                <button type="button" onClick={() => setSheet("paste")} className="ln-mk-btn ln-mk-btn-solid">Paste lyrics</button>
                {sourceTexts.map((x) => <button key={x.label} type="button" onClick={() => applyScript(x.text)} className="ln-mk-btn">Use {x.label} text</button>)}
              </div>
            </div>)}
          {load === "ready" && (mode === "type" ? lines.length > 0 : m.script.length > 0) && (
            <ol ref={list} aria-label={mode === "type" ? "Lines recorded so far" : "Lines to stamp"} className="ln-mk-list">
              {mode === "type" ? lines.map((l: MLine) => (
                <Row key={l.id} id={l.id} startMs={l.startMs} text={l.text} current={l.id === lastId} cursor={false} canStamp={canStamp} editing={editing === l.id}
                  onEdit={() => setEditing(l.id)} onDone={edit(l.id)} onNudge={(d) => dispatch({ type: "nudge", id: l.id, deltaMs: d, durationMs })}
                  onRemove={() => { dispatch({ type: "remove", id: l.id }); focusMain(); }} />
              )) : m.script.map((t, i) => {
                const l = bySrc.get(i);
                if (!t) return <li key={i} aria-hidden="true" className="ln-mk-gap" />;
                return (
                  <Row key={i} id={l?.id} startMs={l ? l.startMs : null} text={t} current={false} cursor={i === m.cursor} canStamp={canStamp} editing={!!l && editing === l.id}
                    onPick={() => dispatch({ type: "cursor", to: i })} onEdit={l ? () => setEditing(l.id) : undefined} onDone={l ? edit(l.id) : undefined}
                    onNudge={l ? (d) => dispatch({ type: "nudge", id: l.id, deltaMs: d, durationMs }) : undefined}
                    onRemove={l ? () => { dispatch({ type: "remove", id: l.id }); focusMain(); } : undefined}
                    onStampNow={() => { dispatch({ type: "cursor", to: i }); dispatch({ type: "stamp", index: i, startMs: stampAt(performance.now()) }); }} />
                );
              })}
            </ol>)}
        </div>

        <div className="ln-mk-input">
          {unmatched > 0 && mode === "tap" && m.script.length > 0 && <p className="ln-mk-hint">{unmatched} saved {unmatched === 1 ? "line is" : "lines are"} not in this text. {unmatched === 1 ? "It stays" : "They stay"} in the file.</p>}
          {loadedCount > 0 && (
            <p className="ln-mk-hint">
              Continue from line {loadedCount + 1}.{" "}
              <button type="button" className="underline underline-offset-2" onClick={() => void seekPlayback(Math.max(0, (lines[lines.length - 1]?.startMs ?? 0) - 2000) / 1000)}>Go to line {loadedCount}</button>
            </p>)}
          {mode === "type" ? (
            <>
              {waiting > 0 && <p role="status" className="ln-mk-hint">{waiting} {waiting === 1 ? "line" : "lines"} waiting</p>}
              <div className="ln-mk-field-row">
                <span className="ln-mk-pending" data-set={pending[0] ? "true" : undefined}>{pending[0] ? `[${formatStamp(pending[0].ms)}]` : "[--:--.--]"}</span>
                <input ref={field} value={text} aria-label="Type the line you are hearing" placeholder={canStamp ? "Type the line you are hearing" : "Waiting for the song"} autoComplete="off" autoCorrect="off" spellCheck={false}
                  readOnly={load !== "ready"} aria-describedby="mk-field-help"
                  onKeyDown={onFieldKey} onChange={(e) => onFieldChange(e.target.value, e.nativeEvent.timeStamp)}
                  onCompositionStart={(e) => { composing.current = true; if (text === "" && canStamp && load === "ready") dispatchP({ type: "type", ms: stampAt(e.timeStamp) }); }}
                  onCompositionEnd={() => { composing.current = false; }} className="ln-mk-field" />
                <output ref={clockEl} aria-hidden="true" title="Song time" className="ln-mk-clock">00:00.00</output>
              </div>
              <p id="mk-field-help" className="ln-mk-hint">
                {stampMsg ?? (pending[0] ? "Tab stamps again at the true start. Enter adds the line." : "The first letter stamps the start. Enter adds the line.")}
              </p>
            </>
          ) : (
            <div className="ln-mk-field-row">
              <span className="ln-mk-pending" data-set={nextScript ? "true" : undefined}>{done ? "Done" : "Next"}</span>
              <p className="ln-mk-next" aria-live="off">{done ? "Every line is stamped." : nextScript ?? "Add the words above first."}</p>
              <output ref={clockEl} aria-hidden="true" title="Song time" className="ln-mk-clock">00:00.00</output>
            </div>)}
          {mode === "tap" && <p className="ln-mk-hint">{stampMsg ?? "Press Space when the line begins. Backspace un-stamps the last one."}</p>}
        </div>

        <div className="ln-mk-dock">
          <div className="ln-np-pill ln-mk-pill" role="group" aria-label="Lyric Maker controls">
            <div className="flex items-center gap-3 text-[11px] tabular-nums text-secondary-text">
              <span>{formatDuration(s.positionMs / 1000)}</span>
              <Scrubber label="Seek YouTube" max={Math.max(1, durationMs / 1000)} value={s.positionMs / 1000}
                disabled={!(durationMs > 0 && STAMPABLE.has(state))} onCommit={(v) => void seekPlayback(v)} className="flex-1" />
              <span>{formatDuration(durationMs / 1000)}</span>
            </div>
            <div className="ln-np-pill-row">
              <div className="flex min-w-0 items-center justify-start gap-1">
                <button type="button" onClick={back5} className="ln-mk-btn" title="Back 5 s (Cmd/Ctrl+Shift+B)"><Rewind size={15} aria-hidden="true" /> Back 5 s</button>
                <button type="button" onClick={() => setSheet("calibrate")} className="ln-mk-btn"><Timer size={15} aria-hidden="true" /> Calibrate</button>
                <div className="relative">
                  <IconButton label="Keys" aria-expanded={keysOpen} aria-controls="mk-keys" onClick={() => setKeysOpen(!keysOpen)} className="size-9"><Keyboard size={18} /></IconButton>
                  {keysOpen && (
                    <div id="mk-keys" role="group" aria-label="Keys" className="ln-lyr-menu ln-mk-keys">
                      {KEYS.map((g) => (
                        <div key={g.title} className="mb-2 last:mb-0">
                          <p className="text-[12px] font-semibold">{g.title}</p>
                          {g.rows.map(([k, v]) => <p key={k} className="flex justify-between gap-4 text-[12px] leading-5 text-secondary-text"><span className="whitespace-pre text-primary-text">{k}</span><span className="text-right">{v}</span></p>)}
                        </div>))}
                    </div>)}
                </div>
              </div>
              <IconButton label={playing ? "Pause YouTube" : "Play YouTube"} disabled={s.busy} onClick={() => void togglePlayback()} className="size-11">
                {playing ? <Pause size={28} fill="currentColor" /> : <Play size={28} fill="currentColor" />}
              </IconButton>
              <div className="flex items-center justify-end gap-1">
                <div role="radiogroup" aria-label="Maker mode" className="ln-lyr-seg" onKeyDown={(e) => {
                  if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
                  e.preventDefault(); e.stopPropagation(); setMode(mode === "type" ? "tap" : "type");
                  window.setTimeout(() => root.current?.querySelector<HTMLElement>('[role="radio"][aria-checked="true"]')?.focus(), 0);
                }}>
                  {([["type", "Type along"], ["tap", "Tap along"]] as const).map(([id, label]) => (
                    <button key={id} type="button" role="radio" aria-checked={mode === id} tabIndex={mode === id ? 0 : -1} onClick={() => setMode(id)} className="ln-lyr-seg-btn">{label}</button>))}
                </div>
                <IconButton label="Undo" disabled={m.past.length === 0} onClick={() => dispatch({ type: "undo" })} className="size-9"><Undo2 size={18} /></IconButton>
                <IconButton label="Redo" disabled={m.future.length === 0} onClick={() => dispatch({ type: "redo" })} className="size-9"><Redo2 size={18} /></IconButton>
                <button type="button" onClick={closeMaker} className="ln-mk-btn ln-mk-btn-solid" title={armedClose && save.state === "error" ? "Close and leave these lines unsaved" : undefined}>{armedClose && save.state === "error" ? "Close without saving" : "Done"}</button>
              </div>
            </div>
          </div>
        </div>
      </div>

      {sheet === "calibrate" && <CalibrateSheet cal={cal} offsetMs={offsetMs} onApply={apply} onManual={setManual} onClose={() => setSheet(null)} />}
      {sheet === "paste" && (
        <div className="ln-mk-scrim">
          <div role="dialog" aria-modal="true" aria-labelledby="mk-paste-title" className="ln-mk-sheet">
            <h2 id="mk-paste-title" className="font-display text-[22px] font-semibold tracking-[-0.01em]">Paste the lyrics</h2>
            <p className="mt-2 text-[14px] leading-5 text-secondary-text">One line per row. Blank rows are kept as breaks and are not stamped.</p>
            <textarea autoFocus value={paste} onChange={(e) => setPaste(e.target.value)} aria-label="Lyrics to stamp" rows={9} className="ln-mk-paste" />
            <div className="mt-4 flex gap-2">
              <button type="button" disabled={!paste.trim()} onClick={() => applyScript(paste)} className="ln-mk-btn ln-mk-btn-solid">Use these lines</button>
              <button type="button" onClick={() => setSheet(null)} className="ln-mk-btn">Cancel</button>
            </div>
          </div>
        </div>)}
    </div>
  );
}
