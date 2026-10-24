"use client";
import { GalleryHorizontal, List, Play } from "lucide-react";
import { useSyncExternalStore, type MouseEvent } from "react";
import { ArtistLinks } from "../../components/LibraryActions";
import { TrackCover } from "../../components/TrackCover";
import { IconButton, cx } from "../../components/ui";
import { playTrack, usePlayback } from "../../lib/playback";
import type { SearchHitDto } from "../../services/tauri/search";
import { ResultRow } from "../search/SourceGroup";

// A shelf of songs shown either as a swipeable row of cards or as a long list that fills the width with columns.
// The choice is remembered and shared by every shelf on the screen.
export type ShelfMode = "carousel" | "list";
const KEY = "lunorx.shelfMode";
const listeners = new Set<() => void>();
let mode: ShelfMode = "carousel";
try { if (typeof window !== "undefined" && window.localStorage.getItem(KEY) === "list") mode = "list"; } catch { /* storage blocked: carousel */ }
const subscribe = (l: () => void) => { listeners.add(l); return () => { listeners.delete(l); }; };
export const useShelfMode = () => useSyncExternalStore(subscribe, () => mode, () => "carousel" as ShelfMode);
function setMode(next: ShelfMode) {
  mode = next;
  try { window.localStorage.setItem(KEY, next); } catch { /* lasts for this visit */ }
  listeners.forEach((l) => l());
}

export function ShelfModeToggle() {
  const current = useShelfMode();
  return (
    <div role="group" aria-label="Layout" className="flex items-center gap-0.5">
      <IconButton label="Carousel" active={current === "carousel"} aria-pressed={current === "carousel"} onClick={() => setMode("carousel")} className="size-9"><GalleryHorizontal size={18} /></IconButton>
      <IconButton label="List" active={current === "list"} aria-pressed={current === "list"} onClick={() => setMode("list")} className="size-9"><List size={18} /></IconButton>
    </div>
  );
}

/** A tap anywhere on the card plays; the artist links inside it still open the artist. */
const fromControl = (e: MouseEvent) => !!(e.target as HTMLElement).closest("a,button,input");

function Card({ hit, following }: { hit: SearchHitDto; following: SearchHitDto[] }) {
  const busy = usePlayback((s) => s.busy);
  const youtube = hit.provider === "YouTube";
  const play = () => { if (youtube && !busy) void playTrack(hit, false, following.filter((t) => t.provider === "YouTube")); };
  return (
    <li onClick={(e) => { if (!fromControl(e)) play(); }}
      className={cx("lx-reveal-host w-[148px] shrink-0 snap-start rounded-xl p-1.5 transition-colors duration-150 hover:bg-frost-200 sm:w-[168px]", youtube && "cursor-pointer")}>
      <div className="relative">
        <TrackCover url={hit.artworkUrl} videoId={youtube ? hit.providerTrackId : ""} className="aspect-square w-full rounded-lg" />
        {youtube && (
          <button type="button" disabled={busy} onClick={play} aria-label={`Play ${hit.title}`}
            className="lx-reveal absolute inset-0 flex items-center justify-center rounded-lg bg-black/45 text-white disabled:opacity-60">
            <Play size={26} fill="currentColor" className="translate-x-[1px]" aria-hidden="true" />
          </button>
        )}
      </div>
      <p className="mt-2 line-clamp-2 text-[13px] leading-[18px] font-semibold" title={hit.title}>{hit.title}</p>
      <p className="truncate text-[12px] leading-4 text-secondary-text"><ArtistLinks track={hit} /></p>
    </li>
  );
}

export default function TrackShelf({ tracks, label }: { tracks: SearchHitDto[]; label: string }) {
  const current = useShelfMode();
  if (current === "list") {
    return (
      <ul aria-label={label} className="grid grid-cols-1 gap-x-4 min-[1280px]:grid-cols-2 min-[1800px]:grid-cols-3">
        {tracks.map((hit, i) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={tracks.slice(i + 1)} />)}
      </ul>
    );
  }
  return (
    <ul aria-label={label} data-no-swipe className="-mx-4 flex snap-x snap-mandatory gap-2 overflow-x-auto px-4 pb-2 [scrollbar-width:thin] md:mx-0 md:px-0">
      {tracks.map((hit, i) => <Card key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={tracks.slice(i + 1)} />)}
    </ul>
  );
}
