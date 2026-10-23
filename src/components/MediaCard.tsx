import type { ReactNode } from "react";
import { Play } from "lucide-react";
import { Art, SCRIM_BG } from "./Art";
import { cx } from "./ui";
import type { Tone } from "../lib/media";

// Square artwork tile, Apple Music style: art on top, title and subtitle under it, no container.
// The small play button shows on hover and on keyboard focus.
export function MediaCard({
  seed,
  tone,
  title,
  subtitle,
  onPlay,
  playLabel,
  onOpen,
  openLabel,
  round = false,
  className,
}: {
  seed: string;
  tone: Tone;
  title: string;
  subtitle: string;
  onPlay?: () => void;
  playLabel?: string;
  // When set, the cover and the title are two separate buttons that open a details view (never nested).
  onOpen?: () => void;
  openLabel?: string;
  round?: boolean;
  className?: string;
}) {
  return (
    <div className={cx("lx-reveal-host flex min-w-0 flex-col gap-2", className)}>
      <div className="relative w-full">
        {onOpen ? (
          <button type="button" onClick={onOpen} aria-label={openLabel ?? `Open ${title}`} className="block w-full rounded-[8px]">
            <Art seed={seed} tone={tone} className="aspect-square w-full shadow-[0_1px_4px_rgba(0,0,0,0.45)]" rounded="rounded-[8px]" />
          </button>
        ) : (
          <Art
            seed={seed}
            tone={tone}
            className="aspect-square w-full shadow-[0_1px_4px_rgba(0,0,0,0.45)]"
            rounded={round ? "rounded-full" : "rounded-[8px]"}
          />
        )}
        {onPlay && (
          <button
            type="button"
            aria-label={playLabel ?? `Play ${title} (demo)`}
            onClick={onPlay}
            className="lx-reveal lx-reveal-pop absolute bottom-2 left-2 flex size-8 items-center justify-center rounded-full bg-black/60 text-white hover:bg-black/75"
          >
            <Play size={14} fill="currentColor" className="translate-x-[0.5px]" aria-hidden="true" />
          </button>
        )}
      </div>
      <div className={cx("min-w-0", round && "text-center")}>
        {onOpen ? (
          <button type="button" onClick={onOpen} title={title} className="block min-h-6 w-full truncate text-left text-[13px] leading-[18px] font-semibold hover:underline">
            {title}
          </button>
        ) : (
          <p className="truncate text-[13px] leading-[18px] font-semibold" title={title}>
            {title}
          </p>
        )}
        <p className="truncate text-[13px] leading-[18px] text-secondary-text" title={subtitle}>
          {subtitle}
        </p>
      </div>
    </div>
  );
}

// Large "Top Picks" tile: full-bleed artwork, scrim from the art's tone, title and subtitle over the bottom edge.
export function FeatureTile({
  seed,
  tone,
  title,
  subtitle,
  onPlay,
  playLabel,
}: {
  seed: string;
  tone: Tone;
  title: string;
  subtitle: string;
  onPlay: () => void;
  playLabel: string;
}) {
  return (
    <button
      type="button"
      onClick={onPlay}
      aria-label={playLabel}
      className="lx-reveal-host group relative block aspect-[5/4] w-full min-w-0 overflow-hidden rounded-[16px] text-left shadow-[0_2px_10px_rgba(0,0,0,0.4)]"
    >
      <Art seed={seed} tone={tone} rounded="rounded-none" className="absolute inset-0 size-full transition-transform duration-500 ease-[cubic-bezier(0.16,1,0.3,1)] group-hover:scale-[1.03]" />
      <span aria-hidden="true" className={cx("absolute inset-0", SCRIM_BG[tone])} />
      <span className="absolute inset-x-4 bottom-4 flex min-w-0 flex-col">
        <span className="truncate font-display text-[20px] leading-6 font-bold tracking-[-0.015em] text-white">{title}</span>
        <span className="truncate text-[13px] leading-[18px] text-white/75">{subtitle}</span>
      </span>
      <span
        aria-hidden="true"
        className="lx-reveal absolute right-4 bottom-4 flex size-9 items-center justify-center rounded-full bg-white/90 text-black"
      >
        <Play size={15} fill="currentColor" className="translate-x-[0.5px]" />
      </span>
    </button>
  );
}

// Horizontal rail of tiles that scrolls sideways.
export function Rail({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div role="list" aria-label={label} className="lx-rail lx-stagger -mx-8 flex snap-x scroll-px-8 gap-5 overflow-x-auto px-8 pb-1">
      {children}
    </div>
  );
}

export function RailItem({ children }: { children: ReactNode }) {
  return (
    <div role="listitem" className="w-[168px] shrink-0 snap-start 2xl:w-[188px]">
      {children}
    </div>
  );
}

export const GRID_CLASS = "lx-stagger grid grid-cols-[repeat(auto-fill,minmax(160px,1fr))] gap-x-5 gap-y-6";
