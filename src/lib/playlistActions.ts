"use client";
import { offlineKeep } from "../services/tauri/offline";
import { syncListLocalPlaylists, syncMoveLocalEntry, syncPlaylistEntries, syncAddLocalEntry, syncRemoveLocalEntry, type LocalEntryDto, type LocalPlaylistDto, type Provider } from "../services/tauri/sync";
import type { SearchHitDto } from "../services/tauri/search";
import { playTrack } from "./playback";

const toHit = (e: LocalEntryDto): SearchHitDto => ({
  provider: e.provider, providerTrackId: e.providerTrackId, canonicalUrl: e.providerUrl, title: e.title ?? e.providerTrackId,
  artists: e.artists ?? [], artworkUrl: e.artworkUrl, durationMs: e.durationMs,
});
export const entryToHit = toHit;

/** Entries a hit can be stored as. Spotify entries keep ids only. */
export function hitToEntry(hit: SearchHitDto): LocalEntryDto {
  const spotify = (hit.provider as Provider) === "Spotify";
  return {
    provider: hit.provider as Provider, providerTrackId: hit.providerTrackId, providerUrl: hit.canonicalUrl,
    title: spotify ? null : hit.title, artists: spotify ? null : hit.artists, artworkUrl: spotify ? null : hit.artworkUrl, durationMs: spotify ? null : hit.durationMs,
  };
}

/** Starts the first YouTube track of a playlist and queues the rest. Resolves false when nothing is playable. */
export async function playPlaylist(id: string): Promise<boolean> {
  const tracks = (await syncPlaylistEntries(id)).map(toHit).filter((t) => t.provider === "YouTube");
  if (tracks.length === 0) return false;
  await playTrack(tracks[0], false, tracks.slice(1));
  return true;
}

export const downloadPlaylist = (id: string) => offlineKeep(id);
export const listPlaylists = (): Promise<LocalPlaylistDto[]> => syncListLocalPlaylists();
export const addToPlaylist = (playlistId: string, hit: SearchHitDto) => syncAddLocalEntry(playlistId, hitToEntry(hit));
export const moveInPlaylist = (playlistId: string, from: number, to: number) => syncMoveLocalEntry(playlistId, from, to);
export const removeFromPlaylist = (playlistId: string, index: number) => syncRemoveLocalEntry(playlistId, index);
