import { invoke } from "@tauri-apps/api/core";
import { syncAddLocalEntry, syncCreateLocalPlaylist, syncUploadPlaylist, type LocalEntryDto } from "./sync";

/** Emitted by the Rust side with the share token when a `lunorx://share/{token}` link opens the app. */
export const SHARE_OPEN_EVENT = "core://share/open";

export interface SharedEntryDto { provider: "Spotify" | "YouTube"; providerTrackId: string; providerUrl: string; title: string; artists: string[]; artworkUrl: string | null; durationMs: number | null }
export interface SharedPlaylistDto { name: string; description: string | null; entries: SharedEntryDto[] }

/** The token from a link that arrived before the window listened; null when there is none. */
export const sharedLinkTake = () => invoke<string | null>("shared_link_take");
export const sharedTokenFromUrl = (raw: string) => invoke<string | null>("shared_token_from_url", { raw });
export const sharedPlaylistGet = (token: string) => invoke<SharedPlaylistDto>("shared_playlist_get", { token });

/**
 * Copies a shared playlist into this device's library and uploads it, so it syncs like any other playlist.
 * Needs a signed-in account. Returns the new local playlist id; play and download then use the normal playlist page.
 */
export async function saveSharedPlaylist(playlist: SharedPlaylistDto): Promise<string> {
  const localId = await syncCreateLocalPlaylist(playlist.name, playlist.description);
  for (const e of playlist.entries) {
    const entry: LocalEntryDto = {
      provider: e.provider,
      providerTrackId: e.providerTrackId,
      providerUrl: e.providerUrl,
      title: e.title,
      artists: e.artists,
      artworkUrl: e.artworkUrl,
      durationMs: e.durationMs,
    };
    await syncAddLocalEntry(localId, entry);
  }
  await syncUploadPlaylist(localId);
  return localId;
}
