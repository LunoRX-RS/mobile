"use client";

import { useEffect, useRef } from "react";
import { ListMusic, Pause, Play, SkipForward } from "lucide-react";
import { playerSkip } from "../../services/tauri/player";
import { TrackCover } from "../TrackCover";
import { useUi } from "../../lib/store";
import { usePlayback, togglePlayback, resumeLast } from "../../lib/playback";
import { IconButton } from "../ui";

// Phone mini player: floats above the tab bar, tap opens Now Playing. Renders nothing until a track is loaded or can
// be resumed; the demo queue of the desktop shell does not exist on mobile.
export default function MobilePlayerBar() {
  const s = usePlayback();
  const bar = useRef<HTMLDivElement>(null);
  const live = s.requested ?? s.track;
  const restoring = !live && s.resume !== null;
  const track = live ?? s.resume?.track ?? null;
  const info = s.info?.providerTrackId === track?.providerTrackId ? s.info : null;
  const title = info?.title ?? track?.title ?? "YouTube";
  const artists = (info?.artists ?? track?.artists ?? []).join(", ");
  const playing = s.player.state === "playing" || s.player.state === "buffering";
  const controllable = ["ready", "playing", "paused", "buffering"].includes(s.player.state);
  const durationMs = restoring ? s.resume!.durationMs : s.durationMs;
  const positionMs = restoring ? s.resume!.positionMs : s.positionMs;
  const pct = durationMs > 0 ? Math.min(100, (positionMs / durationMs) * 100) : 0;
  // The CSP forbids style attributes in the static HTML; setting the width from script is allowed.
  useEffect(() => { if (bar.current) bar.current.style.width = `${pct}%`; }, [pct]);
  if (!track && !s.error) return null;
  return (
    <section aria-label="Player" className="glass-bar relative mx-2 mb-1 shrink-0 overflow-hidden rounded-[14px]">
      <div className="flex items-center gap-2 py-1.5 pr-1 pl-1.5">
        <button
          type="button"
          aria-label={`Open Now Playing: ${title}`}
          onClick={() => {
            // A restored track has no player behind it yet, so there is nothing to open: tapping it continues playback.
            if (restoring) void resumeLast();
            useUi.getState().setNowPlaying(true);
          }}
          className="flex min-w-0 flex-1 items-center gap-3 rounded-[10px] text-left"
        >
          {track && <TrackCover key={track.providerTrackId} url={info?.artworkUrl ?? track.artworkUrl} videoId={track.providerTrackId} className="lx-fade-in size-11 shrink-0" />}
          <span className="min-w-0 flex-1">
            <span className="block truncate text-[14px] leading-5 font-semibold">{title}</span>
            <span role={s.error ? "alert" : undefined} className="block truncate text-[12px] leading-4 text-secondary-text">
              {s.error ?? (restoring ? "Tap play to continue" : s.busy ? "Preparing audio…" : artists)}
            </span>
          </span>
        </button>
        <IconButton
          label={restoring ? `Resume ${title}` : playing ? "Pause" : "Play"}
          disabled={s.busy || !track}
          onClick={() => void (restoring ? resumeLast() : togglePlayback())}
          className="ln-touch"
        >
          {playing ? <Pause size={26} fill="currentColor" /> : <Play size={26} fill="currentColor" />}
        </IconButton>
        <IconButton label="Queue" onClick={() => useUi.getState().toggleQueue()} className="ln-touch">
          <ListMusic size={22} />
        </IconButton>
        <IconButton label="Next track" disabled={s.busy || !live || !controllable} onClick={() => void playerSkip(true)} className="ln-touch">
          <SkipForward size={24} fill="currentColor" />
        </IconButton>
      </div>
      <div className="absolute inset-x-0 bottom-0 h-[2px] bg-white/10" aria-hidden="true">
        <div ref={bar} className="h-full w-0 bg-lumen-lavender" />
      </div>
    </section>
  );
}
