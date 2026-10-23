"use client";

import { useEffect, useRef } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { ChevronDown, ListMusic, MicVocal, Pause, Play, SkipBack, SkipForward } from "lucide-react";
import { useUi } from "../../lib/store";
import { AMBIENT_BG, Art } from "../Art";
import { IconButton, Scrubber, cx } from "../ui";
import { formatDuration, type QueueTrack } from "../../lib/media";
import { usePlayback } from "../../lib/playback";
import { NativeNowPlaying } from "./NativeNowPlaying";
import { DUR, SPRING, tween } from "../../lib/motion";

// Lyrics are sample text for one demo track. The active line is the last one whose start time has passed.
function Lyrics({ track, position }: { track: QueueTrack; position: number }) {
  const lines = track.lyrics ?? [];
  const activeIndex = lines.reduce((acc, line, i) => (line.at <= position ? i : acc), -1);
  const activeRef = useRef<HTMLParagraphElement>(null);
  const reduce = useReducedMotion();

  useEffect(() => {
    activeRef.current?.scrollIntoView({ block: "center", behavior: reduce ? "auto" : "smooth" });
  }, [activeIndex, reduce]);

  if (lines.length === 0) {
    return (
      <p className="self-center text-[22px] leading-7 font-semibold text-secondary-text">
        Demo lyrics are written for one sample track only. This track has none.
      </p>
    );
  }

  return (
    <div className="flex h-full flex-col gap-6 overflow-y-auto py-[38vh]" aria-label="Lyrics (demo sample)">
      {lines.map((line, i) => {
        const active = i === activeIndex;
        return (
          <p
            key={`${track.id}-${i}`}
            ref={active ? activeRef : undefined}
            aria-current={active ? "true" : undefined}
            className={cx(
              "origin-left font-display text-[32px] leading-[40px] font-bold tracking-[-0.02em] transition-[color,opacity,transform] duration-300 ease-[cubic-bezier(0.16,1,0.3,1)] 2xl:text-[40px] 2xl:leading-[48px]",
              active ? "scale-100 text-ink-bright" : "scale-[0.97] text-primary-text opacity-40",
            )}
          >
            {line.text}
          </p>
        );
      })}
    </div>
  );
}

export default function NowPlaying() {
  const s = useUi();
  const native = usePlayback((state) => state.requested !== null || state.track !== null);
  const reduce = useReducedMotion();
  const track = s.queue[s.index];
  const dur = track?.durationSec ?? 0;

  useEffect(() => {
    if (!s.nowPlayingOpen) return;
    function onKey(e: KeyboardEvent) {
      if (e.key === "Escape") s.setNowPlaying(false);
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [s.nowPlayingOpen, s]);

  if (native) return <NativeNowPlaying />;
  return (
    <AnimatePresence>
      {s.nowPlayingOpen && track && (
        <motion.div
          key="now-playing"
          role="dialog"
          aria-modal="true"
          aria-label="Now Playing (demo)"
          initial={reduce ? false : { opacity: 0, y: 48, scale: 0.96 }}
          animate={{ opacity: 1, y: 0, scale: 1, transition: { ...SPRING, opacity: tween(DUR.slow) } }}
          exit={reduce ? { opacity: 0 } : { opacity: 0, y: 48, scale: 0.96, transition: tween(DUR.base) }}
          style={{ transformOrigin: "50% 100%" }}
          className="fixed inset-0 z-40 overflow-hidden bg-night-canvas"
        >
          {/* Ambient field: the artwork itself, blurred, under a heavy scrim so text stays above 4.5:1.
              It cross-fades over 400ms when the track changes; the scrim stays put. */}
          <AnimatePresence initial={false}>
            <motion.div
              key={track.id}
              aria-hidden="true"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              transition={tween(0.4)}
              className="absolute inset-0"
            >
              <div className={cx("absolute inset-0", AMBIENT_BG[track.tone])} />
              <div className="absolute inset-0 scale-125 opacity-50 blur-[90px] saturate-150">
                <Art seed={track.album} tone={track.tone} grain={false} rounded="rounded-none" className="size-full" />
              </div>
            </motion.div>
          </AnimatePresence>
          <div aria-hidden="true" className="absolute inset-0 bg-[rgba(0,0,0,0.66)]" />

          <div className="relative mx-auto flex h-full max-w-content flex-col px-10 pb-8">
            {/* Top row: close chevron sits right of the traffic lights. Empty space drags the window. */}
            <div data-tauri-drag-region className="flex h-[52px] shrink-0 items-center gap-2 pl-[76px]">
              <IconButton label="Collapse Now Playing" onClick={() => s.setNowPlaying(false)} className="size-9">
                <ChevronDown size={22} />
              </IconButton>
            </div>

            <div
              className={cx(
                "grid min-h-0 flex-1 grid-cols-1 items-center gap-14",
                s.lyricsOpen ? "lg:grid-cols-[minmax(0,460px)_minmax(0,1fr)]" : "justify-items-center",
              )}
            >
              <div className="flex w-full max-w-[460px] flex-col gap-5">
                <Art seed={track.album} tone={track.tone} rounded="rounded-[16px]" className="aspect-square w-full shadow-[0_24px_64px_rgba(0,0,0,0.55)]" />
                <div className="min-w-0">
                  <h2 className="truncate font-display text-[22px] leading-7 font-semibold tracking-[-0.015em]">{track.title}</h2>
                  <p className="truncate text-[17px] leading-6 text-secondary-text">
                    {track.artist} · {track.album}
                  </p>
                </div>
              </div>

              {s.lyricsOpen && (
                <div className="flex h-full min-h-0 flex-col">
                  <Lyrics track={track} position={s.positionSec} />
                </div>
              )}
            </div>

            <div className="mx-auto mt-4 flex w-full max-w-[620px] flex-col gap-2">
              <div className="flex items-center gap-3 text-[11px] leading-4 tabular-nums text-secondary-text">
                <span className="w-9 text-right">{formatDuration(s.positionSec)}</span>
                <Scrubber
                  label="Seek (demo, no audio)"
                  valueText={`${formatDuration(s.positionSec)} of ${formatDuration(dur)}`}
                  max={dur}
                  value={s.positionSec}
                  onChange={s.seek}
                  className="flex-1"
                />
                <span className="w-9">-{formatDuration(Math.max(0, dur - s.positionSec))}</span>
              </div>
              <div className="relative flex items-center justify-center gap-5">
                <IconButton label="Previous (demo)" onClick={s.prev} className="size-11 text-primary-text">
                  <SkipBack size={26} fill="currentColor" />
                </IconButton>
                <button
                  type="button"
                  aria-label={s.playing ? "Pause (demo, no audio)" : "Play (demo, no audio)"}
                  onClick={s.togglePlay}
                  className="flex size-14 items-center justify-center rounded-full text-primary-text lx-press hover:scale-105"
                >
                  {s.playing ? <Pause size={36} fill="currentColor" /> : <Play size={36} fill="currentColor" className="translate-x-[2px]" />}
                </button>
                <IconButton label="Next (demo)" onClick={s.next} className="size-11 text-primary-text">
                  <SkipForward size={26} fill="currentColor" />
                </IconButton>
                <div className="absolute right-0 flex items-center gap-1">
                  <IconButton label="Lyrics (demo sample)" active={s.lyricsOpen} aria-pressed={s.lyricsOpen} onClick={() => s.setLyrics(!s.lyricsOpen)}>
                    <MicVocal size={18} />
                  </IconButton>
                  <IconButton label="Queue (demo)" active={s.queueOpen} aria-pressed={s.queueOpen} onClick={s.toggleQueue}>
                    <ListMusic size={18} />
                  </IconButton>
                </div>
              </div>
              <p className="text-center text-[11px] leading-4 text-secondary-text">Demo content · no audio is played and no provider is connected.</p>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
