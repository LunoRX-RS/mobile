"use client";
import { Heart } from "lucide-react";
import { IconButton } from "./ui";
import { useLibrary, toggleSaved, trackKey } from "../lib/library";
import { useUi } from "../lib/store";
import type { SearchHitDto } from "../services/tauri/search";
// Likes are for songs; an episode has no place in Liked Songs. The check is outside the hooks on purpose: the same
// button can be handed a song and then an episode.
export function LikeButton({ track }: { track: SearchHitDto }) {
  return track.provider === "Podcast" ? null : <SongLikeButton track={track} />;
}
function SongLikeButton({ track }: { track: SearchHitDto }) {
  const liked = useLibrary(s => s.tracks.some(t => trackKey(t) === trackKey(track)));
  const disabled = useLibrary(s => !s.ready || s.busy);
  const error = useLibrary(s => s.error);
  return <span className="inline-flex shrink-0 items-center" title={error || undefined}><IconButton label={`${liked ? "Unlike" : "Like"} ${track.title}`} active={liked} aria-pressed={liked} disabled={disabled} onClick={() => void toggleSaved(track)}><Heart size={16} fill={liked ? "currentColor" : "none"} /></IconButton></span>;
}
export function ArtistLinks({ track }: { track: Pick<SearchHitDto, "provider" | "artists" | "artistRefs"> }) {
  // A show is not an artist page.
  if (track.provider === "Podcast") return <span>{track.artists.join(", ")}</span>;
  return <span>{track.artists.map((name, i) => <span key={`${name}-${i}`}>{i > 0 && ", "}<button type="button" className="ln-inline rounded hover:underline focus-visible:outline-2" onClick={() => useUi.getState().openArtist(track.artistRefs?.find(a => a.name === name && a.provider === track.provider) ?? { provider: track.provider, id: null, name })}>{name}</button></span>)}</span>;
}
