"use client";
import { ArtistLinks, LikeButton } from "../LibraryActions";

import { useEffect } from "react";
import { ListMusic, Pause, Play, Square, SkipBack, SkipForward, Volume2, MicVocal, Maximize2 } from "lucide-react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { RepeatButton, AutoplayButton } from "../PlayModeButtons";
import { playerProgress, playerSkip, playerTrackInfo, subscribePlayerProgress, subscribePlayerState } from "../../services/tauri/player";
import { TrackCover } from "../TrackCover";
import { useUi } from "../../lib/store";
import { usePlayback, togglePlayback, stopPlayback, seekPlayback, setPlaybackVolume, initLastPlayed, resumeLast, clearResume } from "../../lib/playback";
import { formatDuration } from "../../lib/media";
import { IconButton, Scrubber } from "../ui";
import { DevicesButton } from "./Devices";

export function PlaybackEvents() {
  // Song details (lyrics, artists) come from YouTube Music; an episode has none to look up.
  const trackId = usePlayback((s) => { const t = s.requested ?? s.track; return t && t.provider !== "Podcast" ? t.providerTrackId : undefined; });
  const request = usePlayback((s) => s.requested);
  useEffect(() => {
    let active = true;
    usePlayback.setState({ info: null, infoState: trackId ? "loading" : "idle" });
    if (trackId) {
      playerTrackInfo(trackId).then((info) => {
        if (active) usePlayback.setState({ info, infoState: "ready" });
      }).catch(() => { if (active) usePlayback.setState({ infoState: "error" }); });
    }
    return () => { active = false; };
  }, [trackId, request]);
  useEffect(() => initLastPlayed(), []);
  useEffect(() => {
    let active = true;
    const listeners: UnlistenFn[] = [];
    const install = (stop: UnlistenFn) => { if (active) listeners.push(stop); else stop(); };
    subscribePlayerState((dto) => { if (active) usePlayback.getState().acceptState(dto); }).then(install).catch(() => {});
    subscribePlayerProgress((dto) => { if (active) usePlayback.getState().acceptProgress(dto); }).then(install).catch(() => {});
    playerProgress().then((dto) => { if (active) usePlayback.getState().acceptProgress(dto); }).catch(() => {});
    return () => { active = false; listeners.forEach((stop) => stop()); };
  }, []);
  return null;
}

export function NativePlayerBar() {
  const s = usePlayback();
  const queueOpen = useUi((u) => u.queueOpen);
  const live = s.requested ?? s.track;
  const restoring = !live && s.resume !== null;
  const track = live ?? s.resume?.track ?? null;
  const shownPositionMs = restoring ? s.resume!.positionMs : s.positionMs;
  const shownDurationMs = restoring ? s.resume!.durationMs : s.durationMs;
  const resumeAt = formatDuration(shownPositionMs / 1000);
  const info = s.info?.providerTrackId === track?.providerTrackId ? s.info : null;
  const title = info?.title ?? track?.title ?? "YouTube";
  const playing = s.player.state === "playing" || s.player.state === "buffering";
  const controllable = ["ready", "playing", "paused", "buffering"].includes(s.player.state);
  return (
    <footer aria-label="YouTube player" className="glass-bar flex min-h-[var(--height-player)] shrink-0 items-center gap-4 border-t border-hairline px-4">
      <div className="flex w-[30%] min-w-0 items-center gap-3">
        {track && <button type="button" aria-label="Open Now Playing" onClick={() => useUi.getState().setNowPlaying(true)} className="shrink-0 rounded-[6px]">
          <TrackCover key={track.providerTrackId} url={info?.artworkUrl ?? track.artworkUrl} videoId={track.providerTrackId} className="lx-fade-in size-12" />
        </button>}
        <div className="min-w-0 flex-1">
        <p className="truncate text-[13px] font-semibold">{title}</p>
        <p className="truncate text-[11px] text-secondary-text">{track && <ArtistLinks track={{ provider: track.provider, artists: info?.artists ?? track.artists ?? [], artistRefs: info?.artistRefs ?? track.artistRefs }} />}</p>
        {(s.error || restoring || s.busy) && <p role={s.error ? "alert" : "status"} title={s.error ?? undefined} className="line-clamp-2 text-[11px] leading-4 text-secondary-text">
          {s.error ?? (restoring ? `Stopped at ${resumeAt}. Press play to continue` : "Preparing audio…")}
        </p>}
        {live && ["playing", "buffering", "paused", "ended"].includes(s.player.state) && <button type="button" aria-label="Open autoplay playlist" onClick={() => useUi.setState({queueOpen:true})} className="block max-w-full truncate rounded text-left text-[11px] text-secondary-text hover:underline">{s.autoplay.state === "ready" ? `Up next · ${s.autoplay.nextTitle} · ${s.autoplay.queue?.length ?? 0} songs` : s.autoplay.state === "preparing" ? `Preparing next · ${s.autoplay.nextTitle}` : s.autoplay.state === "finding" ? "Autoplay · finding related music…" : s.autoplay.state === "unavailable" ? "Autoplay unavailable · no related track could be prepared" : ""}</button>}
        </div>
        <span role="status" className="sr-only">{`YouTube · ${s.player.state}`}</span>
        {track && <LikeButton track={{ ...track, title, artists: info?.artists ?? track.artists ?? [], artistRefs: info?.artistRefs ?? track.artistRefs, artworkUrl: info?.artworkUrl ?? track.artworkUrl ?? null, durationMs: shownDurationMs || null }} />}
        {playing && <span className="lx-eq shrink-0" data-live={s.player.state === "playing"} aria-hidden="true"><i /><i /><i /></span>}
      </div>
      <div className="flex min-w-0 flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-2">
          <RepeatButton />
          <IconButton label="Previous track" disabled={s.busy || !live || !controllable} onClick={() => void playerSkip(false)}><SkipBack size={20} fill="currentColor" /></IconButton>
          <IconButton label={restoring ? `Resume ${title} from ${resumeAt}` : playing ? "Pause YouTube" : "Play YouTube"} disabled={s.busy || !track} onClick={() => void (restoring ? resumeLast() : togglePlayback())}>
            {playing ? <Pause size={24} /> : <Play size={24} />}
          </IconButton>
          <IconButton label="Next track" disabled={s.busy || !live || !controllable} onClick={() => void playerSkip(true)}><SkipForward size={20} fill="currentColor" /></IconButton>
          <AutoplayButton />
          <IconButton label={restoring ? "Forget the last track" : "Stop YouTube"} disabled={s.busy} onClick={() => void (restoring ? clearResume() : stopPlayback())}><Square size={15} /></IconButton>
        </div>
        <div className="flex w-full max-w-[560px] items-center gap-2 text-[11px] tabular-nums text-secondary-text">
          <span>{formatDuration(shownPositionMs / 1000)}</span>
          <Scrubber label="Seek YouTube" max={Math.max(1, shownDurationMs / 1000)} value={shownPositionMs / 1000}
            disabled={restoring || !controllable || !s.durationMs} onCommit={(value) => void seekPlayback(value)} className="flex-1" />
          <span>{formatDuration(shownDurationMs / 1000)}</span>
        </div>
      </div>
      <div className="flex w-[20%] items-center justify-end gap-2 text-secondary-text">
        <IconButton label="Show lyrics" onClick={() => { useUi.getState().setLyrics(true); useUi.getState().setNowPlaying(true); }}><MicVocal size={17} /></IconButton>
        <DevicesButton />
        <IconButton label="Queue" active={queueOpen} aria-pressed={queueOpen} onClick={() => useUi.getState().toggleQueue()}><ListMusic size={17} /></IconButton>
        <IconButton label="Open Now Playing" onClick={() => useUi.getState().setNowPlaying(true)}><Maximize2 size={17} /></IconButton>
        <Volume2 size={16} aria-hidden="true" />
        <Scrubber label="YouTube volume" max={100} value={s.volume} disabled={!controllable}
          onChange={(value) => void setPlaybackVolume(value)} className="w-24" />
      </div>
    </footer>
  );
}
