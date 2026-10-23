"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Cloud, Play, Trash2 } from "lucide-react";
import { playTrack } from "../../lib/playback";
import { useUi } from "../../lib/store";
import { TrackCover } from "../../components/TrackCover";
import Analytics from "./Analytics";
import { Button, EmptyState, Screen, Segmented, StatusNote } from "../../components/ui";
import { statsClearLocal, statsCloudClear, statsCloudSync, statsLocal, statsSetCloud, type PlayCountDto, type StatsViewDto } from "../../services/tauri/stats";

type Scope = "device" | "cloud";

const CLOUD_ERRORS: Record<string, string> = {
  unauthenticated: "Sign in on the Sync page to use cloud stats.",
  offline: "The backend is unreachable. Showing this device only.",
  rateLimited: "Too many requests. Try again in a minute.",
};
const categoryOf = (e: unknown) => (typeof e === "string" ? e : "internal");

const topArtists = (tracks: PlayCountDto[]) => {
  const plays = new Map<string, number>();
  for (const t of tracks) for (const a of t.artists) plays.set(a, (plays.get(a) ?? 0) + t.plays);
  return [...plays].sort((a, b) => b[1] - a[1]).slice(0, 8);
};

export default function StatsScreen() {
  const navigate = useUi((s) => s.navigate);
  const [local, setLocal] = useState<StatsViewDto | null>(null);
  const [cloud, setCloud] = useState<PlayCountDto[] | null>(null);
  const [scope, setScope] = useState<Scope>("device");
  const [note, setNote] = useState<{ tone: "ok" | "bad"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);

  const syncCloud = useCallback(async () => {
    setBusy(true);
    try { setCloud((await statsCloudSync()).tracks); setNote(null); }
    catch (e) { setNote({ tone: "bad", text: CLOUD_ERRORS[categoryOf(e)] ?? "Cloud stats failed. Try again." }); setScope("device"); }
    finally { setBusy(false); }
  }, []);

  useEffect(() => {
    let active = true;
    statsLocal().then((v) => { if (active) setLocal(v); }, () => { if (active) setNote({ tone: "bad", text: "Could not read listening stats. Check local storage." }); });
    return () => { active = false; };
  }, []);
  useEffect(() => { if (local?.cloud) void syncCloud(); }, [local?.cloud, syncCloud]);

  const tracks = (scope === "cloud" && cloud ? cloud : local?.tracks) ?? [];
  const total = useMemo(() => tracks.reduce((n, t) => n + t.plays, 0), [tracks]);
  const artists = useMemo(() => topArtists(tracks), [tracks]);
  const top = useMemo(() => [...tracks].sort((a, b) => b.plays - a.plays || b.lastPlayedMs - a.lastPlayedMs).slice(0, 25), [tracks]);

  async function toggleCloud(on: boolean) {
    setBusy(true);
    try { setLocal(await statsSetCloud(on)); if (!on) { setCloud(null); setScope("device"); } }
    catch { setNote({ tone: "bad", text: "Could not change the cloud setting." }); }
    finally { setBusy(false); }
  }
  async function deleteCloud() {
    setBusy(true);
    try { await statsCloudClear(); setCloud(null); setScope("device"); setLocal(await statsLocal()); setNote({ tone: "ok", text: "Cloud stats deleted. Cloud sync is off." }); }
    catch (e) { setNote({ tone: "bad", text: CLOUD_ERRORS[categoryOf(e)] ?? "Could not delete cloud stats." }); }
    finally { setBusy(false); }
  }
  async function clearLocal() {
    setBusy(true);
    try { setLocal(await statsClearLocal()); setNote({ tone: "ok", text: "Cleared the play counts on this device." }); }
    catch { setNote({ tone: "bad", text: "Could not clear the play counts." }); }
    finally { setBusy(false); }
  }

  const caption = scope === "cloud" ? "Plays across every device signed in to your account." : "Plays counted on this device. Nothing leaves it unless you turn on cloud sync.";
  return (
    <Screen id="stats-title" title="Stats" caption={caption}
      actions={local?.cloud ? <Segmented label="Stats scope" value={scope} options={[{ key: "device", label: "This device" }, { key: "cloud", label: "All devices" }]} onChange={(s) => { setScope(s); if (s === "cloud" && !cloud) void syncCloud(); }} /> : undefined}>
      {note && <StatusNote role={note.tone === "bad" ? "alert" : "status"} tone={note.tone === "bad" ? "amber" : undefined}>{note.text}</StatusNote>}
      {scope === "device" ? <Analytics key={local?.tracks.length ?? 0} /> : local && tracks.length === 0 ? (
        <EmptyState icon={<Play size={24} />} title="No plays counted yet" action={<Button variant="secondary" onClick={() => navigate("search")}>Find songs</Button>}>
          Songs you play are counted here from now on.
        </EmptyState>
      ) : (
        <>
          <dl className="grid grid-cols-3 gap-4">
            {[["Plays", total], ["Songs", tracks.length], ["Artists", new Set(tracks.flatMap((t) => t.artists)).size]].map(([k, v]) => (
              <div key={k} className="rounded-[12px] bg-frost-200 px-4 py-3"><dt className="text-[12px] text-secondary-text">{k}</dt><dd className="font-display text-[28px] leading-9 font-semibold tabular-nums">{v}</dd></div>
            ))}
          </dl>
          <section aria-labelledby="stats-artists" className="flex flex-col gap-2">
            <h2 id="stats-artists" className="text-[15px] font-semibold">Top artists</h2>
            <ol className="flex flex-col">
              {artists.map(([name, plays], i) => (
                <li key={name} className="grid h-10 grid-cols-[24px_minmax(0,1fr)_auto] items-center gap-2 px-2 text-[13px]">
                  <span className="tabular-nums text-secondary-text">{i + 1}</span><span className="truncate">{name}</span><span className="tabular-nums text-secondary-text">{plays} plays</span>
                </li>
              ))}
            </ol>
          </section>
          <section aria-labelledby="stats-tracks" className="flex flex-col gap-2">
            <h2 id="stats-tracks" className="text-[15px] font-semibold">Top songs</h2>
            <ol className="flex flex-col">
              {top.map((t, i) => (
                <li key={`${t.provider}:${t.providerTrackId}`} className="grid min-h-14 grid-cols-[24px_40px_minmax(0,1fr)_auto] items-center gap-3 rounded-lg px-2 hover:bg-frost-200 focus-within:bg-frost-200">
                  <span className="text-[12px] tabular-nums text-secondary-text">{i + 1}</span>
                  <TrackCover url={t.artworkUrl ?? null} videoId={t.providerTrackId} className="size-10" />
                  <button type="button" disabled={t.provider !== "YouTube"} aria-label={`Play ${t.title}`} onClick={() => void playTrack({ provider: t.provider, providerTrackId: t.providerTrackId, canonicalUrl: `https://www.youtube.com/watch?v=${t.providerTrackId}`, title: t.title, artists: t.artists, artworkUrl: t.artworkUrl ?? null })} className="min-w-0 rounded-md text-left disabled:opacity-60">
                    <p className="truncate text-[13px] font-semibold">{t.title}</p>
                    <p className="truncate text-[12px] text-secondary-text">{t.artists.join(", ")}</p>
                  </button>
                  <span className="text-[13px] tabular-nums text-secondary-text">{t.plays} plays</span>
                </li>
              ))}
            </ol>
          </section>
        </>
      )}
      <section aria-labelledby="stats-privacy" className="flex flex-col gap-3 rounded-[12px] bg-frost-200 p-4">
        <h2 id="stats-privacy" className="flex items-center gap-2 text-[15px] font-semibold"><Cloud size={16} aria-hidden="true" />Cloud sync</h2>
        <p className="text-[13px] text-secondary-text">
          Off by default. When on, this device sends each song&rsquo;s title, artists and play count to your LunoRX account so every device can show the same totals. It does not send when you played each song, and never sends provider sign-ins.
        </p>
        <div className="flex flex-wrap gap-2">
          <Button variant={local?.cloud ? "secondary" : "primary"} disabled={busy || !local} onClick={() => void toggleCloud(!local?.cloud)}>{local?.cloud ? "Turn off cloud sync" : "Turn on cloud sync"}</Button>
          {local?.cloud && <Button variant="secondary" disabled={busy} onClick={() => void deleteCloud()}><Trash2 size={14} aria-hidden="true" />Delete cloud stats</Button>}
          <Button variant="ghost" disabled={busy || !local || local.tracks.length === 0} onClick={() => void clearLocal()}>Clear this device</Button>
        </div>
      </section>
    </Screen>
  );
}
