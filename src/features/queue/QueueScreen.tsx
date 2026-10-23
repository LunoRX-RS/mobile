"use client";

import { ListMusic, Pause, Play } from "lucide-react";
import { useUi } from "../../lib/store";
import { formatDuration, type QueueTrack } from "../../lib/media";
import { Art } from "../../components/Art";
import { Equaliser, thumbSeed } from "../../components/Tracks";
import { Button, EmptyState, cx } from "../../components/ui";

// The queue here is the demo queue kept in the UI store. The Rust core queue is not reported by any command yet,
// so this list never shows real playback state. The store has no remove or clear action, so there are no such buttons.

function QueueRow({ t, index, current }: { t: QueueTrack; index: number; current: boolean }) {
  const playing = useUi((s) => s.playing);
  const playIndex = useUi((s) => s.playIndex);
  const togglePlay = useUi((s) => s.togglePlay);
  const live = current && playing;
  const label = `${live ? "Pause" : "Play"} ${t.title} by ${t.artist}, demo, no audio`;
  return (
    <li
      className={cx(
        "lx-reveal-host grid h-14 grid-cols-[40px_minmax(0,1fr)_auto] items-center gap-3 rounded-[8px] px-2 transition-colors duration-150",
        current ? "bg-lumen-wash" : "hover:bg-frost-200 focus-within:bg-frost-200",
      )}
    >
      <div className="relative size-10">
        <Art seed={thumbSeed(t)} tone={t.tone} grain={false} rounded="rounded-[6px]" className="size-10" />
        <button
          type="button"
          aria-label={label}
          aria-current={current ? "true" : undefined}
          onClick={() => (current ? togglePlay() : playIndex(index))}
          className={cx(
            "absolute inset-0 flex items-center justify-center rounded-[6px] bg-black/55 text-white hover:bg-black/70",
            current ? "text-lumen-lavender" : "lx-reveal",
          )}
        >
          {live ? <Equaliser /> : current ? <Pause size={14} fill="currentColor" aria-hidden="true" /> : <Play size={14} fill="currentColor" className="translate-x-[0.5px]" aria-hidden="true" />}
        </button>
      </div>
      <div className="min-w-0">
        <p className="truncate text-[13px] leading-[18px] font-semibold" title={t.title}>
          {t.title}
        </p>
        <p className="truncate text-[13px] leading-[18px] text-secondary-text" title={t.artist}>
          {t.artist}
        </p>
      </div>
      <p className="text-right text-[12px] tabular-nums text-secondary-text">{formatDuration(t.durationSec)}</p>
    </li>
  );
}

export default function QueueScreen() {
  const queue = useUi((s) => s.queue);
  const index = useUi((s) => s.index);
  const navigate = useUi((s) => s.navigate);
  const toggleQueue = useUi((s) => s.toggleQueue);
  const now = queue[index];
  const next = queue.slice(index + 1);

  return (
    <section aria-labelledby="queue-title" className="flex flex-col gap-4">
      <header className="flex flex-col gap-0.5">
        <h2 id="queue-title" className="font-display text-[22px] leading-7 font-semibold tracking-[-0.015em]">
          Queue
        </h2>
        <p className="text-[13px] leading-[18px] text-secondary-text">Demo queue. Nothing here produces audio.</p>
      </header>

      {!now ? (
        <EmptyState
          icon={<ListMusic size={24} />}
          title="Nothing in the queue"
          action={
            <Button
              variant="secondary"
              className="mt-1"
              onClick={() => {
                toggleQueue();
                navigate("search");
              }}
            >
              Find songs
            </Button>
          }
        >
          Songs you play are listed here.
        </EmptyState>
      ) : (
        <>
          <section aria-labelledby="queue-now" className="flex flex-col gap-1">
            <h3 id="queue-now" className="px-2 text-[13px] leading-[18px] font-semibold text-secondary-text">
              Now playing
            </h3>
            <ul className="flex flex-col">
              <QueueRow t={now} index={index} current />
            </ul>
          </section>
          <section aria-labelledby="queue-next" className="flex flex-col gap-1">
            <h3 id="queue-next" className="px-2 text-[13px] leading-[18px] font-semibold text-secondary-text">
              Up next
            </h3>
            {next.length === 0 ? (
              <p className="px-2 py-2 text-[13px] leading-[18px] text-secondary-text">Nothing after this song.</p>
            ) : (
              <ol className="flex flex-col" aria-label="Up next">
                {next.map((t, i) => (
                  <QueueRow key={t.id} t={t} index={index + 1 + i} current={false} />
                ))}
              </ol>
            )}
          </section>
        </>
      )}
    </section>
  );
}
