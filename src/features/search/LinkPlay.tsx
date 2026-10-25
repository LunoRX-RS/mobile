"use client";

import { useEffect, useState } from "react";
import { Link2, Play } from "lucide-react";
import { Button } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import { playTrack, usePlayback } from "../../lib/playback";
import type { YouTubeLink } from "../../lib/ytLink";
import { playerTrackInfo, type LoadedTrack } from "../../services/tauri/player";

/** The track a pasted link points at, with its title and artwork once core has read them. */
export function linkTrack(link: YouTubeLink, info?: { title: string; artists: string[]; artworkUrl: string | null }): LoadedTrack {
  return {
    source: link.music ? "YouTube Music" : "YouTube",
    provider: "YouTube",
    providerTrackId: link.id,
    canonicalUrl: `https://www.youtube.com/watch?v=${link.id}`,
    title: info?.title || "YouTube video",
    artists: info?.artists ?? [],
    artworkUrl: info?.artworkUrl ?? null,
  };
}

/** Shown when Search holds a YouTube or YouTube Music link: play it directly, no search needed. */
export function LinkPlay({ link }: { link: YouTubeLink }) {
  const busy = usePlayback((s) => s.busy);
  const [info, setInfo] = useState<{ title: string; artists: string[]; artworkUrl: string | null } | null>(null);
  const [state, setState] = useState<"loading" | "ready" | "unknown">("loading");

  useEffect(() => {
    let live = true;
    setInfo(null);
    setState("loading");
    playerTrackInfo(link.id).then(
      (t) => { if (live) { setInfo({ title: t.title, artists: t.artists, artworkUrl: t.artworkUrl }); setState("ready"); } },
      () => { if (live) setState("unknown"); },
    );
    return () => { live = false; };
  }, [link.id]);

  const track = linkTrack(link, info ?? undefined);
  return (
    <section aria-label="Play from link" className="glass-card flex items-center gap-4 rounded-[16px] p-4">
      <TrackCover url={track.artworkUrl ?? null} videoId={link.id} title={track.title} className="size-16 shrink-0 rounded-[8px]" />
      <div className="min-w-0 flex-1">
        <p className="flex items-center gap-1.5 text-[12px] leading-4 text-secondary-text">
          <Link2 size={14} aria-hidden="true" />
          {link.music ? "YouTube Music link" : "YouTube link"}
        </p>
        <p className="truncate text-[15px] leading-5 font-semibold text-primary-text">
          {state === "loading" ? "Reading the link…" : track.title}
        </p>
        <p className="truncate text-[13px] leading-[18px] text-secondary-text">
          {state === "unknown" ? "Title not available. It can still play." : track.artists?.join(", ")}
        </p>
      </div>
      <Button variant="primary" disabled={busy} onClick={() => void playTrack(track)}>
        <Play size={16} fill="currentColor" aria-hidden="true" />
        Play
      </Button>
    </section>
  );
}
