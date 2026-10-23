import { ArtistLinks, LikeButton } from "../../components/LibraryActions";
import { CircleSlash, Play, TriangleAlert } from "lucide-react";
import type { SearchHitDto, SourceGroupDto } from "../../services/tauri/search";
import { AddToPlaylist } from "./AddToPlaylist";
import { OpenOnYouTube } from "./OpenOnYouTube";
import { TrackCover } from "../../components/TrackCover";
import { ProviderMark } from "../../components/ProviderMark";
import { Button, StatusNote, cx } from "../../components/ui";
import { formatDuration } from "../../lib/media";
import { useUi } from "../../lib/store";
import { usePlayback, playTrack } from "../../lib/playback";
import { menuHandlers } from "../../components/ContextMenu";
import { trackMenu } from "../../lib/menus";

// Search source labels distinguish YouTube Music from ordinary YouTube.
export const providerName = (p: string) => p;

const playableAfter = (list?: SearchHitDto[]) => (list ?? []).filter((t) => t.provider === "YouTube");

// Rows support search keyboard navigation; action buttons are siblings, never nested.
/** `following` are the tracks after this one in the list the row belongs to; playing the row queues them. */
export function ResultRow({ hit, large = false, onRemove, onMoveUp, onMoveDown, following }: { hit: SearchHitDto; large?: boolean; onRemove?: () => void; onMoveUp?: () => void; onMoveDown?: () => void; following?: SearchHitDto[] }) {
  const menu = trackMenu(hit, { onRemove, onMoveUp, onMoveDown, following });
  const busy = usePlayback((s) => s.busy);
  const youtube = hit.provider === "YouTube";
  const artists = hit.artists.length > 0 ? hit.artists.join(", ") : "Artist not provided";
  const origin = hit.source ?? providerName(hit.provider);
  const thumb = large ? "size-[72px]" : "size-10";
  return (
    <li
      data-result-row
      tabIndex={-1}
      role="group"
      aria-label={`${hit.title} by ${artists}, ${origin}`}
      {...menuHandlers(menu.build, menu.load)}
      className={cx(
        "lx-reveal-host flex items-center gap-3 rounded-[8px] px-2 transition-colors duration-150 hover:bg-frost-200 focus-within:bg-frost-200",
        large ? "h-24" : "h-14",
      )}
    >
      <div className={cx("relative shrink-0", thumb)}>
        <TrackCover url={hit.artworkUrl} videoId={youtube ? hit.providerTrackId : ""} className={cx(thumb, large ? "rounded-[8px]" : "rounded-[6px]")} />
        {youtube && (
          <button
            data-play-track
            type="button"
            disabled={busy}
            onClick={() => void playTrack(hit, false, playableAfter(following))}
            aria-label={`Play ${hit.title} on YouTube`}
            title="Play on YouTube"
            className={cx(
              "lx-reveal absolute inset-0 flex items-center justify-center bg-black/55 text-white hover:bg-black/70 disabled:opacity-60",
              large ? "rounded-[8px]" : "rounded-[6px]",
            )}
          >
            <Play size={large ? 20 : 14} fill="currentColor" className="translate-x-[0.5px]" aria-hidden="true" />
          </button>
        )}
      </div>
      <div className="min-w-0 flex-1">
        <p className={cx("truncate font-semibold", large ? "font-display text-[22px] leading-7 tracking-[-0.015em]" : "text-[13px] leading-[18px]")} title={hit.title}>
          {hit.title} <span className="ml-2 inline-flex items-center gap-1.5 rounded-full bg-frost-200 px-2 py-0.5 align-middle text-[10px] font-medium text-secondary-text"><ProviderMark source={origin} />{origin}</span>
        </p>
        <p className="truncate text-[13px] leading-[18px] text-secondary-text" title={`${artists}, ${origin}`}>
          <ArtistLinks track={hit} />
        </p>
      </div>
      {hit.durationMs !== null && (
        <p className="hidden w-12 shrink-0 text-right text-[12px] tabular-nums text-secondary-text md:block">{formatDuration(hit.durationMs / 1000)}</p>
      )}
      <div className="flex shrink-0 items-center gap-0.5">
        {hit.provider === "Spotify" && (
          <span className="mr-1 hidden text-[12px] text-secondary-text lg:inline" title="Spotify playback is unsupported">
            Playback unsupported
          </span>
        )}
        <LikeButton track={hit} />
        <AddToPlaylist hit={hit} />
        {youtube && <OpenOnYouTube hit={hit} />}
      </div>
    </li>
  );
}

export function SourceGroup({
  group,
  onRetry,
}: {
  group: SourceGroupDto;
  onRetry: () => void;
}) {
  const name = providerName(group.provider);
  if (group.kind === "ok") return null;

  if (group.kind === "failed") {
    // Fixed copy per category, each with the recovery. Provider text never reaches here.
    const fixed =
      group.category === "quotaExceeded"
        ? { text: `${name} has used up its search quota for today. Try again tomorrow.`, retry: false }
        : group.category === "authenticationRequired"
          ? { text: `${name} rejected the API key. Check or replace it in Settings.`, retry: false }
          : { text: `${name} is unavailable right now.`, retry: true };
    return (
      <div>
        <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
          <p className="text-secondary-text">{fixed.text}</p>
          {fixed.retry ? (
            <Button variant="ghost" onClick={onRetry} className="mt-1 h-8 px-2">
              Retry
            </Button>
          ) : (
            group.category === "authenticationRequired" && (
              <Button variant="ghost" onClick={() => useUi.getState().navigate("settings")} className="mt-1 h-8 px-2">
                Open Settings
              </Button>
            )
          )}
        </StatusNote>
      </div>
    );
  }

  const text =
    group.reason === "cancelled"
      ? "Search cancelled."
      : group.reason === "quotaExhausted"
        ? `${name} has reached its search limit for today.`
        : `${name} search is not supported yet.`;
  return (
    <div>
      <StatusNote icon={<CircleSlash size={16} />}>{text}</StatusNote>
    </div>
  );
}
