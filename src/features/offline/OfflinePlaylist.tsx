"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import type { LocalPlaylistDto } from "../../services/tauri/sync";
import { playTrack, usePlayback } from "../../lib/playback";
import { offlineStatus, offlineKeep, offlineCancel, offlineRemove, offlineSetLimit, offlineError, type OfflineStatus } from "../../services/tauri/offline";

export function useOffline(id = "", page = 0, playlist: LocalPlaylistDto | null = null) {
  const [status, setStatus] = useState<OfflineStatus | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const version = useRef(0);
  const refresh = useCallback(async () => {
    const current = ++version.current;
    try { const value = await offlineStatus(id, page * 50); if (current === version.current) { setStatus(value); setError(null); } }
    catch (e) { if (current === version.current) setError(offlineError(e)); }
  }, [id, page, playlist]);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    setStatus(null);
    void listen("core://offline/changed", () => { void refresh(); }).then((fn) => { if (disposed) fn(); else { unlisten = fn; void refresh(); } }, () => { if (!disposed) setError("Offline storage is unavailable."); });
    return () => { disposed = true; version.current++; unlisten?.(); };
  }, [refresh]);
  const act = async (action: () => Promise<void>) => {
    setBusy(true); setError(null);
    try { await action(); await refresh(); } catch (e) { setError(offlineError(e)); }
    finally { setBusy(false); }
  };
  return { status, error, busy, act };
}
const megabytes = (bytes: number) => `${Math.round(bytes / 1048576)} MB`;

export function OfflinePlaylist({ playlist }: { playlist: LocalPlaylistDto }) {
  const { id, entryCount: entries } = playlist;
  const [page, setPage] = useState(0);
  const { status, error, busy, act } = useOffline(id, page, playlist);
  const playerBusy = usePlayback((s) => s.busy);
  const ready = status?.tracks.filter((t) => t.state === "ready").length ?? 0;
  const unsupported = status?.tracks.filter((t) => t.state === "unsupported").length ?? 0;
  const total = (status?.tracks.length ?? 0) - unsupported;
  return <section aria-label="Offline playlist" className="flex flex-col gap-3 rounded-lg bg-frost-100 p-4">
    <div className="flex flex-wrap items-center justify-between gap-3">
      <div><h3 className="font-semibold">Offline listening</h3>
        <p className="text-[13px] text-secondary-text" role="status">{status?.keeping ? `${ready} of ${total} songs ready${status.running ? " · Downloading, up to 2 at a time" : ""}` : "Keep this playlist on this device."}</p>
        {unsupported > 0 && <p className="text-[13px] text-secondary-text">{unsupported} Spotify entries cannot be downloaded.</p>}
      </div>
      <div className="flex flex-wrap gap-2">
        {status?.running ? <Button disabled={busy} onClick={() => void act(() => offlineCancel(id))}>Cancel download</Button>
          : <Button disabled={busy || !status || entries === 0} onClick={() => void act(async () => { await offlineKeep(id); setPage(0); })}>{status?.keeping ? "Update / retry downloads" : "Keep offline"}</Button>}
        {status?.keeping && <Button variant="ghost" disabled={busy || status.running} onClick={() => void act(() => offlineRemove(id))}>Remove offline copy</Button>}
      </div>
    </div>
    {status?.changed && <p className="text-[13px] text-secondary-text">Playlist changed. Update downloads to match it.</p>}
    {error && <p role="alert" className="text-[13px]">{error}</p>}
    {status?.keeping && !status.running && ready < total && <p className="text-[13px] text-secondary-text">Some songs are incomplete. Retry to download missing songs; ready songs stay available.</p>}
    {status?.tracks.slice(page * 50, page * 50 + 50).map(({ track, state, error: downloadError }, index) => <div key={`${page}-${index}`} className="flex items-center gap-3">
      <TrackCover url={track.artworkUrl} videoId={track.provider === "YouTube" ? track.id : ""} title={track.title} className="size-10" />
      <div className="min-w-0 flex-1"><p className="truncate text-[13px]">{track.title}</p><p className="truncate text-[12px] text-secondary-text">{track.artists.join(", ")} · {state === "ready" ? "Available offline" : state === "failed" ? offlineError(downloadError) : state === "unsupported" ? "Unsupported provider" : "Waiting for download"}</p></div>
      <Button variant="ghost" disabled={state !== "ready" || playerBusy} onClick={() => void playTrack({ provider: "YouTube", providerTrackId: track.id, canonicalUrl: `https://www.youtube.com/watch?v=${track.id}`, title: track.title, artists: track.artists, artworkUrl: track.artworkUrl })}>Play</Button>
    </div>)}
    {status && status.tracks.length > 50 && <div className="flex items-center gap-3"><Button disabled={page === 0} onClick={() => setPage(page - 1)}>Previous</Button><span className="text-[13px]">Page {page + 1} of {Math.ceil(status.tracks.length / 50)}</span><Button disabled={(page + 1) * 50 >= status.tracks.length} onClick={() => setPage(page + 1)}>Next</Button></div>}
  </section>;
}

export function CacheSettings() {
  const { status, error, busy, act } = useOffline();
  const [limit, setLimit] = useState("2048");
  useEffect(() => { if (status) setLimit(String(status.limitBytes / 1048576)); }, [status?.limitBytes]);
  return <section id="settings-cache" className="flex flex-col gap-3 rounded-lg bg-frost-100 p-4">
    <h3 className="font-semibold">Disk cache & offline storage</h3>
    <p className="text-[13px] text-secondary-text">{status ? `${megabytes(status.usedBytes)} audio used · ${megabytes(status.pinnedBytes)} kept offline` : "Reading storage…"}</p>
    <p className="text-[13px] text-secondary-text">Played songs are cached automatically. Old unused songs are removed first; offline playlists and the playing song stay protected.</p>
    <div className="flex flex-wrap items-center gap-2"><label htmlFor="cache-size" className="text-[13px]">Audio cache limit (MB)</label><input id="cache-size" type="number" min={64} max={20480} step={1} value={limit} onChange={(e) => setLimit(e.target.value)} className="w-28 rounded bg-frost-200 px-3 py-2" /><Button disabled={busy || !status || !Number.isInteger(Number(limit)) || Number(limit) < 64 || Number(limit) > 20480} onClick={() => void act(() => offlineSetLimit(Number(limit) * 1048576))}>Apply</Button></div>
    <p className="text-[12px] text-secondary-text">Artwork and metadata use additional space. Keep offline is available in Library → Playlists. New downloads use the current audio quality; existing offline audio is reused. Removed copies stay if another offline playlist uses them; playing files remain cached until space is needed.</p>
    {error && <p role="alert" className="text-[13px]">{error}</p>}
  </section>;
}
