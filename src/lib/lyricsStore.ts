"use client";

import { create } from "zustand";
import { lyricsSettingsGet, lyricsSettingsSet, type LyricsSettingsDto } from "../services/tauri/lyricsSources";
import type { LyricsSourceDto } from "../services/tauri/player";
import { normalizeOrder } from "./lyricsSource";

// Two things core owns that the window mirrors: the Auto order / on-off settings, and source entries replaced after
// "Try again", a hand-picked match or an unbind (they win over what Track info delivered for that track).

type LoadState = "idle" | "loading" | "ready" | "error";
type SaveState = "idle" | "saving" | "error";

interface LyricsStore {
  settings: LyricsSettingsDto | null;
  loadState: LoadState;
  saveState: SaveState;
  /** What the last failed save wanted, so Retry can send it again. */
  wanted: { order: string[]; disabled: string[] } | null;
  overrides: Record<string, Record<string, LyricsSourceDto> | undefined>;
  setOverride: (trackId: string, entry: LyricsSourceDto) => void;
}

export const useLyricsStore = create<LyricsStore>((set) => ({
  settings: null,
  loadState: "idle",
  saveState: "idle",
  wanted: null,
  overrides: {},
  setOverride: (trackId, entry) => set((s) => ({ overrides: { ...s.overrides, [trackId]: { ...s.overrides[trackId], [entry.id]: entry } } })),
}));

const tidy = (dto: LyricsSettingsDto): LyricsSettingsDto => ({
  sources: Array.isArray(dto.sources) ? dto.sources : [],
  order: normalizeOrder(Array.isArray(dto.order) ? dto.order : [], Array.isArray(dto.sources) ? dto.sources : []),
  disabled: Array.isArray(dto.disabled) ? dto.disabled : [],
});

/** Reads the settings once; `force` reads again (Retry). */
export async function loadLyricsSettings(force = false): Promise<void> {
  const { loadState } = useLyricsStore.getState();
  if (!force && (loadState === "loading" || loadState === "ready")) return;
  useLyricsStore.setState({ loadState: "loading" });
  try {
    useLyricsStore.setState({ settings: tidy(await lyricsSettingsGet()), loadState: "ready" });
  } catch {
    useLyricsStore.setState({ loadState: "error" });
  }
}

let saveToken = 0;

/** Optimistic: the list changes at once. On failure it goes back to what core last confirmed and `saveState` is "error". */
export async function saveLyricsSettings(next: { order: string[]; disabled: string[] }): Promise<void> {
  const before = useLyricsStore.getState().settings;
  if (!before) return;
  const token = ++saveToken;
  useLyricsStore.setState({ settings: { ...before, ...next }, saveState: "saving", wanted: next });
  try {
    const saved = tidy(await lyricsSettingsSet(next.order, next.disabled));
    if (token === saveToken) useLyricsStore.setState({ settings: saved, saveState: "idle", wanted: null });
  } catch {
    if (token === saveToken) useLyricsStore.setState({ settings: before, saveState: "error" });
  }
}
