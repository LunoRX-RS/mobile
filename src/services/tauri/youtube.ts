import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export type YouTubeConnectionStatus = "restoring" | "disconnected" | "connecting" | "connected" | "expired" | "storeError" | "unavailable";
export function youtubeConnectionStatus(): Promise<YouTubeConnectionStatus> { return invoke("youtube_connection_status"); }
export function youtubeConnect(): Promise<YouTubeConnectionStatus> { return invoke("youtube_connect"); }
export function youtubeConnectFinish(): Promise<YouTubeConnectionStatus> { return invoke("youtube_connect_finish"); }
export function youtubeDisconnect(): Promise<YouTubeConnectionStatus> { return invoke("youtube_disconnect"); }
export function subscribeYouTubeConnection(callback: (status: YouTubeConnectionStatus) => void): Promise<UnlistenFn> {
  return listen<YouTubeConnectionStatus>("core://youtube/connection", (event) => callback(event.payload));
}

export interface ImportReport {
  likedAdded: number; likedRemoved: number; historyAdded: number; playlistsCreated: number; playlistsUpdated: number; playlistTracksAdded: number;
  /** Sections that could not be read or stored: "likes", "playlists", "history". */
  failed: ("likes" | "playlists" | "history" | "ytLikes" | "ytPlaylists" | "ytHistory")[];
}
export type ImportEvent = { state: "running" } | { state: "done"; report: ImportReport } | { state: "failed" };
/** Imports the YouTube Music collection now (read-only toward YouTube). Rejects with "inUse" while another run is going. */
/** Imports one YouTube or YouTube Music playlist (a link or an id) into the local library; resolves with the new local playlist id. */
export function youtubeImportPlaylistFromUrl(url: string): Promise<string> { return invoke("youtube_import_playlist_from_url", { url }); }
export function youtubeImport(): Promise<ImportReport> { return invoke("youtube_import"); }
export function subscribeYouTubeImport(callback: (event: ImportEvent) => void): Promise<UnlistenFn> {
  return listen<ImportEvent>("core://youtube/import", (event) => callback(event.payload));
}
