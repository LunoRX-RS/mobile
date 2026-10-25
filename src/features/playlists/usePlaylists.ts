import { useCallback, useEffect, useState } from "react";
import { useImport } from "../../lib/youtubeImport";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import {
  syncCheckPlaylists,
  syncListLocalPlaylists,
  syncStatus,
  type RemoteState,
  type ErrorCategory,
  type LocalPlaylistDto,
  type SyncStatusDto,
} from "../../services/tauri/sync";

// Rejections are bare category strings; anything else falls back to the generic category.
export function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e)
    ? (e as ErrorCategory)
    : "internal";
}

export function errorText(e: unknown): string {
  return CATEGORY_MESSAGES[toCategory(e)];
}

// Loads local playlists. `active` guards against setting state after unmount or a newer reload.
export function useLocalPlaylists() {
  const [version, setVersion] = useState(0);
  // A finished YouTube import adds playlists, so the list reads again.
  const imported = useImport((s) => s.finished);
  const [loading, setLoading] = useState(true);
  const [playlists, setPlaylists] = useState<LocalPlaylistDto[]>([]);
  // The server's view, checked once per load. Absent while signed out or offline: labels fall back to local state.
  const [remote, setRemote] = useState<Record<string, RemoteState>>({});
  const [error, setError] = useState<ErrorCategory | null>(null);

  useEffect(() => {
    let active = true;
    syncListLocalPlaylists().then(
      (list) => {
        if (!active) return;
        setPlaylists(list);
        setError(null);
        setLoading(false);
        if (list.some((p) => p.remoteId)) {
          syncCheckPlaylists().then(
            (states) => active && setRemote(Object.fromEntries(states.map((s) => [s.localId, s.state]))),
            () => active && setRemote({}),
          );
        }
      },
      (e: unknown) => {
        if (!active) return;
        setPlaylists([]);
        setError(toCategory(e));
        setLoading(false);
      },
    );
    return () => {
      active = false;
    };
  }, [version, imported]);

  const reload = useCallback(() => {
    setLoading(true);
    setVersion((v) => v + 1);
  }, []);

  const withRemote = playlists.map((p) => (remote[p.id] ? { ...p, remote: remote[p.id] } : p));
  return { loading, playlists: withRemote, error, reload };
}

// Plain-word sync state for one playlist.
export function describeUpload(p: LocalPlaylistDto & { remote?: RemoteState }): string {
  if (!p.remoteId) return "Not uploaded";
  // The server's answer wins where it knows more than this device. Nothing is sent until the user acts.
  if (p.remote === "missingOnServer") return "Not on the server any more";
  if (p.remote === "serverNewer") return "Changed on the server";
  if (p.unsentEntries > 0) return `Partly uploaded, ${p.uploaded} of ${p.entryCount}`;
  if (p.queuedWrites > 0) return `${p.queuedWrites} ${p.queuedWrites === 1 ? "change" : "changes"} waiting`;
  return "Synced";
}

export const entriesLabel = (n: number) => `${n} ${n === 1 ? "entry" : "entries"}`;

export function needsUpload(p: LocalPlaylistDto): boolean {
  return p.remoteId === null || p.unsentEntries > 0;
}

// Reads sync_status (offline, signed out, conflicts). `null` until the first answer; an error leaves it null so the
// editor simply shows no banner instead of guessing.
export function useSyncStatus() {
  const [version, setVersion] = useState(0);
  const [status, setStatus] = useState<SyncStatusDto | null>(null);
  useEffect(() => {
    let active = true;
    syncStatus().then(
      (s) => active && setStatus(s),
      () => active && setStatus(null),
    );
    return () => {
      active = false;
    };
  }, [version]);
  const reload = useCallback(() => setVersion((v) => v + 1), []);
  return { status, reload };
}
