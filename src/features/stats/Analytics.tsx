"use client";

import { useEffect, useMemo, useState } from "react";
import { Flame, Play } from "lucide-react";
import { playTrack } from "../../lib/playback";
import { TrackCover } from "../../components/TrackCover";
import { Segmented } from "../../components/ui";
import { statsReport, type ListeningReport } from "../../services/tauri/stats";

type Range = "7d" | "30d" | "year" | "all";
const DAY = 86_400_000;
const WEEKDAYS = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const bounds = (range: Range, now: number): [number, number] => {
  const to = now + DAY;
  if (range === "7d") return [now - 7 * DAY, to];
  if (range === "30d") return [now - 30 * DAY, to];
  if (range === "year") return [new Date(new Date(now).getFullYear(), 0, 1).getTime(), to];
  return [0, to];
};

const hours = (ms: number) => (ms >= 3_600_000 ? `${(ms / 3_600_000).toFixed(1)} h` : `${Math.round(ms / 60_000)} min`);
const dayLabel = (day: number) => new Date(day * DAY).toLocaleDateString([], { month: "short", day: "numeric", timeZone: "UTC" });

/** Bars as SVG so no inline style is needed. `values` are drawn left to right; the tallest fills the height. */
function Bars({ values, labels, title }: { values: number[]; labels: string[]; title: string }) {
  const max = Math.max(1, ...values);
  const w = 100 / values.length;
  return (
    <svg role="img" aria-label={title} viewBox="0 0 100 40" preserveAspectRatio="none" className="h-24 w-full">
      {values.map((v, i) => {
        const h = (v / max) * 38;
        return <rect key={i} x={i * w + w * 0.12} y={40 - h} width={w * 0.76} height={Math.max(h, v > 0 ? 0.8 : 0)} rx={0.6} className={v === max && v > 0 ? "fill-lumen-lavender" : "fill-white/25"}><title>{`${labels[i]}: ${v}`}</title></rect>;
      })}
    </svg>
  );
}

export default function Analytics() {
  const [range, setRange] = useState<Range>("30d");
  const [report, setReport] = useState<ListeningReport | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let active = true;
    const [from, to] = bounds(range, Date.now());
    statsReport(from, to).then((r) => { if (active) { setReport(r); setFailed(false); } }, () => { if (active) setFailed(true); });
    return () => { active = false; };
  }, [range]);

  const days = useMemo(() => {
    if (!report) return { values: [] as number[], labels: [] as string[] };
    const n = range === "7d" ? 7 : range === "30d" ? 30 : 0;
    const byDay = new Map(report.byDay.map((d) => [d.day, d.plays]));
    const first = report.byDay[0]?.day ?? 0;
    const last = report.byDay.at(-1)?.day ?? 0;
    const start = n ? last - n + 1 : first;
    const count = n || Math.min(Math.max(last - first + 1, 1), 366);
    const from = n ? start : Math.max(start, last - count + 1);
    const labels = Array.from({ length: count }, (_, i) => dayLabel(from + i));
    return { values: Array.from({ length: count }, (_, i) => byDay.get(from + i) ?? 0), labels };
  }, [report, range]);

  if (failed) return <p role="alert" className="text-[13px] text-secondary-text">Could not read the listening log. Check local storage.</p>;
  if (!report) return null;

  const peakHour = report.byHour.indexOf(Math.max(...report.byHour));
  const peakDay = report.byWeekday.indexOf(Math.max(...report.byWeekday));
  const skipRate = report.plays ? Math.round((report.skips / report.plays) * 100) : 0;
  const empty = report.plays === 0;
  const cards: [string, string][] = [
    ["Listening time", hours(report.listenedMs)],
    ["Plays", String(report.plays)],
    ["Active days", String(report.activeDays)],
    ["New songs", String(report.newTracks)],
    ["Skip rate", `${skipRate}%`],
    ["Avg. heard", `${report.avgCompletion}%`],
  ];

  return (
    <div className="flex flex-col gap-8">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <Segmented label="Time range" value={range} options={[{ key: "7d", label: "7 days" }, { key: "30d", label: "30 days" }, { key: "year", label: "This year" }, { key: "all", label: "All time" }]} onChange={setRange} />
        <p className="flex items-center gap-2 text-[13px] text-secondary-text"><Flame size={14} aria-hidden="true" />{report.currentStreak}-day streak · best {report.longestStreak}</p>
      </div>

      {empty ? (
        <p role="status" className="text-[13px] text-secondary-text">No plays in this range. Detailed history (time, hours, skips) is recorded from the version that added it; older plays only appear as totals under All devices.</p>
      ) : (
        <>
          {(range === "year" || range === "all") && report.topArtists[0] && report.topTracks[0] && (
            <section aria-label="Highlights" className="rounded-[16px] bg-lumen-wash p-5">
              <p className="text-[12px] text-secondary-text">{range === "year" ? "Your year so far" : "All time"}</p>
              <p className="mt-1 font-display text-[22px] leading-7 font-semibold">{hours(report.listenedMs)} of music across {report.activeDays} days</p>
              <p className="mt-2 text-[13px] text-secondary-text">Top artist <b className="text-primary-text">{report.topArtists[0].name}</b> · Top song <b className="text-primary-text">{report.topTracks[0].title}</b> · Best streak <b className="text-primary-text">{report.longestStreak} days</b></p>
            </section>
          )}
          <dl className="grid grid-cols-2 gap-3 min-[700px]:grid-cols-3">
            {cards.map(([k, v]) => <div key={k} className="rounded-[12px] bg-frost-200 px-4 py-3"><dt className="text-[12px] text-secondary-text">{k}</dt><dd className="font-display text-[24px] leading-8 font-semibold tabular-nums">{v}</dd></div>)}
          </dl>
          <section aria-labelledby="an-days" className="flex flex-col gap-2">
            <h2 id="an-days" className="text-[15px] font-semibold">Plays per day</h2>
            <Bars values={days.values} labels={days.labels} title="Plays per day" />
          </section>
          <div className="grid gap-6 min-[900px]:grid-cols-2">
            <section aria-labelledby="an-hours" className="flex flex-col gap-2">
              <h2 id="an-hours" className="text-[15px] font-semibold">By hour <span className="font-normal text-secondary-text">· peak {String(peakHour).padStart(2, "0")}:00</span></h2>
              <Bars values={report.byHour} labels={report.byHour.map((_, i) => `${String(i).padStart(2, "0")}:00`)} title="Plays by hour of day" />
            </section>
            <section aria-labelledby="an-week" className="flex flex-col gap-2">
              <h2 id="an-week" className="text-[15px] font-semibold">By weekday <span className="font-normal text-secondary-text">· peak {WEEKDAYS[peakDay]}</span></h2>
              <Bars values={report.byWeekday} labels={WEEKDAYS} title="Plays by weekday" />
            </section>
          </div>
          <div className="grid gap-6 min-[900px]:grid-cols-2">
            <section aria-labelledby="an-artists" className="flex flex-col gap-2">
              <h2 id="an-artists" className="text-[15px] font-semibold">Top artists</h2>
              <ol className="flex flex-col">
                {report.topArtists.map((a, i) => (
                  <li key={a.name} className="grid h-10 grid-cols-[24px_minmax(0,1fr)_auto] items-center gap-2 px-2 text-[13px]">
                    <span className="tabular-nums text-secondary-text">{i + 1}</span><span className="truncate">{a.name}</span><span className="tabular-nums text-secondary-text">{a.plays} · {hours(a.listenedMs)}</span>
                  </li>
                ))}
              </ol>
            </section>
            <section aria-labelledby="an-tracks" className="flex flex-col gap-2">
              <h2 id="an-tracks" className="text-[15px] font-semibold">Top songs</h2>
              <ol className="flex flex-col">
                {report.topTracks.map((t, i) => (
                  <li key={`${t.provider}:${t.providerTrackId}`} className="grid min-h-12 grid-cols-[24px_32px_minmax(0,1fr)_auto] items-center gap-2 rounded-lg px-2 hover:bg-frost-200 focus-within:bg-frost-200">
                    <span className="text-[12px] tabular-nums text-secondary-text">{i + 1}</span>
                    <TrackCover url={t.artworkUrl} videoId={t.providerTrackId} className="size-8" />
                    <button type="button" disabled={t.provider !== "YouTube"} aria-label={`Play ${t.title}`} onClick={() => void playTrack({ provider: t.provider, providerTrackId: t.providerTrackId, canonicalUrl: `https://www.youtube.com/watch?v=${t.providerTrackId}`, title: t.title, artists: t.artists, artworkUrl: t.artworkUrl })} className="flex min-w-0 items-center gap-2 rounded-md text-left disabled:opacity-60">
                      <span className="min-w-0"><span className="block truncate text-[13px] font-semibold">{t.title}</span><span className="block truncate text-[12px] text-secondary-text">{t.artists.join(", ")}</span></span>
                    </button>
                    <span className="flex items-center gap-1 text-[12px] tabular-nums text-secondary-text"><Play size={11} aria-hidden="true" />{t.plays}</span>
                  </li>
                ))}
              </ol>
            </section>
          </div>
        </>
      )}
    </div>
  );
}
