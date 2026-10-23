"use client";
import { Play, RefreshCw } from "lucide-react";
import { useFeed, loadFeed } from "../../lib/discovery";
import { usePlayback, playTrack } from "../../lib/playback";
import { Button, Section, Skeleton, StatusNote } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import { LikeButton, ArtistLinks } from "../../components/LibraryActions";
import { ResultRow } from "../search/SourceGroup";
import { useUi } from "../../lib/store";
export default function LiveFeed({ kind, topic = "charts", limit = 10, featured = false }: { kind: "home" | "explore"; topic?: string; limit?: number; featured?: boolean }) {
  const feed = useFeed(kind, topic);
  const busy = usePlayback(s => s.busy);
  const top = feed.sections[0]?.tracks[0];
  return <div className="flex flex-col gap-8">
    {featured && top && <section aria-label="Featured song" className="glass-card flex flex-col gap-6 rounded-2xl p-6 sm:flex-row sm:items-end">
      <TrackCover url={top.artworkUrl} videoId={top.providerTrackId} title={top.title} className="size-56 shrink-0 rounded-xl" />
      <div className="min-w-0 flex-1"><p className="mb-2 text-[13px] text-secondary-text">YouTube Music · Listen now</p><h2 className="font-display text-[34px] font-bold leading-tight">{top.title}</h2><p className="mt-2 text-secondary-text"><ArtistLinks track={top} /></p><div className="mt-5 flex items-center gap-3"><Button variant="primary" disabled={busy} onClick={() => void playTrack(top)} className="h-11 rounded-xl px-6"><Play size={16} fill="currentColor" />Play</Button><LikeButton track={top} /></div></div>
    </section>}
    {feed.loading && !feed.sections.length && <div role="status" aria-label="Loading real music" className="flex flex-col gap-2">{Array.from({length:5},(_,i)=><Skeleton key={i} className="h-14" />)}</div>}
    {feed.error && <StatusNote role="alert">{feed.error}<Button onClick={() => void loadFeed(kind, topic, true)}>Retry</Button></StatusNote>}
    {feed.sections.map((section, i) => <Section key={`${topic}:${section.title}`} id={`feed-${kind}-${i}`} title={section.title} action={i===0 ? <Button variant="ghost" disabled={feed.loading} onClick={() => void loadFeed(kind, topic, true)}><RefreshCw size={14} />Refresh</Button> : undefined}>{section.albums && section.albums.length > 0 && <ul aria-label={section.title} className="grid grid-cols-2 gap-4 md:grid-cols-3 xl:grid-cols-6">{section.albums.map(a => <li key={a.id}><button type="button" aria-label={`Open album ${a.name}`} onClick={() => useUi.getState().openAlbum({ provider: "YouTube", id: a.id, name: a.name, artworkUrl: a.artworkUrl, year: a.year })} className="group block w-full text-left">{a.artworkUrl ? <img src={a.artworkUrl} alt="" loading="lazy" referrerPolicy="no-referrer" className="mb-2 aspect-square w-full rounded-lg object-cover transition-transform duration-200 group-hover:scale-[1.02]" /> : <div className="mb-2 aspect-square w-full rounded-lg bg-frost-200" />}<p className="truncate text-[13px] font-semibold text-primary-text">{a.name}</p><p className="truncate text-[12px] text-secondary-text">{[a.artists.join(", "), a.year].filter(Boolean).join(" · ")}</p></button></li>)}</ul>}<ul>{section.tracks.slice(0, limit).map((hit, i, shown) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={shown.slice(i + 1)} />)}</ul></Section>)}
  </div>;
}
