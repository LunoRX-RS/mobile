"use client";
import { create } from "zustand";
import { libraryLikes, librarySetLike } from "../services/tauri/library";
import { warmArtistPhotos } from "./artistPhotos";
import type { SearchHitDto } from "../services/tauri/search";
export const trackKey = (t: Pick<SearchHitDto, "provider" | "providerTrackId">) => `${t.provider}:${t.providerTrackId}`;
// One row per provider track: duplicate ids made React report duplicate keys in every list built from this store.
const unique = (tracks: SearchHitDto[]) => { const seen = new Set<string>(); return tracks.filter(t => { const k = trackKey(t); if (seen.has(k)) return false; seen.add(k); return true; }); };
export const useLibrary = create<{ tracks: SearchHitDto[]; ready: boolean; busy: boolean; error: string }>(() => ({ tracks: [], ready: false, busy: false, error: "" }));
let version = 0;
export async function refreshLikes() {
  const request = ++version;
  try { const tracks = await libraryLikes(); if (version === request) { const list = unique(tracks); useLibrary.setState({ tracks: list, ready: true, error: "" }); warmArtistPhotos(likedArtists(list)); } }
  catch (error) { if (version === request) useLibrary.setState({ error: error === "inUse" ? "Liked songs are open in another LUNOR-X instance. Close it, then retry." : "Could not read liked songs. Check local storage and retry." }); }
}
export async function toggleSaved(track: SearchHitDto) {
  const state = useLibrary.getState();
  if (!state.ready || state.busy) return;
  ++version;
  useLibrary.setState({ busy: true, error: "" });
  try {
    const liked = !state.tracks.some(t => trackKey(t) === trackKey(track));
    const tracks = await librarySetLike({ ...track, artistRefs: track.artistRefs ?? [] }, liked);
    ++version;
    useLibrary.setState({ tracks: unique(tracks) });
  } catch { useLibrary.setState({ error: "Could not save this like. Check free disk space and retry." }); }
  finally { useLibrary.setState({ busy: false }); }
}

/** Distinct artists with a profile id across the liked songs, in the order the likes list them. */
export function likedArtists(tracks: SearchHitDto[]) {
  const seen = new Map<string, NonNullable<SearchHitDto["artistRefs"]>[number]>();
  for (const t of tracks) for (const a of t.artistRefs ?? []) if (a.id && a.provider === t.provider) seen.set(`${a.provider}:${a.id}`, a);
  return seen.values();
}
