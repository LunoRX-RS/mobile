"use client";

import { create } from "zustand";
import type { QueueTrack } from "./media";

// UI state only. Real playback lives in lib/playback; the queue below is empty until something is loaded into it.

export type View =
  | "home"
  | "explore"
  | "search"
  | "library"
  | "playlists"
  | "player"
  | "connections"
  | "accounts"
  | "login"
  | "settings"
  | "sync"
  | "stats"
  | "replay"
  | "shared"
  | "artist"
  | "album";

export type LibraryTab = "artists" | "songs" | "playlists";
export type RepeatMode = "off" | "all" | "one";

interface UiState {
  selectedArtist: import("../services/tauri/search").ArtistRef | null;
  selectedAlbum: { provider: string; id: string; name: string; artworkUrl: string | null; year: number | null } | null;
  openAlbum: (album: NonNullable<UiState["selectedAlbum"]>) => void;
  /** A playlist id the user asked to delete from a menu; the editor opens its confirmation for it. */
  deleteRequest: string | null;
  requestDelete: (id: string | null) => void;
  openArtist: (artist: import("../services/tauri/search").ArtistRef) => void;
  view: View;
  back: View[];
  forward: View[];
  sidebarCollapsed: boolean;
  paletteOpen: boolean;
  nowPlayingOpen: boolean;
  lyricsOpen: boolean;
  queueOpen: boolean;
  /** The queue shown over the lyrics inside Now Playing. */
  npQueue: boolean;
  /** Phone Now Playing: the drawer that is open over the player (menu, queue or sleep timer). */
  npPanel: null | "menu" | "queue" | "timer";
  /** Phone only: Settings shows one category (true) or the list of categories (false). */
  settingsDetail: boolean;
  selectedPlaylistId: string | null;
  /** The token of the shared playlist on the "shared" view. */
  sharedToken: string | null;
  openShared: (token: string) => void;
  libraryTab: LibraryTab;
  likedOnly: boolean;
  query: string;
  pendingSearch: boolean;
  recentSearches: string[];

  // Demo playback (simulated UI state only).
  queue: QueueTrack[];
  index: number;
  playing: boolean;
  positionSec: number;
  volume: number;
  shuffle: boolean;
  repeat: RepeatMode;
  liked: Record<string, true>;

  navigate: (view: View) => void;
  goBack: () => void;
  goForward: () => void;
  openLibrary: (tab: LibraryTab, likedOnly?: boolean) => void;
  setSelectedPlaylist: (id: string | null) => void;
  toggleSidebar: () => void;
  setPalette: (open: boolean) => void;
  setNowPlaying: (open: boolean) => void;
  setLyrics: (open: boolean) => void;
  toggleQueue: () => void;
  setQuery: (q: string) => void;
  searchFor: (q: string) => void;
  pushRecentSearch: (q: string) => void;
  clearRecentSearches: () => void;
  removeRecentSearch: (query: string) => void;
  playIndex: (i: number) => void;
  togglePlay: () => void;
  next: () => void;
  prev: () => void;
  seek: (sec: number) => void;
  tick: () => void;
  setVolume: (v: number) => void;
  toggleShuffle: () => void;
  cycleRepeat: () => void;
  toggleLike: (id: string) => void;
}

const RECENT_LIMIT = 20;
const RECENT_KEY = "lunorx.recentSearches";
function saveRecent(values: string[]) { try { window.localStorage.setItem(RECENT_KEY,JSON.stringify(values)); } catch { /* Keep session history when disk storage is blocked. */ } }
export function hydrateRecentSearches() {
  try { const parsed:unknown=JSON.parse(window.localStorage.getItem(RECENT_KEY) ?? "[]");if(Array.isArray(parsed)) useUi.setState({recentSearches:Array.from(new Set(parsed.filter((s):s is string=>typeof s==="string" && s.trim().length>0 && s.length<=256))).slice(0,RECENT_LIMIT)}); } catch { /* Ignore corrupt browser data. */ }
}

export const useUi = create<UiState>()((set, get) => ({
  selectedArtist: null,
  selectedAlbum: null,
  deleteRequest: null,
  requestDelete: (id) => { if (id) { set({ selectedPlaylistId: id, deleteRequest: id }); get().navigate("playlists"); } else set({ deleteRequest: null }); },
  openAlbum: (album) => { set({ selectedAlbum: album, nowPlayingOpen: false }); get().navigate("album"); },
  openArtist: (artist) => { set({ selectedArtist: artist, nowPlayingOpen: false }); get().navigate("artist"); },
  view: "home",
  back: [],
  forward: [],
  sidebarCollapsed: false,
  paletteOpen: false,
  nowPlayingOpen: false,
  lyricsOpen: false,
  queueOpen: false,
  npQueue: false,
  npPanel: null,
  settingsDetail: false,
  selectedPlaylistId: null,
  sharedToken: null,
  openShared: (token) => {
    set({ sharedToken: token });
    get().navigate("shared");
  },
  libraryTab: "playlists",
  likedOnly: false,
  query: "",
  pendingSearch: false,
  recentSearches: [],

  queue: [],
  // -1: nothing is loaded until the user plays something. No track is pre-selected at launch.
  index: -1,
  playing: false,
  positionSec: 0,
  volume: 100,
  shuffle: false,
  repeat: "off",
  liked: {},

  navigate: (view) => {
    const { view: current, back } = get();
    if (view === current) return;
    set({ view, back: [...back, current], forward: [] });
  },
  goBack: () => {
    const { back, view, forward } = get();
    const prev = back[back.length - 1];
    if (!prev) return;
    set({ view: prev, back: back.slice(0, -1), forward: [...forward, view] });
  },
  goForward: () => {
    const { forward, view, back } = get();
    const nextView = forward[forward.length - 1];
    if (!nextView) return;
    set({ view: nextView, forward: forward.slice(0, -1), back: [...back, view] });
  },
  openLibrary: (tab, likedOnly = false) => {
    set({ libraryTab: tab, likedOnly });
    get().navigate("library");
  },
  setSelectedPlaylist: (id) => set({ selectedPlaylistId: id }),
  toggleSidebar: () => set((s) => ({ sidebarCollapsed: !s.sidebarCollapsed })),
  setPalette: (open) => set({ paletteOpen: open }),
  setNowPlaying: (open) => set({ nowPlayingOpen: open, lyricsOpen: open ? get().lyricsOpen : false, npQueue: open ? get().npQueue : false, npPanel: open ? get().npPanel : null }),
  setLyrics: (open) => set({ lyricsOpen: open }),
  toggleQueue: () => set((s) => ({ queueOpen: !s.queueOpen })),
  setQuery: (q) => set({ query: q }),
  // The command palette opens the Search view and run the query once.
  searchFor: (q) => {
    set({ query: q, pendingSearch: true });
    get().navigate("search");
  },
  pushRecentSearch: (q) => {
    const trimmed = q.trim();
    if (trimmed === "" || trimmed.length > 256) return;
    const rest = get().recentSearches.filter((x) => x !== trimmed);
    const recentSearches = [trimmed,...rest].slice(0,RECENT_LIMIT);
    saveRecent(recentSearches);set({recentSearches});
  },
  clearRecentSearches: () => {saveRecent([]);set({recentSearches:[]});},
  removeRecentSearch: (query) => {const recentSearches=get().recentSearches.filter(s=>s!==query);saveRecent(recentSearches);set({recentSearches});},

  playIndex: (i) => {
    const { queue } = get();
    if (i < 0 || i >= queue.length) return;
    set({ index: i, playing: true, positionSec: 0 });
  },
  togglePlay: () => set((s) => ({ playing: !s.playing })),
  next: () => {
    const { queue, index, shuffle, repeat } = get();
    if (queue.length === 0) return;
    if (shuffle && queue.length > 1) {
      let pick = index;
      while (pick === index) pick = Math.floor(Math.random() * queue.length);
      set({ index: pick, positionSec: 0 });
      return;
    }
    if (index + 1 < queue.length) {
      set({ index: index + 1, positionSec: 0 });
    } else if (repeat === "all") {
      set({ index: 0, positionSec: 0 });
    } else {
      set({ playing: false, positionSec: 0 });
    }
  },
  prev: () => {
    const { index, positionSec, queue } = get();
    if (positionSec > 3) {
      set({ positionSec: 0 });
      return;
    }
    set({ index: index > 0 ? index - 1 : queue.length - 1, positionSec: 0 });
  },
  seek: (sec) => {
    const dur = get().queue[get().index]?.durationSec ?? 0;
    set({ positionSec: Math.min(Math.max(0, sec), dur) });
  },
  tick: () => {
    const { playing, positionSec, queue, index, repeat } = get();
    if (!playing) return;
    const dur = queue[index]?.durationSec ?? 0;
    if (positionSec + 1 >= dur) {
      if (repeat === "one") set({ positionSec: 0 });
      else get().next();
      return;
    }
    set({ positionSec: positionSec + 1 });
  },
  setVolume: (v) => set({ volume: Math.min(100, Math.max(0, Math.round(v))) }),
  toggleShuffle: () => set((s) => ({ shuffle: !s.shuffle })),
  cycleRepeat: () =>
    set((s) => ({ repeat: s.repeat === "off" ? "all" : s.repeat === "all" ? "one" : "off" })),
  toggleLike: (id) =>
    set((s) => {
      const liked = { ...s.liked };
      if (liked[id]) delete liked[id];
      else liked[id] = true;
      return { liked };
    }),
}));
