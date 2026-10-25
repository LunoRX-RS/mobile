"use client";

import { useEffect, useState } from "react";
import { Share2 } from "lucide-react";
import { useUi } from "../../lib/store";
import { TrackCover } from "../../components/TrackCover";
import { Button, EmptyState, Screen, StatusNote } from "../../components/ui";
import { saveSharedPlaylist, sharedPlaylistGet, type SharedPlaylistDto } from "../../services/tauri/share";

const ERRORS: Record<string, string> = {
  rateLimited: "Too many requests. Try again in a minute.",
  offline: "The LUNOR-X backend is unreachable. Try again later.",
};
const SAVE_ERRORS: Record<string, string> = {
  unauthenticated: "Sign in on the Sync page to save this playlist.",
  offline: "The LUNOR-X backend is unreachable. The playlist was not saved.",
  rateLimited: "Too many requests. Try again in a minute.",
  queueFull: "The sync queue is full. Sync pending changes first, then save again.",
};

type State =
  | { status: "loading" }
  | { status: "ready"; playlist: SharedPlaylistDto }
  | { status: "error"; category: string };

function formatDuration(ms: number | null): string {
  if (ms === null) return "";
  const seconds = Math.floor(ms / 1000);
  return `${Math.floor(seconds / 60)}:${String(seconds % 60).padStart(2, "0")}`;
}

export default function SharedScreen() {
  const token = useUi((s) => s.sharedToken);
  const [state, setState] = useState<State>({ status: "loading" });
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const setSelectedPlaylist = useUi((s) => s.setSelectedPlaylist);
  const navigate = useUi((s) => s.navigate);

  // Saved copy is a normal local playlist: open it, where play and download already live.
  async function save(playlist: SharedPlaylistDto) {
    setSaving(true);
    setSaveError(null);
    try {
      const localId = await saveSharedPlaylist(playlist);
      setSelectedPlaylist(localId);
      navigate("playlists");
    } catch (e: unknown) {
      const category = typeof e === "string" ? e : "internal";
      setSaveError(SAVE_ERRORS[category] ?? "This playlist could not be saved. Try again.");
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    if (!token) return;
    let active = true;
    setState({ status: "loading" });
    sharedPlaylistGet(token)
      .then((playlist) => { if (active) setState({ status: "ready", playlist }); })
      .catch((e: unknown) => {
        if (active) setState({ status: "error", category: typeof e === "string" ? e : "internal" });
      });
    return () => { active = false; };
  }, [token]);

  if (!token) {
    return (
      <EmptyState icon={<Share2 className="size-6" aria-hidden="true" />} title="No shared playlist">
        Open a LUNOR-X share link to see it here.
      </EmptyState>
    );
  }

  if (state.status === "loading") {
    return <StatusNote role="status">Loading shared playlist…</StatusNote>;
  }

  if (state.status === "error") {
    return (
      <StatusNote tone="amber" role="alert">
        {ERRORS[state.category] ?? "This shared playlist could not be loaded. The link may have been revoked."}
      </StatusNote>
    );
  }

  const { playlist } = state;
  return (
    <Screen
      id="shared-title"
      title={playlist.name}
      caption={playlist.description ?? `${playlist.entries.length} songs · Shared, read-only`}
      actions={<Button variant="primary" disabled={saving} onClick={() => void save(playlist)}>{saving ? "Saving…" : "Save playlist"}</Button>}
    >
      {saveError && <StatusNote tone="amber" role="alert">{saveError}</StatusNote>}
      <ol className="m-0 flex list-none flex-col gap-1 p-0">
        {playlist.entries.map((entry, index) => (
          <li key={`${index}-${entry.title}`} className="grid grid-cols-[32px_44px_1fr_auto] items-center gap-3 rounded-[10px] px-2 py-1.5 hover:bg-frost-200">
            <span className="text-right text-[13px] tabular-nums text-secondary-text">{index + 1}</span>
            <TrackCover url={entry.artworkUrl} videoId={entry.provider === "YouTube" ? entry.providerTrackId : ""} title={entry.title} className="size-11" />
            <div className="flex min-w-0 flex-col">
              <span className="truncate font-semibold">{entry.title}</span>
              {entry.artists.length > 0 && <span className="truncate text-[13px] text-secondary-text">{entry.artists.join(", ")}</span>}
            </div>
            <span className="text-[13px] tabular-nums text-secondary-text">{formatDuration(entry.durationMs)}</span>
          </li>
        ))}
      </ol>
    </Screen>
  );
}
