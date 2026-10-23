"use client";

import { useEffect } from "react";
import { create } from "zustand";
import { lrcLoad, type LrcLineDto } from "../services/tauri/lrc";
import type { LyricsSourceDto } from "../services/tauri/player";

/** The user's own file for a track: what the "My LRC" lyrics source reads. Held in memory; the file on disk is the truth. */
export interface MyLrcEntry { exists: boolean; path: string | null; lines: LrcLineDto[] }

interface MyLrcStore {
  byTrack: Record<string, MyLrcEntry | undefined>;
  set: (trackId: string, entry: MyLrcEntry) => void;
}

export const useMyLrcStore = create<MyLrcStore>((set) => ({
  byTrack: {},
  set: (trackId, entry) => set((s) => ({ byTrack: { ...s.byTrack, [trackId]: entry } })),
}));

const pending = new Set<string>();

/** Asks core once per track (and again on `refresh`). Outside the app, or on any error, the track simply has no file. */
export async function refreshMyLrc(trackId: string): Promise<void> {
  if (pending.has(trackId)) return;
  pending.add(trackId);
  try {
    const dto = await lrcLoad(trackId);
    useMyLrcStore.getState().set(trackId, { exists: dto.exists && dto.lines.length > 0, path: dto.path, lines: dto.lines });
  } catch {
    if (!useMyLrcStore.getState().byTrack[trackId]) useMyLrcStore.getState().set(trackId, { exists: false, path: null, lines: [] });
  } finally { pending.delete(trackId); }
}

export function useMyLrc(trackId: string | undefined): MyLrcEntry | null {
  const entry = useMyLrcStore((s) => (trackId ? s.byTrack[trackId] : undefined));
  useEffect(() => { if (trackId && !useMyLrcStore.getState().byTrack[trackId]) void refreshMyLrc(trackId); }, [trackId]);
  return entry ?? null;
}

export const basename = (path: string | null): string => (path ? path.split(/[\\/]/).pop() ?? path : "");

/** The "My LRC" candidate for the source picker. Always present so the picker can explain why it is off. */
export function mineSource(entry: MyLrcEntry | null): LyricsSourceDto {
  const ok = !!entry && entry.exists && entry.lines.length > 0;
  return {
    id: "mine", label: "My LRC", available: ok, synced: ok,
    text: ok ? entry.lines.map((l) => l.text).join("\n") : null,
    lines: ok ? entry.lines : null,
    attribution: ok ? `My LRC · ${basename(entry.path) || "made by you"}` : null,
    status: ok ? "available" : "noMatch",
  };
}
