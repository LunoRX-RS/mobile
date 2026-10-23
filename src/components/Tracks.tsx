"use client";

import { Pause, Play } from "lucide-react";
import { useUi } from "../lib/store";
import { formatDuration, type QueueTrack } from "../lib/media";
import { Art } from "./Art";
import { cx } from "./ui";

// Shared by Home and the details view. Everything here is a demo control: no audio is produced.

// Art seed is album plus track id, so two tracks of one album (or one track repeated) never share a cover.
export const thumbSeed = (t: QueueTrack) => `${t.album}:${t.id}`;

export const playLabel = (t: QueueTrack, current: boolean, playing: boolean) =>
  `${current && playing ? "Pause" : "Play"} ${t.title} by ${t.artist}, demo, no audio`;

// Which sample track is current in the simulated queue, and whether it is "playing".
export function usePlayTrack() {
  const queue = useUi((s) => s.queue);
  const index = useUi((s) => s.index);
  const playing = useUi((s) => s.playing);
  const playIndex = useUi((s) => s.playIndex);
  const togglePlay = useUi((s) => s.togglePlay);
  const currentId = queue[index]?.id;
  return {
    isCurrent: (id: string) => id === currentId,
    isPlaying: (id: string) => id === currentId && playing,
    activate: (id: string) => {
      if (id === currentId) togglePlay();
      else playIndex(Math.max(0, queue.findIndex((t) => t.id === id)));
    },
    // Start a list of sample tracks from its first entry; pause if that entry is already playing.
    startList: (ids: string[]) => {
      const first = ids[0];
      if (!first) return;
      if (first === currentId || ids.includes(currentId ?? "")) togglePlay();
      else playIndex(Math.max(0, queue.findIndex((t) => t.id === first)));
    },
  };
}

// Small lavender equaliser. Lavender is live state only; it shows while the simulated track is "playing".
export function Equaliser() {
  return (
    <span aria-hidden="true" className="lx-eq">
      <i />
      <i />
      <i />
    </span>
  );
}

function PlayGlyph({ playing, size }: { playing: boolean; size: number }) {
  return playing ? (
    <Pause size={size} fill="currentColor" aria-hidden="true" />
  ) : (
    <Play size={size} fill="currentColor" className="translate-x-[0.5px]" aria-hidden="true" />
  );
}

// 56px row: 40px thumbnail with the play button over it, title, artist, provider, duration.
// The play button is the only control; the row itself is not interactive, so nothing is nested.
export function TrackRow({ t }: { t: QueueTrack }) {
  const p = usePlayTrack();
  const current = p.isCurrent(t.id);
  const playing = p.isPlaying(t.id);
  return (
    <li
      onDoubleClick={() => p.activate(t.id)}
      className={cx(
        "lx-reveal-host grid h-14 grid-cols-[40px_minmax(0,1fr)_auto] items-center gap-3 rounded-[8px] px-2 transition-colors duration-150 md:grid-cols-[40px_minmax(0,1fr)_112px_44px]",
        current ? "bg-lumen-wash" : "hover:bg-frost-200 focus-within:bg-frost-200",
      )}
    >
      <div className="relative size-10">
        <Art seed={thumbSeed(t)} tone={t.tone} grain={false} rounded="rounded-[6px]" className="size-10" />
        <button
          type="button"
          aria-label={playLabel(t, current, playing)}
          onClick={() => p.activate(t.id)}
          className={cx(
            "absolute inset-0 flex items-center justify-center rounded-[6px] bg-black/55 text-white hover:bg-black/70",
            current ? "text-lumen-lavender" : "lx-reveal",
          )}
        >
          {playing ? <Equaliser /> : <PlayGlyph playing={false} size={14} />}
        </button>
      </div>
      <div className="min-w-0">
        <p className="truncate text-[13px] leading-[18px] font-semibold" title={t.title}>
          {t.title}
        </p>
        <p className="truncate text-[13px] leading-[18px] text-secondary-text" title={`${t.artist}, ${t.provider}`}>
          {t.artist}
          <span className="md:hidden"> · {t.provider}</span>
        </p>
      </div>
      <p className="hidden truncate text-[13px] text-secondary-text md:block" title={t.provider}>
        {t.provider}
      </p>
      <p className="text-right text-[12px] tabular-nums text-secondary-text">{formatDuration(t.durationSec)}</p>
    </li>
  );
}

// Song table: sentence-case gray header, hairlines between rows. The number cell becomes the play button on
// hover and on keyboard focus.
export function SongTable({ tracks, label }: { tracks: QueueTrack[]; label: string }) {
  const p = usePlayTrack();
  return (
    <table className="w-full table-fixed border-separate border-spacing-0 text-[13px] leading-[18px]">
      <caption className="sr-only">{label}</caption>
      <colgroup>
        <col className="w-12" />
        <col />
        <col className="hidden lg:table-column lg:w-[24%]" />
        <col className="hidden md:table-column md:w-[132px]" />
        <col className="w-16" />
      </colgroup>
      <thead>
        <tr className="[&>th]:border-b [&>th]:border-hairline text-left text-[12px] leading-4 font-semibold text-secondary-text">
          <th scope="col" className="h-8 px-2 text-center font-semibold">
            #
          </th>
          <th scope="col" className="px-2 font-semibold">
            Title
          </th>
          <th scope="col" className="hidden px-2 font-semibold lg:table-cell">
            Album
          </th>
          <th scope="col" className="hidden px-2 font-semibold md:table-cell">
            Provider
          </th>
          <th scope="col" className="px-2 text-right font-semibold">
            Time
          </th>
        </tr>
      </thead>
      <tbody className="lx-stagger">
        {tracks.map((t, i) => {
          const current = p.isCurrent(t.id);
          const playing = p.isPlaying(t.id);
          return (
            <tr
              key={t.id}
              onDoubleClick={() => p.activate(t.id)}
              className={cx(
                "lx-reveal-host h-14 [&>td]:border-t [&>td]:border-hairline [&:first-child>td]:border-t-0 [&>td]:px-2 [&>td:first-child]:rounded-l-[8px] [&>td:last-child]:rounded-r-[8px]",
                current ? "*:bg-lumen-wash" : "hover:*:bg-frost-200 focus-within:*:bg-frost-200",
              )}
            >
              <td className="text-center">
                <div className="relative mx-auto size-8">
                  <span
                    aria-hidden="true"
                    className={cx(
                      "lx-hide absolute inset-0 flex items-center justify-center text-[12px] tabular-nums",
                      current ? "text-lumen-lavender" : "text-secondary-text",
                    )}
                  >
                    {playing ? <Equaliser /> : i + 1}
                  </span>
                  <button
                    type="button"
                    aria-label={playLabel(t, current, playing)}
                    onClick={() => p.activate(t.id)}
                    className="lx-reveal absolute inset-0 flex items-center justify-center rounded-full text-primary-text hover:bg-frost-300"
                  >
                    <PlayGlyph playing={playing} size={13} />
                  </button>
                </div>
              </td>
              <td>
                <div className="flex min-w-0 items-center gap-3">
                  <Art seed={thumbSeed(t)} tone={t.tone} grain={false} rounded="rounded-[6px]" className="size-10 shrink-0" />
                  <div className="min-w-0">
                    <p className="truncate font-semibold" title={t.title}>
                      {t.title}
                    </p>
                    <p className="truncate text-secondary-text" title={t.artist}>
                      {t.artist}
                    </p>
                  </div>
                </div>
              </td>
              <td className="hidden truncate text-secondary-text lg:table-cell" title={t.album}>
                {t.album}
              </td>
              <td className="hidden truncate text-secondary-text md:table-cell" title={t.provider}>
                {t.provider}
              </td>
              <td className="text-right text-[12px] tabular-nums text-secondary-text">{formatDuration(t.durationSec)}</td>
            </tr>
          );
        })}
      </tbody>
    </table>
  );
}

