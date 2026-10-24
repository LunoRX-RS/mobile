"use client";
import { Play, RefreshCw } from "lucide-react";
import { useFeed, loadFeed } from "../../lib/discovery";
import { usePlayback, playTrack } from "../../lib/playback";
import { Button, IconButton, Section, Skeleton, StatusNote } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import { LikeButton, ArtistLinks } from "../../components/LibraryActions";
import TrackShelf, { ShelfModeToggle } from "./TrackShelf";
import { useUi } from "../../lib/store";
export default function LiveFeed({ kind, topic = "charts", limit = 10, featured = false }: { kind: "home" | "explore"; topic?: string; limit?: number; featured?: boolean }) {
  const feed = useFeed(kind, topic);
  const busy = usePlayback(s => s.busy);
  const top = feed.sections[0]?.tracks[0];
  return <div className="flex flex-col gap-8">
    {featured && top && <section aria-label="Featured song" className="glass-card flex items-center gap-4 rounded-2xl p-4 sm:items-end sm:gap-6 sm:p-6">
      <TrackCover url={top.artworkUrl} videoId={top.providerTrackId} title={top.title} className="size-28 shrink-0 rounded-xl sm:size-56" />
      <div className="min-w-0 flex-1"><p className="mb-1 text-[12px] text-secondary-text sm:mb-2 sm:text-[13px]">Listen now</p><h2 className="line-clamp-2 font-display text-[21px] font-bold leading-tight sm:text-[34px]">{top.title}</h2><p className="mt-1 truncate text-[13px] text-secondary-text sm:mt-2 sm:text-base"><ArtistLinks track={top} /></p><div className="mt-3 flex items-center gap-3 sm:mt-5"><Button variant="primary" disabled={busy} onClick={() => void playTrack(top)} className="h-10 rounded-xl px-5 sm:h-11 sm:px-6"><Play size={16} fill="currentColor" />Play</Button><LikeButton track={top} /></div></div>
    </section>}
    {feed.loading && !feed.sections.length && <div role="status" aria-label="Loading real music" className="flex flex-col gap-2">{Array.from({length:5},(_,i)=><Skeleton key={i} className="h-14" />)}</div>}
    {feed.error && <StatusNote role="alert">{feed.error}<Button onClick={() => void loadFeed(kind, topic, true)}>Retry</Button></StatusNote>}
    {feed.sections.map((section, i) => <Section key={`${topic}:${section.title}`} id={`feed-${kind}-${i}`} title={section.title} action={i===0 ? <div className="flex items-center gap-1"><ShelfModeToggle /><IconButton label="Refresh" disabled={feed.loading} onClick={() => void loadFeed(kind, topic, true)} className="size-9"><RefreshCw size={16} /></IconButton></div> : undefined}>{section.albums && section.albums.length > 0 && <ul aria-label={section.title} className="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-6">{section.albums.map(a => <li key={a.id}><button type="button" aria-label={`Open album ${a.name}`} onClick={() => useUi.getState().openAlbum({ provider: "YouTube", id: a.id, name: a.name, artworkUrl: a.artworkUrl, year: a.year })} className="group block w-full text-left">{a.artworkUrl ? <img src={a.artworkUrl} alt="" loading="lazy" referrerPolicy="no-referrer" className="mb-2 aspect-square w-full rounded-lg object-cover transition-transform duration-200 group-hover:scale-[1.02]" /> : <div className="mb-2 aspect-square w-full rounded-lg bg-frost-200" />}<p className="truncate text-[13px] font-semibold text-primary-text">{a.name}</p><p className="truncate text-[12px] text-secondary-text">{[a.artists.join(", "), a.year].filter(Boolean).join(" · ")}</p></button></li>)}</ul>}<TrackShelf tracks={section.tracks.slice(0, limit)} label={section.title} /></Section>)}
  </div>;
}
