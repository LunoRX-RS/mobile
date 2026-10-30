"use client";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { Button } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import type { LocalPlaylistDto } from "../../services/tauri/sync";
import { playTrack, usePlayback } from "../../lib/playback";
import { offlineStatus, offlineKeep, offlineCancel, offlineRemove, offlineSetLimit, offlineError, offlineUsage, offlineClearTransient, type OfflineStatus, type CacheUsage } from "../../services/tauri/offline";

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
function formatBytes(bytes: number): string {
  if (bytes >= 1024 ** 3) return `${(bytes / 1024 ** 3).toFixed(1).replace(/\.0$/, "")} GB`;
  if (bytes >= 1024 ** 2) return `${Math.round(bytes / 1024 ** 2)} MB`;
  return `${Math.round(bytes / 1024)} KB`;
}

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
  // The desktop Storage view's numbers: one call, refreshed when core says the cache changed.
  const [usage, setUsage] = useState<CacheUsage | null>(null);
  const [freed, setFreed] = useState<number | null>(null);
  const usageVersion = useRef(0);
  const refreshUsage = useCallback(async () => {
    const current = ++usageVersion.current;
    try { const value = await offlineUsage(); if (current === usageVersion.current) setUsage(value); }
    catch { if (current === usageVersion.current) setUsage(null); }
  }, []);
  useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen("core://offline/changed", () => { void refreshUsage(); }).then((fn) => { if (disposed) fn(); else { unlisten = fn; void refreshUsage(); } }, () => {});
    return () => { disposed = true; usageVersion.current++; unlisten?.(); };
  }, [refreshUsage]);
  useEffect(() => { if (status) setLimit(String(status.limitBytes / 1048576)); }, [status?.limitBytes]);
  const run = async (action: () => Promise<unknown>) => {
    await act(async () => { await action(); await refreshUsage(); });
  };
  const over = usage !== null && usage.totalBytes >= usage.limitBytes * 0.9;
  const share = (bytes: number) => Math.min(100, (bytes / usage!.limitBytes) * 100);
  return <section id="settings-cache" className="flex flex-col gap-3 rounded-lg bg-frost-100 p-4">
    <h3 className="font-semibold">Disk cache & offline storage</h3>
    {usage && <div className="flex flex-col gap-2">
      <div role="meter" aria-valuemin={0} aria-valuemax={usage.limitBytes} aria-valuenow={usage.totalBytes} aria-valuetext={`${formatBytes(usage.totalBytes)} of ${formatBytes(usage.limitBytes)} used`} className="flex h-1.5 w-full overflow-hidden rounded-full bg-frost-300">
        <div className={over ? "bg-caution-amber" : "bg-secondary-text"} style={{ width: `${share(usage.audioBytes)}%` }} />
        <div className={over ? "bg-caution-amber/70" : "bg-secondary-text/50"} style={{ width: `${share(usage.artworkBytes + usage.lyricBytes)}%` }} />
      </div>
      <p className="text-[12px] leading-4 text-secondary-text">
        {formatBytes(usage.totalBytes)} of {formatBytes(usage.limitBytes)} · {formatBytes(usage.audioBytes)} songs · {formatBytes(usage.artworkBytes)} artwork · {formatBytes(usage.lyricBytes)} lyrics
      </p>
    </div>}
    <p className="text-[13px] text-secondary-text">{status ? `${megabytes(status.usedBytes)} audio used · ${megabytes(status.pinnedBytes)} kept offline` : "Reading storage…"}</p>
    <p className="text-[13px] text-secondary-text">Played songs are cached automatically. Old unused songs are removed first; offline playlists and the playing song stay protected.</p>
    <div className="flex flex-wrap items-center gap-2"><label htmlFor="cache-size" className="text-[13px]">Audio cache limit (MB)</label><input id="cache-size" type="number" min={64} max={20480} step={1} value={limit} onChange={(e) => setLimit(e.target.value)} className="w-28 rounded bg-frost-200 px-3 py-2" /><Button disabled={busy || !status || !Number.isInteger(Number(limit)) || Number(limit) < 64 || Number(limit) > 20480} onClick={() => void run(() => offlineSetLimit(Number(limit) * 1048576))}>Apply</Button></div>
    {usage && <div className="flex flex-wrap items-center justify-between gap-2">
      <p className="text-[13px]">Cached songs not kept offline: <span className="text-secondary-text">{formatBytes(usage.clearableBytes)}</span></p>
      <Button variant="ghost" disabled={busy || usage.clearableBytes === 0} onClick={() => void run(async () => setFreed(await offlineClearTransient()))}>Clear cached</Button>
    </div>}
    {freed !== null && <p role="status" className="text-[13px] text-secondary-text">Freed {formatBytes(freed)}.</p>}
    {usage && usage.playlists.length > 0 && <ul className="flex flex-col gap-1">
      {usage.sharedBytes > 0 && <li className="text-[12px] text-secondary-text">{formatBytes(usage.sharedBytes)} is kept by more than one playlist, so these sizes are not a split of the total.</li>}
      {usage.playlists.map((p) => <li key={p.id} className="flex flex-wrap items-center justify-between gap-2 text-[13px]">
        <span>{p.name} <span className="text-[12px] text-secondary-text">· {p.tracks} songs · {formatBytes(p.bytes)}</span></span>
        <Button variant="ghost" disabled={busy} onClick={() => void run(async () => { await offlineRemove(p.id); setFreed(null); })}>Remove</Button>
      </li>)}
    </ul>}
    <p className="text-[12px] text-secondary-text">Artwork and metadata use additional space. Keep offline is available in Library → Playlists. New downloads use the current audio quality; existing offline audio is reused. Removed copies stay if another offline playlist uses them; playing files remain cached until space is needed.</p>
    {error && <p role="alert" className="text-[13px]">{error}</p>}
  </section>;
}
