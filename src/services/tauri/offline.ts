import { invoke } from "@tauri-apps/api/core";
export interface OfflineTrack { provider: string; id: string; title: string; artists: string[]; artworkUrl: string | null; durationMs: number | null }
export interface OfflineStatus { keeping: boolean; running: boolean; changed: boolean; tracks: { track: OfflineTrack; error: string | null; state: "ready" | "pending" | "failed" | "unsupported" }[]; usedBytes: number; pinnedBytes: number; limitBytes: number }
export const offlineStatus = (playlistId = "", artworkStart = 0) => invoke<OfflineStatus>("offline_status", { playlistId, artworkStart });
export const offlineKeep = (playlistId: string) => invoke<void>("offline_keep", { playlistId });
export const offlineCancel = (playlistId: string) => invoke<void>("offline_cancel", { playlistId });
export const offlineRemove = (playlistId: string) => invoke<void>("offline_remove", { playlistId });
export const offlineSetLimit = (bytes: number) => invoke<void>("offline_set_limit", { bytes });
/** One playlist kept offline. `bytes` includes songs it shares with another playlist. */
export interface PlaylistUsage { id: string; name: string; tracks: number; bytes: number }
export interface CacheUsage {
  audioBytes: number; artworkBytes: number; lyricBytes: number; totalBytes: number;
  limitBytes: number; clearableBytes: number; sharedBytes: number; playlists: PlaylistUsage[];
}
export const offlineUsage = () => invoke<CacheUsage>("offline_usage");
/** Frees cached songs that no offline playlist keeps and the player is not using. Resolves with the bytes released. */
export const offlineClearTransient = () => invoke<number>("offline_clear_transient");
export function offlineError(e: unknown): string {
  switch (e) {
    case "tooLarge": return "This audio file exceeds the 64 MB per-song limit.";
    case "full": return "Not enough cache space. Raise the limit or remove offline playlists. Playing and pinned songs are protected.";
    case "inUse": return "Offline storage is in use by another LUNOR-X instance. Close that instance first.";
    case "busy": return "Another playlist is downloading. Finish or cancel it first.";
    case "storage": return "Could not access offline storage. Check free disk space and file permissions.";
    case "download": return "Could not download this song. Try again when YouTube is available.";
    case "mediaRefused": return "YouTube refused the audio bytes. Sign in with Google in Connections, then try again.";
    case "verificationRequired": return "YouTube asked for verification. Sign in with Google in Connections.";
    case "validation": return "This playlist cannot be downloaded.";
    default: return "Offline storage is unavailable.";
  }
}
