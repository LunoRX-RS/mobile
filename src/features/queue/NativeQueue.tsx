"use client";

import { GripVertical, ListMusic, Loader2, X } from "lucide-react";
import { Reorder, useDragControls } from "motion/react";
import { playerQueueEdit, playerQueueMore } from "../../services/tauri/player";
import { usePlayback, playTrack } from "../../lib/playback";
import { useUi } from "../../lib/store";
import { TrackCover } from "../../components/TrackCover";
import { LikeButton } from "../../components/LibraryActions";
import { formatDuration } from "../../lib/media";
import { Button, EmptyState, IconButton } from "../../components/ui";
import { useCallback, useEffect, useRef, useState } from "react";
import { addToPlaylist } from "../../lib/playlistActions";
import { syncCreateLocalPlaylist } from "../../services/tauri/sync";

const NEXT_STATUS: Record<string, string> = {
  ready: "Ready to play after this song",
  preparing: "Preparing the audio",
  finding: "Finding related music",
  unavailable: "No related track could be prepared",
};

/** The most songs core keeps up next (QUEUE_MAX in autoplay.rs); at this size there is nothing more to load. */
const QUEUE_CAP = 500;

type Upcoming = ReturnType<typeof usePlayback.getState>["autoplay"]["queue"][number];

const toHit = (item: Upcoming) => ({ provider: item.provider, source: "YouTube Music" as const, providerTrackId: item.id, canonicalUrl: `https://www.youtube.com/watch?v=${item.id}`, title: item.title, artists: item.artists, artworkUrl: item.artworkUrl, durationMs: item.durationMs });

/** One up-next row. Only the grip starts a drag, so scrolling the list with a finger still works. */
function QueueRow({ item, index, busy, status, onDrop }: { item: Upcoming; index: number; busy: boolean; status: string | null; onDrop: (id: string) => void }) {
  const controls = useDragControls();
  const hit = toHit(item);
  return (
    <Reorder.Item value={item} as="li" dragListener={false} dragControls={controls} onDragEnd={() => onDrop(item.id)}
      whileDrag={{ scale: 1.02, boxShadow: "0 12px 32px rgba(0,0,0,0.5)", zIndex: 5 }}
      className="grid min-h-16 grid-cols-[36px_40px_minmax(0,1fr)_auto] items-center gap-2 rounded-lg bg-transparent px-1 hover:bg-frost-200 focus-within:bg-frost-200">
      <button type="button" aria-label={`Drag to move ${item.title}`} onPointerDown={(e) => controls.start(e)}
        className="flex h-11 w-9 cursor-grab touch-none items-center justify-center rounded-md text-secondary-text active:cursor-grabbing"><GripVertical size={18} aria-hidden="true" /></button>
      <button type="button" disabled={busy} onClick={() => void playTrack(hit, true)} aria-label={`Play ${item.title} from autoplay queue`} className="rounded-md disabled:opacity-40"><TrackCover url={item.artworkUrl} videoId={item.id} className="size-10" /></button>
      <button type="button" disabled={busy} onClick={() => void playTrack(hit, true)} className="min-w-0 rounded-md text-left disabled:opacity-40">
        <p className="truncate text-[13px] font-semibold" title={item.title}>{item.title}</p>
        <p className="truncate text-[12px] text-secondary-text">{item.artists.join(", ")}{item.durationMs != null ? ` · ${formatDuration(item.durationMs / 1000)}` : ""}</p>
        {index === 0 && status && <p role="status" className="text-[11px] text-secondary-text">{status}</p>}
      </button>
      <div className="flex items-center gap-0.5">
        <IconButton label={`Remove ${item.title} from the queue`} onClick={() => void playerQueueEdit({ op: "remove", id: item.id }).catch(() => {})} className="size-9"><X size={16} /></IconButton>
        <LikeButton track={hit} />
      </div>
    </Reorder.Item>
  );
}

/** Loads more related songs when the end of the list scrolls into view, one request at a time. It never gives up:
 * after an empty answer it waits a little longer each time and asks again while the end is still on screen. */
function useLoadMore(enabled: boolean, resetKey: string | undefined, remaining: number) {
  // State, not a plain ref: the effect below runs again when the sentinel is drawn later or replaced.
  const [sentinel, setSentinel] = useState<HTMLLIElement | null>(null);
  const [loading, setLoading] = useState(false);
  const [retry, setRetry] = useState(0);
  const misses = useRef(0);
  const inFlight = useRef(false);
  useEffect(() => { misses.current = 0; }, [resetKey]);
  const load = useCallback(async () => {
    if (inFlight.current) return;
    inFlight.current = true;
    setLoading(true);
    let added = 0;
    try { added = await playerQueueMore(); } catch { /* counted as a miss below */ }
    misses.current = added > 0 ? 0 : misses.current + 1;
    inFlight.current = false;
    setLoading(false);
    // Always look again once a load ends: core reports the longer queue before this call returns, so the "fewer than 30
    // left" check may already have run while this load was in flight. After an empty answer wait a little longer each time.
    window.setTimeout(() => setRetry((n) => n + 1), added > 0 ? 0 : Math.min(800 * 2 ** (misses.current - 1), 8000));
  }, []);
  useEffect(() => {
    if (!enabled || !sentinel) return;
    const observer = new IntersectionObserver((entries) => { if (entries.some((e) => e.isIntersecting)) void load(); }, { rootMargin: "900px" });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, [enabled, sentinel, load, retry]); // not `loading`: the observer stays put while a load runs; `retry` re-arms it after an empty answer
  // Keep a good stretch of songs ready ahead of the scroll, so the end of the list is rarely reached.
  useEffect(() => { if (enabled && remaining < 30) void load(); }, [enabled, remaining, load, retry]);
  return { sentinel: setSentinel, loading };
}

export default function NativeQueue({ bare = false }: { bare?: boolean } = {}) {
  const s = usePlayback();
  const navigate = useUi((u) => u.navigate);
  const toggleQueue = useUi((u) => u.toggleQueue);
  const track = s.requested ?? s.track;
  const info = s.info?.providerTrackId === track?.providerTrackId ? s.info : null;
  const title = info?.title ?? track?.title ?? "YouTube";
  const artists = (info?.artists ?? track?.artists)?.join(", ") ?? "";
  const playing = s.player.state === "playing" || s.player.state === "buffering";
  const next = s.autoplay;
  const upcoming = next.queue ?? [];
  // The list as the user is dragging it; it follows core again as soon as core reports a new order.
  const [order, setOrder] = useState(upcoming);
  const dragging = useRef(false);
  const signature = upcoming.map((t) => t.id).join(",");
  useEffect(() => { if (!dragging.current) setOrder(upcoming); }, [signature]);
  // The first frame only draws what is on screen; the rest follows on the next frame so opening stays smooth.
  const [drawn, setDrawn] = useState(10);
  useEffect(() => { const id = window.setTimeout(() => setDrawn(Number.MAX_SAFE_INTEGER), 80); return () => window.clearTimeout(id); }, []);
  const more = useLoadMore(!!track && upcoming.length > 0 && upcoming.length < QUEUE_CAP && next.state !== "finding", track?.providerTrackId, upcoming.length);
  const drop = (id: string) => {
    dragging.current = false;
    const from = upcoming.findIndex((t) => t.id === id);
    const to = order.findIndex((t) => t.id === id);
    if (from < 0 || to < 0 || from === to) { setOrder(upcoming); return; }
    void playerQueueEdit({ op: "move", from, to }).catch(() => setOrder(upcoming));
  };
  const [saved, setSaved] = useState<"" | "saving" | "done" | "failed">("");
  async function saveQueue() {
    if (!track) return;
    setSaved("saving");
    try {
      const id = await syncCreateLocalPlaylist(`Queue · ${new Date().toLocaleDateString()}`, null);
      for (const hit of [{ ...track, source: "YouTube Music" as const, artists: track.artists ?? [], artworkUrl: track.artworkUrl ?? null, durationMs: s.durationMs || null }, ...upcoming.map(toHit)]) await addToPlaylist(id, hit);
      setSaved("done");
    } catch { setSaved("failed"); }
  }

  if (!track) {
    return (
      <section aria-labelledby="queue-title" className="flex flex-col gap-4">
        <h2 id="queue-title" className="font-display text-[22px] leading-7 font-semibold tracking-[-0.015em]">Queue</h2>
        <EmptyState
          icon={<ListMusic size={24} />}
          title="Nothing in the queue"
          action={<Button variant="secondary" className="mt-1" onClick={() => { toggleQueue(); navigate("search"); }}>Find songs</Button>}
        >
          Play a song and it appears here.
        </EmptyState>
      </section>
    );
  }

  return (
    <section aria-labelledby="queue-title" className="flex flex-col gap-4">
      <header className={bare ? "hidden" : "flex flex-col gap-0.5"}>
        <h2 id="queue-title" className="font-display text-[22px] leading-7 font-semibold tracking-[-0.015em]">Queue</h2>
        <p className="text-[13px] leading-[18px] text-secondary-text">
          Autoplay · {upcoming.length} songs up next. Select any song to continue from there.
        </p>
        <Button variant="secondary" className="mt-2 self-start" disabled={saved === "saving"} onClick={() => void saveQueue()}>
          {saved === "saving" ? "Saving…" : saved === "done" ? "Saved to Playlists" : saved === "failed" ? "Could not save. Retry" : "Save queue as playlist"}
        </Button>
      </header>

      <section aria-labelledby="queue-now" className="flex flex-col gap-1">
        <h3 id="queue-now" className="px-2 text-[13px] leading-[18px] font-semibold text-secondary-text">Now playing</h3>
        <div className="grid h-14 grid-cols-[40px_minmax(0,1fr)_auto] items-center gap-3 rounded-[8px] bg-lumen-wash px-2" aria-current="true">
          <TrackCover key={track.providerTrackId} url={info?.artworkUrl ?? track.artworkUrl} videoId={track.providerTrackId} className="size-10 rounded-[6px]" />
          <div className="min-w-0">
            <p className="truncate text-[13px] leading-[18px] font-semibold" title={title}>{title}</p>
            <p className="truncate text-[13px] leading-[18px] text-secondary-text" title={artists}>{artists || "YouTube Music"}</p>
          </div>
          <p className="text-right text-[12px] text-secondary-text">{playing ? "Playing" : s.player.state === "paused" ? "Paused" : ""}</p>
        </div>
      </section>

      <section aria-labelledby="queue-next" className="flex flex-col gap-1">
        <h3 id="queue-next" className="px-2 text-[13px] leading-[18px] font-semibold text-secondary-text">Up next</h3>
        {next.state === "finding" && <p role="status" className="flex items-center gap-2 px-2 py-2 text-[13px] text-secondary-text"><Loader2 size={16} className="lx-spin" aria-hidden="true" />Finding up to 50 related songs…</p>}
        {upcoming.length > 0 ? <Reorder.Group as="ol" axis="y" values={order.slice(0, drawn)} onReorder={(v) => { dragging.current = true; setOrder([...v, ...order.slice(drawn)]); }} aria-label="Autoplay playlist" className="flex flex-col">
          {order.slice(0, drawn).map((item, index) => <QueueRow key={item.id} item={item} index={index} busy={s.busy} status={NEXT_STATUS[next.state] ?? "Up next"} onDrop={drop} />)}
          {drawn >= order.length && order.length < QUEUE_CAP && next.state !== "finding" && <li ref={more.sentinel} className="flex h-14 items-center justify-center text-[12px] text-secondary-text">
            <span role="status" className="flex items-center gap-2"><Loader2 size={14} className="lx-spin" aria-hidden="true" />Loading more…</span>
          </li>}
        </Reorder.Group> : next.state !== "finding" && <p role="status" className="px-2 py-2 text-[13px] text-secondary-text">{NEXT_STATUS[next.state] ?? "Nothing after this song yet."}</p>}

      </section>
    </section>
  );
}
