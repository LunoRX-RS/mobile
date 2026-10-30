"use client";
import { useLayout } from "../../lib/layout";
import { useLibrary } from "../../lib/library";
import { playTrack as playSavedTrack } from "../../lib/playback";

import {
  Cable,
  CirclePlay,
  Cloud,
  ChartColumn,
  Compass,
  Heart,
  Mic,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  Plus,
  Search,
  Settings,
  Sparkles,
  type LucideIcon,
} from "lucide-react";
import { Fragment, useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent, type FocusEvent } from "react";
import { createPortal } from "react-dom";
import { useUi, type View } from "../../lib/store";
import { usePlayback } from "../../lib/playback";
import type { Tone } from "../../lib/media";
import { useLocalPlaylists } from "../../features/playlists/usePlaylists";
import { syncStatus, type SyncStatusDto } from "../../services/tauri/sync";
import { Art } from "../Art";
import { menuHandlers } from "../ContextMenu";
import { playlistMenu } from "../../lib/menus";
import { playPlaylist } from "../../lib/playlistActions";
import { ActivePill, LayoutLi } from "../Motion";
import { IconButton, cx } from "../ui";

const COLLAPSED_KEY = "lunorx.sidebar.collapsed";
const ORDER_KEY = "lunorx.sidebar.playlistOrder";
const TONES: Tone[] = ["slate", "violet", "amber", "rose", "moss", "ink"];
const EASE = "ease-[cubic-bezier(0.16,1,0.3,1)]";

// localStorage can throw or be absent (private windows, blocked site data). Every access is guarded.
function readStore(key: string): string | null {
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}
function writeStore(key: string, value: string) {
  try {
    window.localStorage.setItem(key, value);
  } catch {
    /* the sidebar works without persistence */
  }
}

function toneFor(seed: string): Tone {
  let h = 2166136261;
  for (let i = 0; i < seed.length; i++) h = Math.imul(h ^ seed.charCodeAt(i), 16777619);
  return TONES[(h >>> 0) % TONES.length];
}

/* ---------- Tooltip (collapsed mode) ---------- */

type Tip = { top: number; left: number; title: string; sub?: string };

// Portalled to <body>: the sidebar's backdrop-filter would otherwise become the containing block for fixed children.
function useTip(enabled: boolean) {
  const [tip, setTip] = useState<Tip | null>(null);
  const show = (el: HTMLElement, title: string, sub?: string) => {
    if (!enabled) return;
    const r = el.getBoundingClientRect();
    setTip({ top: r.top + r.height / 2, left: r.right + 10, title, sub });
  };
  const hide = () => setTip(null);
  const bind = (title: string, sub?: string) => ({
    onMouseEnter: (e: MouseEvent<HTMLElement>) => show(e.currentTarget, title, sub),
    onFocus: (e: FocusEvent<HTMLElement>) => show(e.currentTarget, title, sub),
    onMouseLeave: hide,
    onBlur: hide,
  });
  const node =
    enabled && tip
      ? createPortal(
          <div
            aria-hidden="true"
            className="glass-solid lx-tip pointer-events-none fixed z-[70] max-w-[240px] -translate-y-1/2 rounded-[8px] px-2.5 py-1.5 shadow-[0_8px_24px_rgba(0,0,0,0.5)]"
            style={{ top: tip.top, left: tip.left }}
          >
            <p className="truncate text-[13px] leading-[18px] font-semibold text-primary-text">{tip.title}</p>
            {tip.sub && <p className="truncate text-[12px] leading-4 text-secondary-text">{tip.sub}</p>}
          </div>,
          document.body,
        )
      : null;
  return { bind, hide, node };
}

/* ---------- Primary navigation ---------- */

type NavDef = { view: View; label: string; Icon: LucideIcon };
const NAV: NavDef[] = [
  { view: "home", label: "Listen Now", Icon: CirclePlay },
  { view: "search", label: "Search", Icon: Search },
  { view: "explore", label: "Explore", Icon: Compass },
  { view: "podcasts", label: "Podcasts", Icon: Mic },
  { view: "stats", label: "Stats", Icon: ChartColumn },
  { view: "replay", label: "Unified Replay", Icon: Sparkles },
];

function NavRow({
  def,
  active,
  collapsed,
  onSelect,
  bind,
}: {
  def: NavDef;
  active: boolean;
  collapsed: boolean;
  onSelect: () => void;
  bind: ReturnType<typeof useTip>["bind"];
}) {
  const { Icon, label } = def;
  return (
    <button
      type="button"
      onClick={onSelect}
      aria-current={active ? "page" : undefined}
      aria-label={collapsed ? label : undefined}
      {...(collapsed ? bind(label) : {})}
      className={cx(
        "lx-press relative isolate flex h-11 w-full items-center gap-3 rounded-[8px] px-2.5 text-[14px] leading-5 font-medium transition-colors duration-150",
        collapsed && "justify-center px-0",
        active ? "text-primary-text" : "text-secondary-text hover:bg-frost-100 hover:text-primary-text",
      )}
    >
      {active && <ActivePill layoutId="nav-pill" className="bg-frost-200" />}
      {/* Active = filled variant of the same glyph (a soft fill under the 1.75 stroke). */}
      <Icon size={20} aria-hidden="true" className="shrink-0" fill={active ? "currentColor" : "none"} fillOpacity={active ? 0.28 : 0} />
      {!collapsed && <span className="lx-label truncate">{label}</span>}
    </button>
  );
}

/* ---------- Library ---------- */

type Filter = "all" | "playlists" | "artists";
const FILTERS: { id: Filter; label: string }[] = [
  { id: "all", label: "All" },
  { id: "playlists", label: "Playlists" },
  { id: "artists", label: "Artists" },
];

type Kind = "liked" | "playlist" | "album" | "artist";
interface LibItem {
  key: string;
  kind: Kind;
  title: string;
  subtitle: string;
  tone: Tone;
  seed: string;
  active: boolean;
  playing: boolean;
  open: () => void;
  play: () => void;
  /** Right-click menu for this row, when it has one. */
  menu?: () => import("../ContextMenu").MenuItem[];
}

function Equaliser() {
  return (
    <span aria-hidden="true" className="lx-eq shrink-0">
      <i />
      <i />
      <i />
    </span>
  );
}

function LibRow({
  item,
  collapsed,
  tabbable,
  onFocusRow,
  bind,
}: {
  item: LibItem;
  collapsed: boolean;
  tabbable: boolean;
  onFocusRow: (key: string) => void;
  bind: ReturnType<typeof useTip>["bind"];
}) {
  const name = `${item.title}, ${item.subtitle}${item.playing ? ", Playing" : ""}`;
  const round = item.kind === "artist" ? "rounded-full" : "rounded-[6px]";
  const size = collapsed ? "size-11" : "size-10";
  return (
    <LayoutLi>
      <button
        type="button"
        data-lib-row=""
        data-key={item.key}
        data-reorder={item.kind === "playlist" ? "" : undefined}
        tabIndex={tabbable ? 0 : -1}
        aria-label={name}
        aria-current={item.active ? "page" : undefined}
        onClick={item.open}
        {...(item.menu ? menuHandlers(item.menu) : {})}
        {...(collapsed ? bind(item.title, item.subtitle) : {})}
        onFocusCapture={() => onFocusRow(item.key)}
        className={cx(
          "relative isolate flex h-12 w-full items-center rounded-[8px] transition-colors duration-150",
          collapsed ? "justify-center" : "gap-3 px-2.5 text-left",
          !item.active && "hover:bg-frost-100",
        )}
      >
        {item.active && <ActivePill layoutId="lib-pill" className="bg-frost-200" />}
        <span className={cx("relative shrink-0", size)}>
          <Art seed={item.seed} tone={item.tone} rounded={round} grain={false} className={cx("absolute inset-0", size)} />
          {item.kind === "liked" && (
            <Heart size={size === "size-11" ? 18 : 16} aria-hidden="true" fill="currentColor" className="absolute top-1/2 left-1/2 -translate-x-1/2 -translate-y-1/2 text-ink-bright" />
          )}
          {collapsed && item.playing && (
            <span className="absolute -right-0.5 -bottom-0.5 rounded-[4px] bg-night-canvas/80 p-[3px]">
              <Equaliser />
            </span>
          )}
        </span>
        {!collapsed && (
          <>
            <span className="lx-label flex min-w-0 flex-1 flex-col">
              <span title={item.title} className="truncate text-[13px] leading-[18px] font-semibold text-primary-text">
                {item.title}
              </span>
              <span title={item.subtitle} className="truncate text-[12px] leading-4 text-secondary-text">
                {item.subtitle}
              </span>
            </span>
            {item.playing && <Equaliser />}
          </>
        )}
      </button>
    </LayoutLi>
  );
}

function SkeletonRows({ collapsed }: { collapsed: boolean }) {
  return (
    <>
      {[0, 1].map((i) => (
        <li key={i} aria-hidden="true" className={cx("flex h-12 items-center", collapsed ? "justify-center" : "gap-3 px-2.5")}>
          <span className={cx("lx-shimmer shrink-0 rounded-[6px] bg-frost-200", collapsed ? "size-11" : "size-10")} />
          {!collapsed && (
            <span className="flex flex-1 flex-col gap-1.5">
              <span className="lx-shimmer h-2.5 w-3/4 rounded-full bg-frost-200" />
              <span className="lx-shimmer h-2 w-1/2 rounded-full bg-frost-100" />
            </span>
          )}
        </li>
      ))}
    </>
  );
}

/* ---------- Sync status (footer) ---------- */

type Badge = { kind: "loading" } | { kind: "unavailable" } | { kind: "ok"; dto: SyncStatusDto };

function describe(b: Badge, go: (v: View) => void, retry: () => void): { text: string; dot: string; action?: { label: string; run: () => void } } {
  if (b.kind === "loading") return { text: "Checking sync", dot: "bg-tertiary-text" };
  if (b.kind === "unavailable") return { text: "Sync status unavailable", dot: "bg-caution-amber", action: { label: "Retry", run: retry } };
  switch (b.dto.state.state) {
    case "synced":
      return { text: "Synced", dot: "bg-synced-blue" };
    case "syncing":
      return { text: "Syncing", dot: "bg-synced-blue" };
    case "offline":
      return { text: "Offline", dot: "bg-caution-amber", action: { label: "Retry", run: retry } };
    case "signedOut":
      return { text: "Signed out", dot: "bg-tertiary-text", action: { label: "Sign in", run: () => go("sync") } };
    case "conflict":
      return { text: "Conflict", dot: "bg-critical-coral", action: { label: "Resolve", run: () => go("sync") } };
    case "rateLimited":
      return { text: "Sync paused", dot: "bg-caution-amber", action: { label: "Retry", run: retry } };
    default:
      return { text: "Sync problem", dot: "bg-critical-coral", action: { label: "Retry", run: retry } };
  }
}

const linkBtn =
  "inline-flex h-8 items-center rounded-[8px] px-2 text-[12px] leading-4 font-medium text-secondary-text transition-colors duration-150 hover:bg-frost-100 hover:text-primary-text";

/* ---------- Sidebar ---------- */

export default function Sidebar() {
  const view = useUi((s) => s.view);
  const navigate = useUi((s) => s.navigate);
  const storedCollapsed = useUi((s) => s.sidebarCollapsed);
  // iPad portrait and Android tablets get the icon rail regardless of the saved desktop preference.
  const collapsed = storedCollapsed || useLayout() === "tablet";
  const toggleSidebar = useUi((s) => s.toggleSidebar);
  const openLibrary = useUi((s) => s.openLibrary);
  const setSelectedPlaylist = useUi((s) => s.setSelectedPlaylist);
  const selectedPlaylistId = useUi((s) => s.selectedPlaylistId);
  const savedTracks = useLibrary(s => s.tracks);
  const playbackLabel = usePlayback((s) => s.requested || s.track ? `YouTube · ${s.player.state}` : s.available ? "YouTube · ready" : "Playback unavailable");
  const { loading, playlists, error, reload } = useLocalPlaylists();
  const tips = useTip(collapsed);

  const [filter, setFilter] = useState<Filter>("all");
  const [focusKey, setFocusKey] = useState<string | null>(null);
  const [order, setOrder] = useState<string[]>([]);
  const [note, setNote] = useState("");
  const [badge, setBadge] = useState<Badge>({ kind: "loading" });
  const [edges, setEdges] = useState({ top: false, bottom: false });
  const hydrated = useRef(false);
  const listRef = useRef<HTMLUListElement>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  const firstView = useRef(true);

  // Per-viewer state: collapsed flag and playlist order. Read once after mount, written on change.
  useEffect(() => {
    if (readStore(COLLAPSED_KEY) === "1" && !useUi.getState().sidebarCollapsed) useUi.getState().toggleSidebar();
    try {
      const raw = readStore(ORDER_KEY);
      const parsed: unknown = raw ? JSON.parse(raw) : [];
      if (Array.isArray(parsed)) setOrder(parsed.filter((x): x is string => typeof x === "string"));
    } catch {
      /* corrupt value: start unordered */
    }
    hydrated.current = true;
  }, []);
  useEffect(() => {
    if (hydrated.current) writeStore(COLLAPSED_KEY, storedCollapsed ? "1" : "0");
  }, [storedCollapsed]);

  const refreshBadge = useCallback(() => {
    syncStatus().then(
      (dto) => setBadge({ kind: "ok", dto }),
      () => setBadge({ kind: "unavailable" }),
    );
  }, []);
  // No polling: re-read when the window regains focus and when the user moves between views.
  useEffect(() => {
    refreshBadge();
    window.addEventListener("focus", refreshBadge);
    return () => window.removeEventListener("focus", refreshBadge);
  }, [refreshBadge]);
  useEffect(() => {
    if (firstView.current) {
      firstView.current = false;
      return;
    }
    reload();
    refreshBadge();
  }, [view, reload, refreshBadge]);

  const ordered = useMemo(() => {
    const rank = (id: string) => {
      const i = order.indexOf(id);
      return i === -1 ? Number.MAX_SAFE_INTEGER : i;
    };
    return [...playlists].sort((a, b) => rank(a.id) - rank(b.id));
  }, [playlists, order]);

  const likedCount = savedTracks.length;

  const items = useMemo<LibItem[]>(() => {
    const out: LibItem[] = [];
    const wantPlaylists = filter === "all" || filter === "playlists";
    if (wantPlaylists) {
      out.push({
        key: "liked",
        kind: "liked",
        title: "Liked Songs",
        subtitle: `Playlist · ${likedCount} ${likedCount === 1 ? "song" : "songs"}`,
        tone: "violet",
        seed: "liked-songs",
        active: view === "library" && useUi.getState().likedOnly,
        playing: false,
        open: () => openLibrary("songs", true),
        play: () => { const first = savedTracks.find(t => t.provider === "YouTube"); if (first) void playSavedTrack(first); },
      });
      for (const p of ordered) {
        out.push({
          key: `p:${p.id}`,
          kind: "playlist",
          title: p.name,
          subtitle: `Playlist · ${p.entryCount} ${p.entryCount === 1 ? "song" : "songs"}`,
          tone: toneFor(p.id),
          seed: p.id,
          active: view === "playlists" && selectedPlaylistId === p.id,
          playing: false,
          open: () => {
            setSelectedPlaylist(p.id);
            navigate("playlists");
          },
          play: () => void playPlaylist(p.id).then((ok) => { if (!ok) setNote(`${p.name} has no YouTube tracks to play.`); }, () => setNote(`Could not play ${p.name}.`)),
          menu: playlistMenu(p, { onOpen: () => { setSelectedPlaylist(p.id); navigate("playlists"); } }),
        });
      }
    }
    if (filter === "all" || filter === "artists") {
      const artists = new Map(savedTracks.flatMap(t => t.artists.map(name => [`${t.provider}:${name}`, { name, provider: t.provider, id: t.artistRefs?.find(a => a.name === name)?.id ?? null }] as const)));
      for (const [key, artist] of artists) out.push({ key: `artist:${key}`, kind: "artist", title: artist.name, subtitle: artist.provider === "YouTube" ? "YouTube Music" : artist.provider, tone: "violet", seed: key, active: false, playing: false, open: () => useUi.getState().openArtist(artist), play: () => useUi.getState().openArtist(artist) });
    }
    return out;
  }, [savedTracks, filter, ordered, likedCount, view, selectedPlaylistId, openLibrary, setSelectedPlaylist, navigate]);

  const tabKey = items.some((i) => i.key === focusKey) ? focusKey : (items[0]?.key ?? null);

  // Fade the list edges only while there is more to scroll.
  const measure = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const top = el.scrollTop > 2;
    const bottom = el.scrollTop + el.clientHeight < el.scrollHeight - 2;
    setEdges((e) => (e.top === top && e.bottom === bottom ? e : { top, bottom }));
  }, []);
  useEffect(() => {
    measure();
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [measure, items.length, collapsed, loading]);

  function move(key: string, dir: -1 | 1) {
    if (!key.startsWith("p:")) {
      setNote("Only your playlists can be reordered.");
      return;
    }
    const id = key.slice(2);
    const ids = ordered.map((p) => p.id);
    const from = ids.indexOf(id);
    const to = from + dir;
    if (from < 0 || to < 0 || to >= ids.length) {
      setNote(dir < 0 ? "Already first." : "Already last.");
      return;
    }
    [ids[from], ids[to]] = [ids[to], ids[from]];
    setOrder(ids);
    writeStore(ORDER_KEY, JSON.stringify(ids));
    setNote(`${ordered[from].name} moved to position ${to + 1} of ${ids.length}.`);
    requestAnimationFrame(() => listRef.current?.querySelector<HTMLElement>(`[data-key="${CSS.escape(key)}"]`)?.focus());
  }

  function onListKey(e: KeyboardEvent<HTMLUListElement>) {
    const row = (e.target as HTMLElement).closest<HTMLElement>("[data-lib-row]");
    if (!row || e.metaKey || e.ctrlKey) return;
    const rows = Array.from(e.currentTarget.querySelectorAll<HTMLElement>("[data-lib-row]"));
    const i = rows.indexOf(row);
    const key = row.dataset.key ?? "";
    const go = (n: number) => {
      e.preventDefault();
      rows[Math.max(0, Math.min(rows.length - 1, n))]?.focus();
    };
    if (e.altKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
      e.preventDefault();
      move(key, e.key === "ArrowUp" ? -1 : 1);
    } else if (e.altKey) {
      return;
    } else if (e.key === "ArrowDown") go(i + 1);
    else if (e.key === "ArrowUp") go(i - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(rows.length - 1);
    else if (e.key === " " || e.key === "p" || e.key === "P") {
      e.preventDefault();
      items.find((it) => it.key === key)?.play();
    }
  }

  function onChipKey(e: KeyboardEvent<HTMLDivElement>) {
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const at = FILTERS.findIndex((f) => f.id === filter);
    const next = FILTERS[(at + (e.key === "ArrowRight" ? 1 : FILTERS.length - 1)) % FILTERS.length];
    setFilter(next.id);
    requestAnimationFrame(() => e.currentTarget.querySelector<HTMLElement>(`[data-chip="${next.id}"]`)?.focus());
  }

  function newPlaylist() {
    setSelectedPlaylist(null);
    navigate("playlists");
    window.setTimeout(() => document.getElementById("editor-new-name")?.focus(), 350);
  }

  function showAll() {
    openLibrary(filter === "all" ? "playlists" : filter);
  }

  const status = describe(badge, navigate, refreshBadge);
  const showEmpty = !loading && !error && playlists.length === 0 && (filter === "all" || filter === "playlists");
  const playlistsPending = loading && playlists.length === 0;
  const failed = !loading && error !== null && (filter === "all" || filter === "playlists");
  const iconBtnActive = (v: View) => (view === v ? "bg-frost-200 text-primary-text" : "");

  return (
    <nav
      aria-label="Primary"
      className={cx(
        "glass flex min-h-0 shrink-0 flex-col overflow-x-hidden px-3 pb-3 transition-[width] duration-[220ms]",
        EASE,
        collapsed ? "w-sidebar-collapsed" : "w-sidebar",
      )}
    >
      {/* 56px header aligned to the toolbar. the row is a window drag handle. */}
      <div data-tauri-drag-region className={cx("flex h-11 shrink-0 items-center", collapsed ? "justify-center" : "justify-between")}>
        {collapsed ? (
          <IconButton label="Expand sidebar" aria-expanded={false} aria-keyshortcuts="Meta+B Control+B" onClick={toggleSidebar}>
            <PanelLeftOpen size={18} />
          </IconButton>
        ) : (
          <>
            <span data-tauri-drag-region className="flex min-w-0 items-center gap-2 text-primary-text">
              <Moon size={18} aria-hidden="true" fill="currentColor" fillOpacity={0.9} className="shrink-0" />
              <span className="truncate text-[14px] leading-5 font-semibold tracking-[-0.01em]">LunoRX</span>
            </span>
            <IconButton label="Collapse sidebar" aria-expanded={true} aria-keyshortcuts="Meta+B Control+B" onClick={toggleSidebar}>
              <PanelLeftClose size={18} />
            </IconButton>
          </>
        )}
      </div>

      <div className="flex flex-col gap-0.5">
        {NAV.map((def) => (
          <NavRow key={def.view} def={def} active={view === def.view} collapsed={collapsed} onSelect={() => navigate(def.view)} bind={tips.bind} />
        ))}
      </div>

      {/* Library panel */}
      <section aria-labelledby="sidebar-library" className="mt-5 flex min-h-0 flex-1 flex-col">
        <div className={cx("flex h-8 shrink-0 items-center", collapsed ? "justify-center" : "justify-between pl-2.5")}>
          {!collapsed && (
            <h2 id="sidebar-library" className="lx-label text-[12px] leading-4 font-semibold text-secondary-text">
              Library
            </h2>
          )}
          {collapsed && <h2 id="sidebar-library" className="sr-only">Library</h2>}
          <div className="flex items-center">
            {!collapsed && (
              <button type="button" onClick={showAll} className={linkBtn} aria-label={filter === "all" ? "Show all in Library" : `Show all ${filter}`}>
                Show all
              </button>
            )}
            <IconButton label="New playlist" onClick={newPlaylist} {...(collapsed ? tips.bind("New playlist") : {})}>
              <Plus size={18} />
            </IconButton>
          </div>
        </div>

        {!collapsed && (
          <div role="radiogroup" aria-label="Filter library" onKeyDown={onChipKey} className="mt-1 flex shrink-0 gap-0.5">
            {FILTERS.map((f) => {
              const on = f.id === filter;
              return (
                <button
                  key={f.id}
                  type="button"
                  role="radio"
                  aria-checked={on}
                  data-chip={f.id}
                  tabIndex={on ? 0 : -1}
                  onClick={() => setFilter(f.id)}
                  className="group flex h-8 min-w-8 items-center justify-center"
                >
                  <span
                    className={cx(
                      "relative isolate flex h-7 items-center rounded-full px-2 text-[12px] leading-4 font-medium transition-colors duration-150",
                      on ? "text-primary-text" : "text-secondary-text group-hover:bg-frost-100 group-hover:text-primary-text",
                    )}
                  >
                    {on && <ActivePill layoutId="chip-pill" radius={9999} className="bg-frost-200" />}
                    {f.label}
                  </span>
                </button>
              );
            })}
          </div>
        )}

        <div
          ref={scrollRef}
          onScroll={() => {
            measure();
            tips.hide();
          }}
          data-top={edges.top ? "1" : undefined}
          data-bottom={edges.bottom ? "1" : undefined}
          className="lx-fade -mx-1 mt-1 min-h-0 flex-1 overflow-y-auto overflow-x-hidden px-1 py-1"
        >
          <ul ref={listRef} role="list" aria-label="Library items" onKeyDown={onListKey} className="flex flex-col gap-0.5">
            {items.map((item) => (
              <Fragment key={item.key}>
                <LibRow item={item} collapsed={collapsed} tabbable={item.key === tabKey} onFocusRow={setFocusKey} bind={tips.bind} />
                {
                  item.key === "liked" && showEmpty && !collapsed ? (
                    <li className="flex flex-col items-start gap-2 px-2.5 py-2">
                      <p className="text-[12px] leading-4 text-secondary-text">Playlists you create show up here.</p>
                      <button
                        type="button"
                        onClick={newPlaylist}
                        className="inline-flex h-8 items-center rounded-[8px] bg-frost-200 px-3 text-[13px] font-medium text-primary-text transition-colors duration-150 hover:bg-frost-300"
                      >
                        Create your first playlist
                      </button>
                    </li>
                  ) : item.key === "liked" && playlistsPending ? (
                    <SkeletonRows collapsed={collapsed} />
                  ) : item.key === "liked" && failed ? (
                    <li className={cx("flex flex-col items-start gap-1 py-2", collapsed ? "items-center" : "px-2.5")}>
                      {!collapsed && <p className="text-[12px] leading-4 text-secondary-text">Could not load playlists</p>}
                      <button type="button" onClick={reload} aria-label="Retry loading playlists" className={linkBtn}>
                        Retry
                      </button>
                    </li>
                  ) : null
                }
              </Fragment>
            ))}
          </ul>
        </div>
      </section>

      {/* Footer: sync status, quick entry points and the one demo indicator. */}
      <div className="mt-2 flex shrink-0 flex-col gap-1">
        {collapsed ? (
          <div className="flex flex-col items-center gap-1">
            <button
              type="button"
              aria-label={`${status.text}. Open Sync`}
              {...tips.bind(status.text, "Open Sync")}
              onClick={() => navigate("sync")}
              className="flex size-9 items-center justify-center rounded-[8px] text-secondary-text hover:bg-frost-100 hover:text-primary-text"
            >
              <span className="relative flex">
                <Cloud size={18} aria-hidden="true" />
                <span aria-hidden="true" className={cx("absolute -right-1 -top-1 size-2 rounded-full", status.dot)} />
              </span>
            </button>
            <span role="status" className="sr-only">{playbackLabel}</span>
            <IconButton label="Connections" aria-current={view === "connections" ? "page" : undefined} onClick={() => navigate("connections")} className={cx("size-9", iconBtnActive("connections"))}>
              <Cable size={18} />
            </IconButton>
            <IconButton label="Settings" aria-current={view === "settings" ? "page" : undefined} onClick={() => navigate("settings")} className={cx("size-9", iconBtnActive("settings"))}>
              <Settings size={18} />
            </IconButton>
          </div>
        ) : (
          <>
            <div className="flex items-center gap-1">
              {status.action ? (
                <button type="button" onClick={status.action.run} title={status.action.label} aria-label={`${status.text}. ${status.action.label}`}
                  className="flex h-9 min-w-0 flex-1 items-center gap-2 rounded-[8px] pl-2.5 text-left text-[13px] leading-[18px] text-primary-text transition-colors duration-150 hover:bg-frost-100">
                  <span aria-hidden="true" className={cx("size-2 shrink-0 rounded-full", status.dot)} />
                  <span className="truncate">{status.text}</span>
                </button>
              ) : (
                <div role="status" className="flex min-w-0 flex-1 items-center gap-2 pl-2.5 text-[13px] leading-[18px] text-primary-text">
                  <span aria-hidden="true" className={cx("size-2 shrink-0 rounded-full", status.dot)} />
                  <span className="truncate">{status.text}</span>
                </div>
              )}
              <IconButton label="Connections" aria-current={view === "connections" ? "page" : undefined} onClick={() => navigate("connections")} className={cx("size-9", iconBtnActive("connections"))}>
                <Cable size={18} />
              </IconButton>
              <IconButton label="Settings" aria-current={view === "settings" ? "page" : undefined} onClick={() => navigate("settings")} className={cx("size-9", iconBtnActive("settings"))}>
                <Settings size={18} />
              </IconButton>
            </div>
            <div role="status" title={playbackLabel} aria-label={playbackLabel} className="px-2.5 pt-0.5 text-[11px] leading-[14px] text-secondary-text">
              {playbackLabel}
            </div>
          </>
        )}
      </div>

      <p role="status" aria-live="polite" className="sr-only">
        {note}
      </p>
      {tips.node}
    </nav>
  );
}
