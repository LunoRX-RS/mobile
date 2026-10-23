"use client";
import { Heart } from "lucide-react";
import { IconButton } from "./ui";
import { useLibrary, toggleSaved, trackKey } from "../lib/library";
import { useUi } from "../lib/store";
import type { SearchHitDto } from "../services/tauri/search";
export function LikeButton({ track }: { track: SearchHitDto }) {
  const liked = useLibrary(s => s.tracks.some(t => trackKey(t) === trackKey(track)));
  const disabled = useLibrary(s => !s.ready || s.busy);
  const error = useLibrary(s => s.error);
  return <span className="inline-flex shrink-0 items-center" title={error || undefined}><IconButton label={`${liked ? "Unlike" : "Like"} ${track.title}`} active={liked} aria-pressed={liked} disabled={disabled} onClick={() => void toggleSaved(track)}><Heart size={16} fill={liked ? "currentColor" : "none"} /></IconButton></span>;
}
export function ArtistLinks({ track }: { track: Pick<SearchHitDto, "provider" | "artists" | "artistRefs"> }) {
  return <span>{track.artists.map((name, i) => <span key={`${name}-${i}`}>{i > 0 && ", "}<button type="button" className="rounded hover:underline focus-visible:outline-2" onClick={() => useUi.getState().openArtist(track.artistRefs?.find(a => a.name === name && a.provider === track.provider) ?? { provider: track.provider, id: null, name })}>{name}</button></span>)}</span>;
}
