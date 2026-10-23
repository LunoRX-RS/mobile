"use client";
import { create } from "zustand";
import { subscribeYouTubeImport, type ImportEvent, type ImportReport } from "../services/tauri/youtube";
import { refreshLikes } from "./library";

interface ImportState { state: "idle" | "running" | "done" | "failed"; report: ImportReport | null; finished: number }
export const useImport = create<ImportState>(() => ({ state: "idle", report: null, finished: 0 }));

export function applyImportEvent(event: ImportEvent) {
  if (event.state === "running") { useImport.setState({ state: "running" }); return; }
  if (event.state === "failed") { useImport.setState({ state: "failed" }); return; }
  useImport.setState((s) => ({ state: "done", report: event.report, finished: s.finished + 1 }));
  // New likes, history and playlists landed on the device: redraw everything that shows them.
  void refreshLikes();
}

/** Listens for import progress for the life of the app. Returns the cleanup. */
export function initYouTubeImport(): () => void {
  let stop: (() => void) | undefined;
  let disposed = false;
  subscribeYouTubeImport(applyImportEvent).then((fn) => { if (disposed) fn(); else stop = fn; }, () => {});
  return () => { disposed = true; stop?.(); };
}

export function importSummary(r: ImportReport): string {
  const parts: string[] = [];
  if (r.likedAdded) parts.push(`${r.likedAdded} liked ${r.likedAdded === 1 ? "song" : "songs"}`);
  if (r.likedRemoved) parts.push(`${r.likedRemoved} un-liked on YouTube Music`);
  if (r.playlistsCreated) parts.push(`${r.playlistsCreated} new ${r.playlistsCreated === 1 ? "playlist" : "playlists"}`);
  if (r.playlistsUpdated) parts.push(`${r.playlistsUpdated} updated ${r.playlistsUpdated === 1 ? "playlist" : "playlists"}`);
  if (r.historyAdded) parts.push(`${r.historyAdded} from history`);
  const base = parts.length ? `Imported ${parts.join(", ")}.` : "Already up to date.";
  const names = { likes: "liked songs", playlists: "playlists", history: "history", ytLikes: "YouTube liked videos", ytPlaylists: "YouTube playlists", ytHistory: "YouTube history" } as const;
  return r.failed.length ? `${base} Could not read: ${r.failed.map((f) => names[f]).join(", ")}.` : base;
}
