"use client";
import { invalidateHomeFeed } from "./discovery";
import { create } from "zustand";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SearchHitDto } from "../services/tauri/search";
export const useHistory = create<{ tracks: SearchHitDto[]; error: string }>(() => ({ tracks: [], error: "" }));
let version = 0;
export async function refreshHistory() {
  const request = ++version;
  try { const tracks = await invoke<SearchHitDto[]>("library_history"); if (request === version) useHistory.setState({ tracks, error: "" }); }
  catch { if (request === version) useHistory.setState({ error: "Could not read listening history. Check local storage and retry." }); }
}
export async function clearHistory() {
  ++version;
  try { await invoke<SearchHitDto[]>("library_clear_history"); await refreshHistory(); invalidateHomeFeed(); }
  catch { useHistory.setState({ error: "Could not clear listening history. Check local storage and retry." }); }
}
export function initHistory() {
  let active = true;
  const stops: UnlistenFn[] = [];
  const install = (stop: UnlistenFn) => { if (active) stops.push(stop); else stop(); };
  listen<SearchHitDto[]>("core://history/changed", e => { if (active) { ++version; useHistory.setState({ tracks: e.payload, error: "" }); invalidateHomeFeed(); } }).then(install).then(() => { if (active) void refreshHistory(); }).catch(() => { if (active) void refreshHistory(); });
  listen("core://history/error", () => { if (active) useHistory.setState({ error: "Could not save listening history. Check free disk space and storage access." }); }).then(install).catch(() => {});
  return () => { active = false; stops.forEach(stop => stop()); };
}
