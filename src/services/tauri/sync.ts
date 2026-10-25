import { invoke } from "@tauri-apps/api/core";

// Mirrors commands/sync.rs and sync/engine.rs. Tauri maps snake_case Rust argument names to camelCase JS keys.
// Errors cross IPC as the bare category string; no message, token or password ever reaches the frontend.

export type ErrorCategory =
  | "validation"
  | "unauthenticated"
  | "conflict"
  | "rateLimited"
  | "offline"
  | "storage"
  | "queueFull"
  | "internal";

export type SyncState =
  | { state: "signedOut" }
  | { state: "syncing" }
  | { state: "synced" }
  | { state: "offline" }
  | { state: "rateLimited"; retryAfterSecs: number | null }
  | { state: "conflict"; playlistId: string }
  | { state: "error"; kind: ErrorCategory };

export interface SignedInDto {
  userId: string | null;
  expiresAt: string;
}

export interface SignOutDto {
  warning: ErrorCategory | null;
}

export interface SyncStatusDto {
  state: SyncState;
  pendingWrites: number;
  conflicts: string[];
}

export type Provider = "Spotify" | "YouTube";

export interface LocalEntryDto {
  provider: Provider;
  providerTrackId: string;
  providerUrl: string;
  title: string | null;
  artists: string[] | null;
  artworkUrl: string | null;
  durationMs: number | null;
}

export interface LocalPlaylistDto {
  id: string;
  name: string;
  remoteId: string | null;
  entryCount: number;
  // Entries the server has accepted; fewer than entryCount means a partial upload that can resume.
  uploaded: number;
  // Queued writes for this playlist alone; they are not on the server yet.
  queuedWrites: number;
  // Entries past the server-held and queued ones: the unsent partial-upload tail.
  unsentEntries: number;
  /** The note under the title; null for none. An import marker is never shown here. */
  description: string | null;
}

export type ResolutionDto = "keepServer" | "keepMine";

/** Where a local playlist stands on the server, from `sync_check_playlists`. */
export type RemoteState = "notUploaded" | "missingOnServer" | "serverNewer" | "localChanges" | "inSync";

/** Asks the server about every local playlist. Read-only: uploads nothing. Rejects when signed out or offline. */
export function syncCheckPlaylists(): Promise<{ localId: string; state: RemoteState }[]> {
  return invoke("sync_check_playlists");
}

export function syncListLocalPlaylists(): Promise<LocalPlaylistDto[]> {
  return invoke<LocalPlaylistDto[]>("sync_list_local_playlists");
}

// Index is the entry's position in the local playlist.
export function syncRemoveLocalEntry(localId: string, index: number): Promise<void> {
  return invoke<void>("sync_remove_local_entry", { localId, index });
}

// Returns how many queued writes the server accepted.
export function syncFlushPending(): Promise<number> {
  return invoke<number>("sync_flush_pending");
}

/** Fired after sign-in or sign-out so the remote-control link can start or stop. */
export const ACCOUNT_CHANGED = "lunorx:account-changed";
const accountChanged = () => window.dispatchEvent(new Event(ACCOUNT_CHANGED));
// One reload after sign-in or sign-out, so every screen starts from the new account's data. The short delay lets the
// caller finish its own update first; the reload comes from a user action, so it cannot loop.
const reloadOnce = () => { window.setTimeout(() => window.location.reload(), 150); };

/** Creates an account and signs in with it. Rejects with "validation" for a bad email or a password outside 12 to 128 bytes. */
export async function syncRegister(email: string, password: string, antibot: string[] = []): Promise<SignedInDto> {
  // Turnstile tokens are single use: [0] is for creating the account, [1] for the sign-in that follows.
  const done = await invoke<SignedInDto>("sync_register", { email, password, registerToken: antibot[0], loginToken: antibot[1] });
  accountChanged();
  reloadOnce();
  return done;
}

/** A read-only share link. `url` contains the token: show it to the user, do not log it. */
export interface ShareLinkDto {
  id: string;
  url: string;
}

export interface ShareDto {
  id: string;
  createdAt: string;
  active: boolean;
  /** The live link, rebuilt from the token the server keeps for the owner. Contains the token: show it, never log it. */
  url: string | null;
}

/** Makes a read-only link for a playlist that is on the server. Rejects with "unauthenticated" when signed out. */
export function syncShare(localId: string): Promise<ShareLinkDto> {
  return invoke<ShareLinkDto>("sync_share_playlist", { localId });
}

export function syncListShares(localId: string): Promise<ShareDto[]> {
  return invoke<ShareDto[]>("sync_list_shares", { localId });
}

export function syncRevokeShare(localId: string, shareId: string): Promise<void> {
  return invoke<void>("sync_revoke_share", { localId, shareId });
}

export async function syncSignIn(email: string, password: string, antibotToken?: string): Promise<SignedInDto> {
  const done = await invoke<SignedInDto>("sync_sign_in", { email, password, token: antibotToken });
  accountChanged();
  reloadOnce();
  return done;
}

export async function syncSignOut(): Promise<SignOutDto> {
  const done = await invoke<SignOutDto>("sync_sign_out");
  accountChanged();
  // A warning ("the server did not confirm") is shown on screen, so a reload would hide it.
  if (!done.warning) reloadOnce();
  return done;
}

// Returns how many queued writes were dropped.
export function syncDiscardPending(): Promise<number> {
  return invoke<number>("sync_discard_pending");
}

export function syncStatus(): Promise<SyncStatusDto> {
  return invoke<SyncStatusDto>("sync_status");
}

// Returns the local playlist id.
export function syncCreateLocalPlaylist(name: string, description: string | null): Promise<string> {
  return invoke<string>("sync_create_local_playlist", { name, description });
}

/** The playlist's entries in order, as this device holds them. */
export function syncPlaylistEntries(localId: string): Promise<LocalEntryDto[]> {
  return invoke<LocalEntryDto[]>("sync_playlist_entries", { localId });
}

/** Deletes a playlist that is not on the server. Rejects with "validation" for an uploaded one. */
export function syncDeleteLocalPlaylist(localId: string): Promise<void> {
  return invoke<void>("sync_delete_local_playlist", { localId });
}

/** Renames a playlist that is not on the server. Rejects with "validation" for an uploaded one or a bad name. */
export function syncRenameLocalPlaylist(localId: string, name: string): Promise<void> {
  return invoke<void>("sync_rename_local_playlist", { localId, name });
}

/** Sets (or, with null or empty text, clears) a playlist's description. An uploaded playlist changes on the server first,
 *  so it needs a connection: rejects with "offline", "conflict" (changed elsewhere) or "validation" (text over 2000
 *  characters, or changes are waiting to be sent). */
export function syncSetPlaylistDescription(localId: string, description: string | null): Promise<void> {
  return invoke<void>("sync_set_playlist_description", { localId, description });
}

/** Moves a track within a playlist that is not on the server so it ends at index `to`. */
export function syncMoveLocalEntry(localId: string, from: number, to: number): Promise<void> {
  return invoke<void>("sync_move_local_entry", { localId, from, to });
}

export function syncAddLocalEntry(localId: string, entry: LocalEntryDto): Promise<void> {
  return invoke<void>("sync_add_local_entry", { localId, entry });
}

// Returns the remote playlist id.
export function syncUploadPlaylist(localId: string): Promise<string> {
  return invoke<string>("sync_upload_playlist", { localId });
}

export function syncResolveConflict(playlistId: string, resolution: ResolutionDto): Promise<void> {
  return invoke<void>("sync_resolve_conflict", { playlistId, resolution });
}
