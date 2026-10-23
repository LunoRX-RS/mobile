"use client";

import {
  Heart,
  ListMusic,
  Maximize2,
  MicVocal,
  Pause,
  Play,
  Repeat,
  Repeat1,
  Shuffle,
  SkipBack,
  SkipForward,
  Volume1,
  Volume2,
  VolumeX,
} from "lucide-react";
import { useUi } from "../../lib/store";
import { formatDuration } from "../../lib/media";
import { Art } from "../Art";
import { IconButton, Scrubber } from "../ui";
import { Crossfade, PopOnTrue } from "../Motion";
import { usePlayback } from "../../lib/playback";
import { NativePlayerBar } from "./NativePlayer";
import { DevicesButton, RemotePlayerBar } from "./Devices";
import { useRemote } from "../../lib/remote";

// Thin translucent bar. Everything here is a demo control: no audio is produced.
export default function PlayerBar() {
  const s = useUi();
  const native = usePlayback((p) => p.requested !== null || p.track !== null || p.resume !== null || p.busy || p.error !== null);
  const driving = useRemote((r) => r.target !== null);
  if (driving) return <RemotePlayerBar />;
  if (native) return <NativePlayerBar />;
  const track = s.queue[s.index];
  const dur = track?.durationSec ?? 0;
  const liked = track ? s.liked[track.id] === true : false;
  const VolumeIcon = s.volume === 0 ? VolumeX : s.volume < 50 ? Volume1 : Volume2;

  return (
    <footer aria-label="Player (demo)" className="glass-bar flex h-[var(--height-player)] shrink-0 items-center gap-4 border-t border-hairline px-4">
      {/* Track */}
      <div className="flex w-[30%] min-w-0 items-center gap-3">
        {track ? (
          <>
            <button type="button" aria-label="Open Now Playing (demo)" onClick={() => s.setNowPlaying(true)} className="shrink-0 rounded-[6px]">
              <Crossfade k={track.id}>
                <Art seed={track.album} tone={track.tone} grain={false} rounded="rounded-[6px]" className="size-14 shadow-[0_1px_4px_rgba(0,0,0,0.5)]" />
              </Crossfade>
            </button>
            <div className="min-w-0">
              <p className="truncate text-[13px] leading-[18px] font-semibold">{track.title}</p>
              <p className="truncate text-[13px] leading-[18px] text-secondary-text">{track.artist}</p>
            </div>
            <IconButton label={liked ? "Remove from Liked Songs (demo)" : "Save to Liked Songs (demo)"} active={liked} aria-pressed={liked} onClick={() => s.toggleLike(track.id)}>
              <PopOnTrue on={liked}>
                <Heart size={15} fill={liked ? "currentColor" : "none"} />
              </PopOnTrue>
            </IconButton>
          </>
        ) : (
          <p className="text-[13px] text-secondary-text">Nothing loaded.</p>
        )}
      </div>

      {/* Transport and seek */}
      <div className="flex min-w-0 flex-1 flex-col items-center gap-0.5">
        <div className="flex items-center gap-1.5">
          <IconButton label="Shuffle (demo)" active={s.shuffle} aria-pressed={s.shuffle} onClick={s.toggleShuffle}>
            <Shuffle size={15} />
          </IconButton>
          <IconButton label="Previous (demo)" onClick={s.prev} disabled={!track} className="text-primary-text">
            <SkipBack size={18} fill="currentColor" />
          </IconButton>
          <button
            type="button"
            aria-label={s.playing ? "Pause (demo, no audio)" : "Play (demo, no audio)"}
            onClick={s.togglePlay}
            disabled={!track}
            className="flex size-9 items-center justify-center rounded-full text-primary-text lx-press hover:scale-105 disabled:opacity-40"
          >
            {s.playing ? <Pause size={26} fill="currentColor" /> : <Play size={26} fill="currentColor" className="translate-x-[1px]" />}
          </button>
          <IconButton label="Next (demo)" onClick={s.next} disabled={!track} className="text-primary-text">
            <SkipForward size={18} fill="currentColor" />
          </IconButton>
          <IconButton
            label={s.repeat === "one" ? "Repeat one (demo)" : s.repeat === "all" ? "Repeat all (demo)" : "Repeat off (demo)"}
            active={s.repeat !== "off"}
            aria-pressed={s.repeat !== "off"}
            onClick={s.cycleRepeat}
          >
            {s.repeat === "one" ? <Repeat1 size={15} /> : <Repeat size={15} />}
          </IconButton>
        </div>

        <div className="flex w-full max-w-[560px] items-center gap-2 text-[11px] leading-4 tabular-nums text-secondary-text">
          <span className="w-9 text-right">{formatDuration(s.positionSec)}</span>
          <Scrubber
            label="Seek (demo, no audio)"
            valueText={`${formatDuration(s.positionSec)} of ${formatDuration(dur)}`}
            max={dur}
            value={s.positionSec}
            disabled={!track}
            onChange={s.seek}
            className="flex-1"
          />
          <span className="w-9">{formatDuration(dur)}</span>
        </div>
      </div>

      {/* Right cluster */}
      <div className="flex w-[30%] items-center justify-end gap-0.5">
        <IconButton label="Lyrics (demo sample)" active={s.lyricsOpen} aria-pressed={s.lyricsOpen} onClick={() => { s.setNowPlaying(true); s.setLyrics(true); }}>
          <MicVocal size={16} />
        </IconButton>
        <IconButton label="Queue (demo)" active={s.queueOpen} aria-pressed={s.queueOpen} onClick={s.toggleQueue}>
          <ListMusic size={16} />
        </IconButton>
        <DevicesButton />
        <div className="ml-2 flex items-center gap-1.5 text-secondary-text">
          <VolumeIcon size={16} aria-hidden="true" />
          <Scrubber label="Volume (demo, no audio)" max={100} value={s.volume} onChange={s.setVolume} className="w-24" />
        </div>
        <IconButton label="Now Playing (demo)" onClick={() => s.setNowPlaying(true)} className="ml-1">
          <Maximize2 size={15} />
        </IconButton>
      </div>
    </footer>
  );
}
