"use client";

import { ChevronDown, ChevronLeft, ChevronRight, Loader2, Minus, Plus, Search, Settings, Sparkles } from "lucide-react";
import { useCallback, useEffect, useId, useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { labelOf, orderSources, sourceStatus, type LyricsChoice, type LyricsSourcePref } from "../lib/lyricsSource";
import { loadLyricsSettings, useLyricsStore } from "../lib/lyricsStore";
import { OFFSET_STEP_MS, clampOffset, formatOffset } from "../lib/lyricsTiming";
import { lyricsSourceBind, lyricsSourceFetch, lyricsSourceSearch, lyricsSourceUnbind, type LyricsCandidateDto, type LyricsTrackArgs } from "../services/tauri/lyricsSources";
import type { LyricsSourceDto } from "../services/tauri/player";
import { formatDuration } from "../lib/media";

const FOCUSABLE = 'button:not(:disabled), input:not(:disabled), [tabindex="0"]';
const REPEAT_DELAY_MS = 400;
const REPEAT_MS = 70;

function useOnline(): boolean {
  const [online, setOnline] = useState(true);
  useEffect(() => {
    const sync = () => setOnline(navigator.onLine);
    sync();
    window.addEventListener("online", sync);
    window.addEventListener("offline", sync);
    return () => { window.removeEventListener("online", sync); window.removeEventListener("offline", sync); };
  }, []);
  return online;
}

const Badge = ({ children }: { children: ReactNode }) => <span className="ln-src-badge">{children}</span>;
const Spinner = () => <Loader2 size={14} aria-hidden="true" className="animate-spin motion-reduce:animate-none" />;

/** Timing stepper: 0.05 s steps, hold to repeat. A pointer press steps at once and then repeats; the keyboard uses the button's own click and key repeat. */
function Stepper({ value, onChange }: { value: number; onChange: (ms: number) => void }) {
  const latest = useRef(value);
  latest.current = value;
  const timers = useRef<{ wait: number; tick: number }>({ wait: 0, tick: 0 });
  const stop = useCallback(() => { window.clearTimeout(timers.current.wait); window.clearInterval(timers.current.tick); }, []);
  useEffect(() => stop, [stop]);
  const step = (dir: 1 | -1) => { const next = clampOffset(latest.current + dir * OFFSET_STEP_MS); latest.current = next; onChange(next); };
  const hold = (dir: 1 | -1) => ({
    onPointerDown: (e: React.PointerEvent) => { if (e.button !== 0) return; stop(); step(dir); timers.current.wait = window.setTimeout(() => { timers.current.tick = window.setInterval(() => step(dir), REPEAT_MS); }, REPEAT_DELAY_MS); },
    onPointerUp: stop, onPointerLeave: stop, onPointerCancel: stop, onBlur: stop,
    onClick: (e: React.MouseEvent) => { if (e.detail === 0) step(dir); },
  });
  return (
    <div role="group" aria-label="Timing offset" className="ln-src-stepper">
      <button type="button" aria-label="Move lyrics earlier by 0.05 seconds" disabled={value <= -5000} {...hold(-1)}><Minus size={16} aria-hidden="true" /></button>
      <output aria-label="Current offset" className="ln-src-offset">{formatOffset(value)}</output>
      <button type="button" aria-label="Move lyrics later by 0.05 seconds" disabled={value >= 5000} {...hold(1)}><Plus size={16} aria-hidden="true" /></button>
    </div>
  );
}

type SearchState =
  | { state: "idle" }
  | { state: "loading" }
  | { state: "error" }
  | { state: "done"; items: LyricsCandidateDto[] };

export interface SourceSheetProps {
  track: LyricsTrackArgs;
  sources: readonly LyricsSourceDto[];
  pref: LyricsSourcePref;
  choice: LyricsChoice;
  /** The lyrics info for this track: loading, failed or ready. */
  info: "loading" | "error" | "ready";
  onPick: (next: LyricsSourcePref) => void;
  /** Core delivered a new entry (Try again, a hand-picked match, an unbind). */
  onEntry: (entry: LyricsSourceDto) => void;
  /** The chosen source has real timestamps, so a timing offset means something. */
  offsetApplies: boolean;
  offsetMs: number;
  onOffset: (ms: number) => void;
  onOpenSettings: () => void;
}

/**
 * The "Source: <name>" button in the controls pill and the sheet it opens upward: Automatic, one radio per source,
 * Timing, and Search by title. Escape goes back one level, then closes; focus is trapped in the sheet and returns to the button.
 */
export function LyricsSourceControl(p: SourceSheetProps) {
  const uid = useId();
  const [open, setOpen] = useState(false);
  const [view, setView] = useState<"main" | "search">("main");
  const wrap = useRef<HTMLDivElement>(null);
  const button = useRef<HTMLButtonElement>(null);
  const sheet = useRef<HTMLDivElement>(null);
  const online = useOnline();
  const settings = useLyricsStore((s) => s.settings);
  useEffect(() => { void loadLyricsSettings(); }, []);

  const close = useCallback((focus = true) => { setOpen(false); setView("main"); if (focus) button.current?.focus(); }, []);
  useEffect(() => {
    if (!open) return;
    const away = (event: PointerEvent) => { if (!wrap.current?.contains(event.target as Node)) close(false); };
    document.addEventListener("pointerdown", away);
    return () => document.removeEventListener("pointerdown", away);
  }, [open, close]);
  // Move focus into the sheet when it opens or its view changes: the chosen radio, or the search field.
  useEffect(() => {
    if (!open) return;
    const root = sheet.current;
    (root?.querySelector<HTMLElement>('[role="radio"][tabindex="0"], input') ?? root?.querySelector<HTMLElement>(FOCUSABLE))?.focus();
  }, [open, view, p.info]);

  const onKeyDown = (event: ReactKeyboardEvent) => {
    if (!open) return;
    if (event.key === "Escape") {
      event.preventDefault();
      event.stopPropagation(); // Now Playing closes on Escape too; this is one level
      if (view === "search") setView("main"); else close();
      return;
    }
    if (event.key !== "Tab") return;
    const items = Array.from(sheet.current?.querySelectorAll<HTMLElement>(FOCUSABLE) ?? []).filter((el) => el.tabIndex >= 0);
    if (items.length === 0) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    const next = event.shiftKey ? (at <= 0 ? items.length - 1 : at - 1) : (at < 0 || at === items.length - 1 ? 0 : at + 1);
    event.preventDefault();
    items[next].focus();
  };

  const shown = p.choice.auto;
  const current = p.info === "loading" ? "loading" : p.pref === "auto" || !p.choice.source ? `Auto · ${shown ? labelOf(shown) : "none"}` : labelOf(p.choice.source);
  const order = settings?.order ?? [];
  return (
    <div ref={wrap} className="relative min-w-0" onKeyDown={onKeyDown}>
      <button ref={button} type="button" aria-haspopup="dialog" aria-expanded={open} aria-controls={open ? uid : undefined}
        onClick={() => (open ? close(false) : setOpen(true))} className="ln-src-btn">
        <span className="truncate"><span className="text-secondary-text">Source: </span>{current}</span>
        <ChevronDown size={14} aria-hidden="true" className="shrink-0" />
      </button>
      {open && (
        <div ref={sheet} id={uid} role="dialog" aria-label="Lyrics source" className="ln-src-sheet">
          {view === "main"
            ? <MainView {...p} order={order} online={online} onSearch={() => setView("search")} />
            : <SearchView track={p.track} online={online} onBack={() => setView("main")}
                onBound={(entry) => { p.onEntry(entry); p.onPick(entry.id); setView("main"); }} />}
        </div>
      )}
    </div>
  );
}

function MainView(p: SourceSheetProps & { order: readonly string[]; online: boolean; onSearch: () => void }) {
  const group = useRef<HTMLDivElement>(null);
  const [busy, setBusy] = useState<Record<string, boolean>>({});
  const [said, setSaid] = useState("");
  const timing = useRef<HTMLDivElement>(null);
  const rows = orderSources(p.sources, p.order);
  const chosen = p.pref !== "auto" && p.choice.source?.id === p.pref ? p.pref : "auto";
  const tabbable = chosen;

  const onRadioKey = (event: ReactKeyboardEvent) => {
    const keys = ["ArrowDown", "ArrowRight", "ArrowUp", "ArrowLeft", "Home", "End"];
    if (!keys.includes(event.key)) return;
    const radios = Array.from(group.current?.querySelectorAll<HTMLElement>('[role="radio"]:not(:disabled)') ?? []);
    const at = radios.indexOf(document.activeElement as HTMLElement);
    if (radios.length === 0 || at < 0) return;
    event.preventDefault();
    const to = event.key === "Home" ? 0 : event.key === "End" ? radios.length - 1
      : (at + (event.key === "ArrowDown" || event.key === "ArrowRight" ? 1 : -1) + radios.length) % radios.length;
    radios[to].focus();
  };

  // A finished retry or unbind can remove the button that had focus: keep focus inside the sheet.
  useEffect(() => {
    const root = group.current?.closest<HTMLElement>('[role="dialog"]');
    if (said && root && !root.contains(document.activeElement)) root.querySelector<HTMLElement>('[role="radio"][tabindex="0"]')?.focus();
  }, [said]);

  const run = async (s: LyricsSourceDto, action: "retry" | "unbind") => {
    setBusy((b) => ({ ...b, [s.id]: true }));
    setSaid("");
    try {
      if (action === "unbind") await lyricsSourceUnbind({ providerTrackId: p.track.providerTrackId, sourceId: s.id });
      const entry = await lyricsSourceFetch({ ...p.track, sourceId: s.id });
      p.onEntry(entry);
      const ok = sourceStatus(entry).usable;
      setSaid(action === "unbind" ? `${labelOf(s)}: match removed.` : ok ? `${labelOf(s)}: lyrics found.` : `${labelOf(s)}: ${entry.status === "unreachable" ? "still unreachable" : entry.status === "failed" ? "failed again" : "no lyrics found"}.`);
    } catch {
      setSaid(`${labelOf(s)}: could not try again.`);
    } finally { setBusy((b) => ({ ...b, [s.id]: false })); }
  };

  return (
    <>
      <header className="ln-src-head">
        <div className="min-w-0">
          <h3 className="ln-src-title">Lyrics source</h3>
          <p className="ln-src-quiet">Just for this song. The default order is in Settings.</p>
        </div>
        <button type="button" aria-label="Open lyrics settings" title="Open lyrics settings" onClick={p.onOpenSettings} className="ln-src-icon"><Settings size={16} aria-hidden="true" /></button>
      </header>
      <div className="ln-src-body">
        {!p.online && <p className="ln-src-note">You are offline. Lyrics already loaded still work; Try again and search need a connection.</p>}
        {p.info === "loading" && (
          <div aria-hidden="true" className="ln-src-skel">{[0, 1, 2, 3, 4].map((i) => <div key={i} className="lx-shimmer" />)}</div>
        )}
        {p.info === "loading" && <p role="status" className="sr-only">Loading lyrics sources</p>}
        {p.info === "error" && <p role="alert" className="ln-src-note">Lyrics sources could not be loaded. Playback can continue.</p>}
        {p.info === "ready" && (
          <div ref={group} role="radiogroup" aria-label="Lyrics source" onKeyDown={onRadioKey} className="ln-src-list">
            <div role="none" className="ln-src-row" data-auto="">
              <button type="button" role="radio" aria-checked={chosen === "auto"} tabIndex={tabbable === "auto" ? 0 : -1} onClick={() => p.onPick("auto")} className="ln-src-radio">
                <span className="ln-src-dot" aria-hidden="true" />
                <Sparkles size={15} aria-hidden="true" className="shrink-0 text-secondary-text" />
                <span className="min-w-0 flex-1">
                  <span className="block">Automatic</span>
                  <span className="ln-src-sub">{p.choice.auto ? `Showing ${labelOf(p.choice.auto)}` : "No lyrics found"}</span>
                </span>
              </button>
            </div>
            {rows.map((s) => {
              const st = sourceStatus(s);
              const title = st.reason ?? undefined;
              const off = !p.online;
              return (
                <div key={s.id} role="none" className="ln-src-row">
                  <button type="button" role="radio" aria-checked={chosen === s.id} disabled={!st.usable} tabIndex={tabbable === s.id ? 0 : -1} title={title}
                    aria-label={st.usable ? `${labelOf(s)}, ${[...st.badges, st.text].filter(Boolean).join(", ")}${s.bound ? ", matched by you" : ""}` : `${labelOf(s)}. ${st.text}. ${st.reason ?? ""}`}
                    onClick={() => p.onPick(s.id)} className="ln-src-radio">
                    <span className="ln-src-dot" aria-hidden="true" />
                    <span className="min-w-0 flex-1 truncate">{labelOf(s)}</span>
                    <span className="ln-src-state" aria-hidden="true">
                      {s.bound && <span className="ln-src-mine">Matched by you</span>}
                      {st.badges.map((b) => <Badge key={b}>{b}</Badge>)}
                      {st.text && <span title={title}>{st.text}</span>}
                    </span>
                  </button>
                  {st.retry && (
                    <button type="button" aria-disabled={busy[s.id] || off} onClick={() => { if (!busy[s.id] && !off) void run(s, "retry"); }} className="ln-src-link">
                      {busy[s.id] ? <><Spinner />Trying</> : "Try again"}
                    </button>)}
                  {s.bound && (
                    <button type="button" aria-disabled={busy[s.id] || off} onClick={() => { if (!busy[s.id] && !off) void run(s, "unbind"); }} className="ln-src-link">
                      {busy[s.id] ? <><Spinner />Removing</> : "Unbind"}
                    </button>)}
                </div>
              );
            })}
          </div>
        )}
        <p role="status" className="ln-src-said">{said}</p>
      </div>
      {p.offsetApplies && (
        <div ref={timing} className="ln-src-timing">
          <div className="min-w-0 flex-1">
            <p className="ln-src-label">Timing</p>
            <p className="ln-src-sub">Lyrics ahead of the singer? Move them later.</p>
          </div>
          <div className="flex flex-col items-end gap-1">
            <Stepper value={p.offsetMs} onChange={p.onOffset} />
            {p.offsetMs !== 0 && <button type="button" onClick={() => { p.onOffset(0); timing.current?.querySelector<HTMLElement>("[aria-label^='Move lyrics later']")?.focus(); }} className="ln-src-link">Reset</button>}
          </div>
        </div>
      )}
      <button type="button" onClick={p.onSearch} className="ln-src-search">
        <Search size={15} aria-hidden="true" className="shrink-0 text-secondary-text" />
        <span className="flex-1 text-left">Wrong song? Search by title</span>
        <ChevronRight size={16} aria-hidden="true" className="shrink-0 text-secondary-text" />
      </button>
    </>
  );
}

function SearchView({ track, online, onBack, onBound }: { track: LyricsTrackArgs; online: boolean; onBack: () => void; onBound: (entry: LyricsSourceDto) => void }) {
  const [query, setQuery] = useState(() => [track.title, ...track.artists].join(" ").trim());
  const [result, setResult] = useState<SearchState>({ state: "idle" });
  const [binding, setBinding] = useState<string | null>(null);
  const [bindError, setBindError] = useState(false);
  const labels = useLyricsStore((s) => s.settings?.sources);
  const token = useRef(0);
  const nameOf = (id: string) => labels?.find((x) => x.id === id)?.label ?? id;

  const search = useCallback(async () => {
    const q = query.trim();
    if (!q) return;
    const mine = ++token.current;
    setResult({ state: "loading" });
    setBindError(false);
    try {
      const { candidates } = await lyricsSourceSearch({ title: track.title, artists: track.artists, query: q });
      if (mine === token.current) setResult({ state: "done", items: candidates });
    } catch { if (mine === token.current) setResult({ state: "error" }); }
  }, [query, track.title, track.artists]);

  const choose = async (c: LyricsCandidateDto) => {
    setBinding(c.key);
    setBindError(false);
    try {
      onBound(await lyricsSourceBind({ providerTrackId: track.providerTrackId, sourceId: c.sourceId, candidateKey: c.key, title: track.title, artists: track.artists, durationMs: track.durationMs }));
    } catch { setBindError(true); setBinding(null); }
  };

  return (
    <>
      <header className="ln-src-head">
        <button type="button" aria-label="Back to sources" title="Back to sources" onClick={onBack} className="ln-src-icon"><ChevronLeft size={18} aria-hidden="true" /></button>
        <h3 className="ln-src-title flex-1">Search by title</h3>
      </header>
      <div className="ln-src-body">
        <div className="ln-src-field">
          <Search size={15} aria-hidden="true" className="shrink-0 text-secondary-text" />
          <input type="search" value={query} onChange={(e) => setQuery(e.target.value)} aria-label="Song title and artist" placeholder="Song title and artist" autoComplete="off" spellCheck={false}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); if (online) void search(); } }} />
        </div>
        {!online && <p className="ln-src-note">You are offline. Search needs a connection.</p>}
        {result.state === "idle" && online && <p className="ln-src-quiet px-1">Press Enter to search every source that is turned on.</p>}
        {result.state === "loading" && (<>
          <div aria-hidden="true" className="ln-src-skel">{[0, 1, 2, 3].map((i) => <div key={i} className="lx-shimmer" />)}</div>
          <p role="status" className="sr-only">Searching</p>
        </>)}
        {result.state === "error" && (
          <div role="alert" className="ln-src-note flex items-center justify-between gap-3">
            <span>Search failed.</span><button type="button" onClick={() => void search()} className="ln-src-link">Retry</button>
          </div>)}
        {result.state === "done" && result.items.length === 0 && <p role="status" className="ln-src-quiet px-1">No other matches</p>}
        {bindError && <p role="alert" className="ln-src-note">That match could not be used. Try another.</p>}
        {result.state === "done" && result.items.length > 0 && (
          <ul aria-label="Matches" className="ln-src-results">
            {result.items.map((c) => (
              <li key={`${c.sourceId}:${c.key}`}>
                <button type="button" aria-disabled={binding !== null} onClick={() => { if (binding === null) void choose(c); }} className="ln-src-hit">
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{c.title}</span>
                    <span className="ln-src-sub block truncate">{c.artist}{c.durationMs > 0 && ` · ${formatDuration(c.durationMs / 1000)}`} · {nameOf(c.sourceId)}</span>
                  </span>
                  <span className="ln-src-state" aria-hidden="true">
                    {binding === c.key ? <Spinner /> : <>{c.synced && <Badge>Synced</Badge>}{c.words && <Badge>Word</Badge>}</>}
                  </span>
                  <span className="sr-only">{[c.synced && "synced", c.words && "word by word"].filter(Boolean).join(", ")}</span>
                </button>
              </li>
            ))}
          </ul>)}
      </div>
    </>
  );
}
