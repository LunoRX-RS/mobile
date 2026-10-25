"use client";

import { useCallback, useEffect, useMemo, useState } from "react";
import { Music2, Play, Sparkles, TrendingUp, Users } from "lucide-react";
import { useUi } from "../../lib/store";
import { TrackCover } from "../../components/TrackCover";
import { Button, EmptyState, Screen, Segmented, StatusNote } from "../../components/ui";
import { statsLocal, statsReport, type PlayCountDto, type ListeningReport } from "../../services/tauri/stats";

type Tab = "replay" | "dna" | "crossover" | "timeline" | "alltime";

const MS_HOUR = 3_600_000;

const monthRange = (y: number, m: number) => ({
  from: new Date(y, m, 1).getTime(),
  to: new Date(y, m + 1, 1).getTime(),
});
const yearRange = (y: number) => ({ from: new Date(y, 0, 1).getTime(), to: new Date(y + 1, 0, 1).getTime() });

function fmtMs(ms: number): string {
  const h = ms / MS_HOUR;
  if (h >= 1) return `${h.toFixed(1)}h`;
  return `${Math.round(ms / 60000)}m`;
}

function keyOf(t: { title: string; artists: string[] }): string {
  return `${t.title.trim().toLowerCase()}::${t.artists.map((a) => a.trim().toLowerCase()).join(",")}`;
}

export default function ReplayScreen() {
  const navigate = useUi((s) => s.navigate);
  const [tab, setTab] = useState<Tab>("replay");
  const [local, setLocal] = useState<{ tracks: PlayCountDto[] } | null>(null);
  const [alltime, setAlltime] = useState<ListeningReport | null>(null);
  const [monthly, setMonthly] = useState<ListeningReport | null>(null);
  const [yearly, setYearly] = useState<ListeningReport | null>(null);
  const [note, setNote] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const now = new Date();
  const [viewYear, setViewYear] = useState(now.getFullYear());
  const [viewMonth, setViewMonth] = useState(now.getMonth());

  const load = useCallback(async () => {
    setBusy(true);
    try {
      const lv = await statsLocal();
      setLocal({ tracks: lv.tracks });
      const at = await statsReport(0, Number.MAX_SAFE_INTEGER);
      setAlltime(at);
      const mr = monthRange(viewYear, viewMonth);
      const yr = yearRange(viewYear);
      const [m, y] = await Promise.all([
        statsReport(mr.from, mr.to),
        statsReport(yr.from, yr.to),
      ]);
      setMonthly(m);
      setYearly(y);
      setNote(null);
    } catch {
      setNote("Could not read listening stats. Play some songs first.");
    } finally {
      setBusy(false);
    }
  }, [viewYear, viewMonth]);

  useEffect(() => { void load(); }, [load]);

  const allTracks = local?.tracks ?? [];
  const totalPlays = useMemo(() => allTracks.reduce((n, t) => n + t.plays, 0), [allTracks]);

  const wrapped = useMemo(() => {
    if (allTracks.length === 0 || totalPlays === 0) return null;
    const sorted = [...allTracks].sort((a, b) => b.plays - a.plays);
    const cumShare = (frac: number) => {
      const n = Math.max(1, Math.ceil(sorted.length * frac));
      const s = sorted.slice(0, n).reduce((x, t) => x + t.plays, 0);
      return Math.round((s / totalPlays) * 100);
    };
    const topTrack = sorted[0];
    const topArtistMap = new Map<string, number>();
    for (const t of allTracks) for (const a of t.artists) topArtistMap.set(a, (topArtistMap.get(a) ?? 0) + t.plays);
    const topArtist = [...topArtistMap].sort((a, b) => b[1] - a[1])[0];
    return {
      topTrackShare: Math.round((topTrack.plays / totalPlays) * 100),
      topArtistShare: topArtist ? Math.round((topArtist[1] / totalPlays) * 100) : 0,
      topTrack,
      topArtist: topArtist?.[0] ?? null,
      top1: cumShare(0.01),
      top5: cumShare(0.05),
      top10: cumShare(0.10),
      artistCount: topArtistMap.size,
    };
  }, [allTracks, totalPlays]);

  const crossover = useMemo(() => {
    const byProvider = new Map<string, { plays: number; tracks: number }>();
    for (const t of allTracks) {
      const cur = byProvider.get(t.provider) ?? { plays: 0, tracks: 0 };
      cur.plays += t.plays;
      cur.tracks += 1;
      byProvider.set(t.provider, cur);
    }
    const sets = new Map<string, Set<string>>();
    for (const t of allTracks) {
      if (!sets.has(t.provider)) sets.set(t.provider, new Set());
      sets.get(t.provider)!.add(keyOf(t));
    }
    const providers = [...sets.keys()];
    let overlap = 0;
    if (providers.length >= 2) {
      const a = sets.get(providers[0])!;
      const b = sets.get(providers[1])!;
      for (const k of a) if (b.has(k)) overlap += 1;
    }
    return { byProvider, providers, overlap };
  }, [allTracks]);

  const monthLabel = new Date(viewYear, viewMonth, 1).toLocaleString(undefined, { month: "long", year: "numeric" });

  const peakHour = alltime ? alltime.byHour.indexOf(Math.max(...alltime.byHour)) : 0;

  return (
    <>
    <Screen
      id="replay-title"
      title="Unified Replay"
      caption="One place for everything you have listened to. Local-first; nothing leaves this device unless cloud sync is on."
    >
      {note && <StatusNote tone="amber">{note}</StatusNote>}

      {/* Hero */}
      <section className="relative overflow-hidden rounded-[20px] bg-gradient-to-br from-frost-200 via-frost-100 to-night-canvas p-6">
        <div className="pointer-events-none absolute -right-10 -top-10 size-48 rounded-full bg-lumen-lavender/20 blur-3xl" />
        <div className="relative flex flex-col gap-3 sm:flex-row sm:items-end sm:justify-between">
          <div className="flex flex-col gap-1">
            <p className="text-[12px] font-semibold uppercase tracking-[0.18em] text-lumen-lavender">Your listening, unified</p>
            <h2 className="font-display text-[26px] leading-8 font-bold tracking-[-0.02em]">A single story across every provider</h2>
            <p className="max-w-[60ch] text-[13px] leading-[18px] text-secondary-text">
              Replay, Music DNA, Crossover, Timeline and All-time stats — all from your on-device play log.
            </p>
          </div>
          {wrapped && (
            <div className="flex flex-col items-start gap-1 rounded-[12px] bg-night-canvas/60 px-4 py-3 backdrop-blur">
              <p className="text-[11px] font-semibold uppercase tracking-wide text-secondary-text">Highlights</p>
              <p className="font-display text-[22px] leading-7 font-bold tabular-nums">{wrapped.topTrackShare}%</p>
              <p className="text-[12px] text-secondary-text">of your plays are your #1 track</p>
            </div>
          )}
        </div>
      </section>

      <Segmented<Tab>
        label="Replay section"
        value={tab}
        options={[
          { key: "replay", label: "Replay" },
          { key: "dna", label: "Music DNA" },
          { key: "crossover", label: "Crossover" },
          { key: "timeline", label: "Timeline" },
          { key: "alltime", label: "All-time" },
        ]}
        onChange={setTab}
      />

      {tab === "replay" && (
        <div className="flex flex-col gap-6">
          <div className="flex flex-wrap items-center gap-2">
            <Button variant="secondary" onClick={() => { const d = new Date(viewYear, viewMonth - 1, 1); setViewYear(d.getFullYear()); setViewMonth(d.getMonth()); }}>‹ Prev</Button>
            <span className="min-w-[180px] text-center text-[14px] font-semibold">{monthLabel}</span>
            <Button variant="secondary" onClick={() => { const d = new Date(viewYear, viewMonth + 1, 1); setViewYear(d.getFullYear()); setViewMonth(d.getMonth()); }}>Next ›</Button>
          </div>

          {monthly && (
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              {([
                ["Plays", monthly.plays, Play],
                ["Listened", fmtMs(monthly.listenedMs), TrendingUp],
                ["Songs", monthly.uniqueTracks, Music2],
                ["Artists", monthly.uniqueArtists, Users],
              ] as [string, string | number, React.ComponentType<{ size?: number; className?: string; "aria-hidden"?: boolean }>][]).map(([k, v, Icon]) => (
                <div key={k as string} className="group relative overflow-hidden rounded-[14px] bg-frost-200 px-4 py-4 transition-colors hover:bg-frost-300">
                  <div className="flex items-center justify-between">
                    <dt className="text-[12px] text-secondary-text">{k as string}</dt>
                    <Icon size={16} className="text-tertiary-text" aria-hidden={true} />
                  </div>
                  <dd className="mt-1 font-display text-[26px] leading-8 font-bold tabular-nums">{v}</dd>
                </div>
              ))}
            </div>
          )}

          <div className="grid grid-cols-1 gap-4 lg:grid-cols-2">
            {monthly && monthly.topArtists.length > 0 && (
              <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
                <h2 className="text-[15px] font-semibold">Top artists this month</h2>
                <ol className="flex flex-col gap-1">
                  {monthly.topArtists.slice(0, 5).map((a, i) => (
                    <li key={a.name} className="grid h-9 grid-cols-[24px_minmax(0,1fr)_auto] items-center gap-2 rounded-[8px] px-2 text-[13px] hover:bg-frost-200">
                      <span className="tabular-nums text-secondary-text">{i + 1}</span>
                      <span className="truncate font-medium">{a.name}</span>
                      <span className="tabular-nums text-secondary-text">{a.plays} · {fmtMs(a.listenedMs)}</span>
                    </li>
                  ))}
                </ol>
              </section>
            )}
            {monthly && monthly.topTracks.length > 0 && (
              <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
                <h2 className="text-[15px] font-semibold">Top songs this month</h2>
                <ol className="flex flex-col gap-1">
                  {monthly.topTracks.slice(0, 5).map((t, i) => (
                    <li key={`${t.provider}:${t.providerTrackId}`} className="grid min-h-11 grid-cols-[24px_36px_minmax(0,1fr)_auto] items-center gap-3 rounded-[8px] px-2 hover:bg-frost-200">
                      <span className="text-[12px] tabular-nums text-secondary-text">{i + 1}</span>
                      <TrackCover url={t.artworkUrl ?? null} videoId={t.providerTrackId} className="size-9" />
                      <div className="min-w-0">
                        <p className="truncate text-[13px] font-semibold">{t.title}</p>
                        <p className="truncate text-[12px] text-secondary-text">{t.artists.join(", ")}</p>
                      </div>
                      <span className="text-[13px] tabular-nums text-secondary-text">{t.plays}</span>
                    </li>
                  ))}
                </ol>
              </section>
            )}
          </div>

          {yearly && (
            <section className="relative overflow-hidden rounded-[16px] bg-gradient-to-r from-frost-200 to-frost-100 p-5">
              <div className="pointer-events-none absolute -right-8 -top-8 size-32 rounded-full bg-lumen-lavender/20 blur-2xl" />
              <h2 className="text-[15px] font-semibold">{viewYear} in numbers</h2>
              <div className="mt-3 grid grid-cols-2 gap-3 sm:grid-cols-4">
                {[
                  ["Plays", yearly.plays],
                  ["Listened", fmtMs(yearly.listenedMs)],
                  ["Songs", yearly.uniqueTracks],
                  ["Artists", yearly.uniqueArtists],
                ].map(([k, v]) => (
                  <div key={k as string}>
                    <dt className="text-[12px] text-secondary-text">{k as string}</dt>
                    <dd className="font-display text-[20px] leading-7 font-bold tabular-nums">{v}</dd>
                  </div>
                ))}
              </div>
              {yearly.newTracks > 0 && <p className="mt-3 text-[13px] text-secondary-text">{yearly.newTracks} new songs discovered this year.</p>}
            </section>
          )}

          {allTracks.length === 0 && (
            <EmptyState icon={<Play size={24} />} title="No plays counted yet">
              <p>Songs you play are counted here from now on.</p>
              <Button variant="primary" onClick={() => navigate("search")}>Find songs</Button>
            </EmptyState>
          )}
        </div>
      )}

      {tab === "dna" && (
        <div className="flex flex-col gap-6">
          {alltime && (
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              {[
                ["Plays", alltime.plays],
                ["Listened", fmtMs(alltime.listenedMs)],
                ["Songs", alltime.uniqueTracks],
                ["Artists", alltime.uniqueArtists],
              ].map(([k, v]) => (
                <div key={k as string} className="rounded-[14px] bg-frost-200 px-4 py-4">
                  <dt className="text-[12px] text-secondary-text">{k as string}</dt>
                  <dd className="mt-1 font-display text-[26px] leading-8 font-bold tabular-nums">{v}</dd>
                </div>
              ))}
            </div>
          )}

          {alltime && (
            <section className="flex flex-col gap-3 rounded-[16px] bg-frost-100 p-5">
              <h2 className="text-[15px] font-semibold">Listening profile</h2>
              <div className="grid grid-cols-1 gap-2 text-[13px] sm:grid-cols-2">
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Peak hour</p>
                  <p className="font-semibold">{peakHour}:00</p>
                </div>
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Active days</p>
                  <p className="font-semibold">{alltime.activeDays}</p>
                </div>
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Current streak</p>
                  <p className="font-semibold">{alltime.currentStreak} days</p>
                </div>
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Longest streak</p>
                  <p className="font-semibold">{alltime.longestStreak} days</p>
                </div>
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Avg. completion</p>
                  <p className="font-semibold">{alltime.avgCompletion}%</p>
                </div>
                <div className="rounded-[10px] bg-frost-200 px-3 py-2">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Skips</p>
                  <p className="font-semibold">{alltime.skips}</p>
                </div>
              </div>
              <div className="mt-2">
                <p className="mb-1 text-[11px] uppercase tracking-wide text-secondary-text">Weekday skew</p>
                <div className="flex h-16 items-end gap-1.5">
                  {alltime.byWeekday.map((v, i) => {
                    const max = Math.max(1, ...alltime.byWeekday);
                    return (
                      <div key={i} className="flex flex-1 flex-col items-center gap-1">
                        <div className="w-full rounded-t-[4px] bg-frost-300" style={{ height: `${(v / max) * 100}%` }} title={`${["Sun","Mon","Tue","Wed","Thu","Fri","Sat"][i]} — ${v} plays`} />
                        <span className="text-[10px] text-tertiary-text">{["S","M","T","W","T","F","S"][i]}</span>
                      </div>
                    );
                  })}
                </div>
              </div>
            </section>
          )}

          {wrapped && (
            <section className="relative overflow-hidden rounded-[16px] bg-gradient-to-br from-lumen-lavender/20 via-frost-200 to-frost-100 p-5">
              <div className="pointer-events-none absolute -right-6 -top-6 size-40 rounded-full bg-lumen-lavender/30 blur-3xl" />
              <h2 className="flex items-center gap-2 text-[15px] font-semibold">
                <Sparkles size={16} className="text-lumen-lavender" aria-hidden="true" />
                Highlights
              </h2>
              <ul className="mt-3 flex flex-col gap-2 text-[13px]">
                <li className="rounded-[10px] bg-night-canvas/40 px-3 py-2 backdrop-blur">
                  Your top track, <span className="font-semibold text-primary-text">{wrapped.topTrack.title}</span>, is {wrapped.topTrackShare}% of your plays.
                </li>
                {wrapped.topArtist && (
                  <li className="rounded-[10px] bg-night-canvas/40 px-3 py-2 backdrop-blur">
                    Your top artist, <span className="font-semibold text-primary-text">{wrapped.topArtist}</span>, is {wrapped.topArtistShare}% of your plays.
                  </li>
                )}
                <li className="rounded-[10px] bg-night-canvas/40 px-3 py-2 backdrop-blur">
                  Your top 1% of tracks account for {wrapped.top1}% of all plays; top 5% for {wrapped.top5}%; top 10% for {wrapped.top10}%.
                </li>
                <li className="rounded-[10px] bg-night-canvas/40 px-3 py-2 backdrop-blur">
                  {wrapped.artistCount} distinct artists across {allTracks.length} tracked songs.
                </li>
              </ul>
            </section>
          )}

          {alltime && alltime.topArtists.length > 0 && (
            <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
              <h2 className="text-[15px] font-semibold">Music DNA · top artists</h2>
              <ol className="flex flex-col gap-1">
                {alltime.topArtists.slice(0, 8).map((a, i) => (
                  <li key={a.name} className="grid h-9 grid-cols-[24px_minmax(0,1fr)_auto] items-center gap-2 rounded-[8px] px-2 text-[13px] hover:bg-frost-200">
                    <span className="tabular-nums text-secondary-text">{i + 1}</span>
                    <span className="truncate font-medium">{a.name}</span>
                    <span className="tabular-nums text-secondary-text">{a.plays} · {fmtMs(a.listenedMs)}</span>
                  </li>
                ))}
              </ol>
            </section>
          )}
        </div>
      )}

      {tab === "crossover" && (
        <div className="flex flex-col gap-6">
          <section className="flex flex-col gap-3">
            <h2 className="text-[15px] font-semibold">Provider crossover</h2>
            <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
              {[...crossover.byProvider.entries()].map(([p, v]) => (
                <div key={p} className="relative overflow-hidden rounded-[14px] bg-frost-200 px-4 py-4">
                  <div className="pointer-events-none absolute -right-4 -top-4 size-20 rounded-full bg-lumen-lavender/15 blur-2xl" />
                  <p className="text-[13px] font-semibold">{p}</p>
                  <p className="mt-1 font-display text-[22px] leading-7 font-bold tabular-nums">{v.plays}</p>
                  <p className="text-[12px] text-secondary-text">{v.tracks} tracks</p>
                </div>
              ))}
            </div>
            <p className="text-[13px] text-secondary-text">
              {crossover.providers.length >= 2
                ? `${crossover.overlap} songs appear in both ${crossover.providers.join(" and ")} by matching title and artists.`
                : "Connect and play from more than one provider to see crossover stats."}
            </p>
          </section>

          {alltime && alltime.topTracks.length > 0 && (
            <section className="flex flex-col gap-3">
              <h2 className="text-[15px] font-semibold">Top tracks per provider</h2>
              {crossover.providers.map((p) => {
                const ts = alltime.topTracks.filter((t) => t.provider === p).slice(0, 5);
                if (ts.length === 0) return null;
                return (
                  <div key={p} className="flex flex-col gap-2 rounded-[14px] bg-frost-100 p-4">
                    <p className="text-[12px] font-semibold text-secondary-text">{p}</p>
                    <ol className="flex flex-col gap-1">
                      {ts.map((t, i) => (
                        <li key={`${t.provider}:${t.providerTrackId}`} className="grid min-h-10 grid-cols-[20px_32px_minmax(0,1fr)_auto] items-center gap-2 rounded-[8px] px-2 text-[13px] hover:bg-frost-200">
                          <span className="tabular-nums text-secondary-text">{i + 1}</span>
                          <TrackCover url={t.artworkUrl ?? null} videoId={t.providerTrackId} className="size-8" />
                          <div className="min-w-0">
                            <p className="truncate font-semibold">{t.title}</p>
                            <p className="truncate text-[12px] text-secondary-text">{t.artists.join(", ")}</p>
                          </div>
                          <span className="tabular-nums text-secondary-text">{t.plays}</span>
                        </li>
                      ))}
                    </ol>
                  </div>
                );
              })}
            </section>
          )}
        </div>
      )}

      {tab === "timeline" && (
        <div className="flex flex-col gap-6">
          {alltime ? (
            <>
              <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
                <h2 className="text-[15px] font-semibold">By hour of day</h2>
                <div className="flex h-28 items-end gap-1">
                  {alltime.byHour.map((v, i) => {
                    const max = Math.max(1, ...alltime.byHour);
                    return (
                      <div
                        key={i}
                        className="flex-1 rounded-t-[4px] bg-gradient-to-t from-lumen-lavender/60 to-lumen-lavender/20 transition-all hover:from-lumen-lavender hover:to-lumen-lavender/40"
                        style={{ height: `${(v / max) * 100}%` }}
                        title={`${i}:00 — ${v} plays`}
                      />
                    );
                  })}
                </div>
                <p className="text-[12px] text-secondary-text">Peak at {peakHour}:00.</p>
              </section>

              <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
                <h2 className="text-[15px] font-semibold">By weekday</h2>
                <div className="flex h-24 items-end gap-2">
                  {alltime.byWeekday.map((v, i) => {
                    const max = Math.max(1, ...alltime.byWeekday);
                    return (
                      <div key={i} className="flex flex-1 flex-col items-center gap-1">
                        <div className="w-full rounded-t-[6px] bg-frost-300" style={{ height: `${(v / max) * 100}%` }} title={`${["Sun","Mon","Tue","Wed","Thu","Fri","Sat"][i]} — ${v} plays`} />
                        <span className="text-[10px] text-tertiary-text">{["S","M","T","W","T","F","S"][i]}</span>
                      </div>
                    );
                  })}
                </div>
              </section>

              <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
                <h2 className="text-[15px] font-semibold">Recent days</h2>
                <div className="flex h-20 items-end gap-px overflow-x-auto">
                  {alltime.byDay.slice(-45).map((d) => {
                    const max = Math.max(1, ...alltime.byDay.map((x) => x.plays));
                    return <div key={d.day} className="min-w-[5px] flex-1 rounded-t-[2px] bg-frost-300" style={{ height: `${(d.plays / max) * 100}%` }} title={`${d.plays} plays`} />;
                  })}
                </div>
              </section>

              <section className="grid grid-cols-1 gap-3 sm:grid-cols-3">
                <div className="rounded-[12px] bg-frost-200 px-4 py-3">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Current streak</p>
                  <p className="font-display text-[22px] leading-7 font-bold tabular-nums">{alltime.currentStreak} days</p>
                </div>
                <div className="rounded-[12px] bg-frost-200 px-4 py-3">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Longest streak</p>
                  <p className="font-display text-[22px] leading-7 font-bold tabular-nums">{alltime.longestStreak} days</p>
                </div>
                <div className="rounded-[12px] bg-frost-200 px-4 py-3">
                  <p className="text-[11px] uppercase tracking-wide text-secondary-text">Active days</p>
                  <p className="font-display text-[22px] leading-7 font-bold tabular-nums">{alltime.activeDays}</p>
                </div>
              </section>
            </>
          ) : (
            <EmptyState icon={<Music2 size={24} />} title="No timeline yet" />
          )}
        </div>
      )}

      {tab === "alltime" && (
        <div className="flex flex-col gap-6">
          {alltime && (
            <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
              {[
                ["Plays", alltime.plays],
                ["Listened", fmtMs(alltime.listenedMs)],
                ["Songs", alltime.uniqueTracks],
                ["Artists", alltime.uniqueArtists],
              ].map(([k, v]) => (
                <div key={k as string} className="rounded-[14px] bg-frost-200 px-4 py-4">
                  <dt className="text-[12px] text-secondary-text">{k as string}</dt>
                  <dd className="mt-1 font-display text-[26px] leading-8 font-bold tabular-nums">{v}</dd>
                </div>
              ))}
            </div>
          )}

          {alltime && alltime.topTracks.length > 0 && (
            <section className="flex flex-col gap-2 rounded-[16px] bg-frost-100 p-4">
              <h2 className="text-[15px] font-semibold">All-time top songs</h2>
              <ol className="flex flex-col gap-1">
                {alltime.topTracks.map((t, i) => (
                  <li key={`${t.provider}:${t.providerTrackId}`} className="grid min-h-12 grid-cols-[28px_40px_minmax(0,1fr)_auto] items-center gap-3 rounded-[8px] px-2 hover:bg-frost-200">
                    <span className="text-[12px] tabular-nums text-secondary-text">{i + 1}</span>
                    <TrackCover url={t.artworkUrl ?? null} videoId={t.providerTrackId} className="size-10" />
                    <div className="min-w-0">
                      <p className="truncate text-[13px] font-semibold">{t.title}</p>
                      <p className="truncate text-[12px] text-secondary-text">{t.artists.join(", ")}</p>
                    </div>
                    <span className="text-[13px] tabular-nums text-secondary-text">{t.plays}</span>
                  </li>
                ))}
              </ol>
            </section>
          )}

          {allTracks.length === 0 && (
            <EmptyState icon={<Play size={24} />} title="No plays counted yet">
              <p>Songs you play are counted here from now on.</p>
            </EmptyState>
          )}
        </div>
      )}

      {busy && <p className="text-[12px] text-secondary-text">Loading…</p>}
    </Screen>
    </>
  );
}