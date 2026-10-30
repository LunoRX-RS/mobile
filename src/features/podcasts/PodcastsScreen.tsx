"use client";

import { useEffect, useRef, useState } from "react";
import { ArrowLeft, Check, Loader2, Mic, Plus, Search as SearchIcon, TriangleAlert } from "lucide-react";
import { episodeTrack, podcastEpisodes, podcastShows, type EpisodeDto, type ShowDto } from "../../services/tauri/podcasts";
import { playTrack, usePlayback } from "../../lib/playback";
import { Button, EmptyState, Screen, Section, Skeleton, StatusNote } from "../../components/ui";
import { TrackCover } from "../../components/TrackCover";
import YouTubeEpisodes from "./YouTubeEpisodes";

const KEY = "lunorx.podcastFollowing";
const MAX_FOLLOWED = 60;
const FRESH_MS = 10 * 60 * 1000;
const PAGE = 30;

// Starting points only. Each is a plain search, so a show that is not listed is found by typing its name.
const SUGGESTIONS = ["9arm", "ลึกลับ", "True crime", "Business", "Comedy", "Technology", "News", "Health"];

function readFollowed(): ShowDto[] {
  try {
    const v: unknown = JSON.parse(window.localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v)
      ? v.filter((s): s is ShowDto => !!s && typeof (s as ShowDto).id === "string" && typeof (s as ShowDto).title === "string").slice(0, MAX_FOLLOWED)
      : [];
  } catch {
    return [];
  }
}

function writeFollowed(shows: ShowDto[]) {
  try { window.localStorage.setItem(KEY, JSON.stringify(shows)); } catch { /* kept for this visit only */ }
}

// What was asked a moment ago opens at once instead of asking again.
const shownBefore = new Map<string, { at: number; shows: ShowDto[] }>();
const feedsBefore = new Map<string, { at: number; episodes: EpisodeDto[] }>();
const fresh = (at: number) => Date.now() - at < FRESH_MS;

function when(published: string | null): string {
  const d = published ? new Date(published) : null;
  return d && !Number.isNaN(d.getTime()) ? d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" }) : "";
}

function length(ms: number | null): string {
  if (!ms) return "";
  const minutes = Math.round(ms / 60000);
  return minutes >= 60 ? `${Math.floor(minutes / 60)} h ${minutes % 60 ? `${minutes % 60} min` : ""}`.trim() : `${Math.max(1, minutes)} min`;
}

function ShowRow({ show, onOpen }: { show: ShowDto; onOpen: () => void }) {
  return (
    <li>
      <button type="button" onClick={onOpen} className="ln-touch flex min-h-[72px] w-full items-center gap-3 rounded-[10px] px-2 py-2 text-left active:bg-frost-200">
        <TrackCover url={show.artworkUrl} videoId="" title={show.title} className="size-14 rounded-[8px]" />
        <span className="min-w-0 flex-1">
          <span className="line-clamp-2 block text-[15px] font-medium text-primary-text">{show.title}</span>
          <span className="block truncate text-[13px] text-secondary-text">{show.author}</span>
        </span>
      </button>
    </li>
  );
}

function EpisodeRow({ episode, list, index }: { episode: EpisodeDto; list: EpisodeDto[]; index: number }) {
  const busy = usePlayback((s) => s.busy);
  const meta = [when(episode.published), length(episode.durationMs)].filter(Boolean).join(" · ");
  return (
    <li>
      <button
        type="button"
        disabled={busy}
        onClick={() => void playTrack(episodeTrack(episode), false, list.slice(index + 1, index + 1 + 50).map(episodeTrack))}
        className="ln-touch flex min-h-[72px] w-full items-center gap-3 rounded-[10px] px-2 py-2 text-left active:bg-frost-200 disabled:opacity-60"
      >
        <TrackCover url={episode.artworkUrl} videoId="" title={episode.title} className="size-14 rounded-[8px]" />
        <span className="min-w-0 flex-1">
          <span className="line-clamp-2 block text-[15px] text-primary-text">{episode.title}</span>
          {meta && <span className="block text-[12px] text-secondary-text">{meta}</span>}
        </span>
      </button>
    </li>
  );
}

function ShowPage({ show, followed, onFollow, onBack }: { show: ShowDto; followed: boolean; onFollow: () => void; onBack: () => void }) {
  const [episodes, setEpisodes] = useState<EpisodeDto[] | null>(() => { const c = feedsBefore.get(show.id); return c && fresh(c.at) ? c.episodes : null; });
  const [failed, setFailed] = useState(false);
  const [shown, setShown] = useState(PAGE);

  useEffect(() => {
    if (episodes !== null) return;
    let live = true;
    podcastEpisodes(show.id).then(
      (list) => { if (live) { feedsBefore.set(show.id, { at: Date.now(), episodes: list }); setEpisodes(list); } },
      () => { if (live) setFailed(true); },
    );
    return () => { live = false; };
  }, [show.id, episodes]);

  return (
    <div className="flex flex-col gap-6">
      <div className="flex items-center gap-3">
        <Button variant="ghost" onClick={onBack} aria-label="Back to podcasts" className="ln-touch -ml-2 px-2"><ArrowLeft size={20} aria-hidden="true" /></Button>
        <p className="text-[13px] text-secondary-text">Podcasts</p>
      </div>
      <div className="flex items-center gap-4">
        <TrackCover url={show.artworkUrl} videoId="" title={show.title} className="size-24 rounded-[12px]" />
        <div className="min-w-0 flex-1">
          <h2 className="line-clamp-3 text-[20px] font-semibold leading-tight text-primary-text">{show.title}</h2>
          <p className="truncate text-[14px] text-secondary-text">{show.author}</p>
          <Button variant={followed ? "secondary" : "primary"} onClick={onFollow} aria-pressed={followed} className="mt-2 h-9 px-3">
            {followed ? <Check size={14} aria-hidden="true" /> : <Plus size={14} aria-hidden="true" />}{followed ? "Following" : "Follow"}
          </Button>
        </div>
      </div>
      {episodes === null && !failed && (
        <div role="status" aria-label="Loading episodes" className="flex flex-col gap-1">
          <p className="flex items-center gap-2 pb-2 text-[13px] text-secondary-text"><Loader2 size={16} className="lx-spin" aria-hidden="true" />Loading episodes…</p>
          {Array.from({ length: 5 }, (_, i) => <Skeleton key={i} className="h-[72px]" />)}
        </div>
      )}
      {failed && (
        <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
          <p>This show&apos;s feed could not be loaded. Check the connection and try again.</p>
          <Button variant="ghost" onClick={() => { setFailed(false); setEpisodes(null); }} className="mt-1 h-8 px-2">Retry</Button>
        </StatusNote>
      )}
      {episodes !== null && episodes.length === 0 && (
        <EmptyState icon={<Mic size={24} />} title="No playable episodes">This feed lists no audio the app can play.</EmptyState>
      )}
      {episodes !== null && episodes.length > 0 && (
        <Section id="show-episodes" title={`${episodes.length} episodes`}>
          <ul aria-label="Episodes" className="flex flex-col">
            {episodes.slice(0, shown).map((e, i) => <EpisodeRow key={e.id} episode={e} list={episodes} index={i} />)}
          </ul>
          {shown < episodes.length && (
            <div className="pt-4"><Button variant="secondary" onClick={() => setShown((n) => n + PAGE)}>More episodes</Button></div>
          )}
        </Section>
      )}
    </div>
  );
}

export default function PodcastsScreen() {
  const [input, setInput] = useState("");
  const [asked, setAsked] = useState("");
  const [shows, setShows] = useState<ShowDto[] | null>(null);
  const [searching, setSearching] = useState(false);
  const [failed, setFailed] = useState(false);
  const [followed, setFollowed] = useState<ShowDto[]>([]);
  const [open, setOpen] = useState<ShowDto | null>(null);
  const [youtube, setYoutube] = useState(false);
  // Only the latest search may write its answer.
  const latest = useRef(0);

  useEffect(() => setFollowed(readFollowed()), []);

  function search(raw: string) {
    const text = raw.trim();
    if (text === "") return;
    setInput(text);
    setAsked(text);
    setYoutube(false);
    setFailed(false);
    const key = text.toLowerCase();
    const seen = shownBefore.get(key);
    const mine = ++latest.current;
    if (seen && fresh(seen.at)) { setShows(seen.shows); setSearching(false); return; }
    setShows(null);
    setSearching(true);
    podcastShows(text).then(
      (list) => { if (latest.current === mine) { shownBefore.set(key, { at: Date.now(), shows: list }); setShows(list); setSearching(false); } },
      () => { if (latest.current === mine) { setFailed(true); setSearching(false); } },
    );
  }

  const isFollowed = (show: ShowDto) => followed.some((f) => f.id === show.id);
  function toggleFollow(show: ShowDto) {
    const next = isFollowed(show) ? followed.filter((f) => f.id !== show.id) : [show, ...followed].slice(0, MAX_FOLLOWED);
    setFollowed(next);
    writeFollowed(next);
  }

  if (open) {
    return (
      <Screen id="podcasts-title" title="Podcasts" caption="">
        <ShowPage show={open} followed={isFollowed(open)} onFollow={() => toggleFollow(open)} onBack={() => setOpen(null)} />
      </Screen>
    );
  }

  return (
    <Screen id="podcasts-title" title="Podcasts" caption="Find a show, then play any episode. Episodes are read from the show's own feed, not from YouTube.">
      <div className="flex flex-col gap-8">
        <form role="search" className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); search(input); }}>
          <label htmlFor="podcast-query" className="sr-only">Search podcasts</label>
          <div className="flex h-12 min-w-0 flex-1 items-center gap-3 rounded-full bg-frost-200 px-5 transition-colors focus-within:bg-frost-300 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-primary-text">
            <SearchIcon size={18} aria-hidden="true" className="shrink-0 text-secondary-text" />
            <input
              id="podcast-query"
              type="search"
              value={input}
              onChange={(e) => setInput(e.currentTarget.value)}
              placeholder="A show or a topic"
              autoComplete="off"
              maxLength={120}
              className="h-full min-w-0 flex-1 bg-transparent text-[17px] text-primary-text outline-none placeholder:text-tertiary-text [&::-webkit-search-cancel-button]:hidden"
            />
          </div>
          <Button variant="primary" type="submit" disabled={input.trim() === "" || searching} className="h-12 rounded-full px-6">Search</Button>
        </form>

        {followed.length > 0 && asked === "" && (
          <Section id="podcast-following" title="Following">
            <ul aria-label="Followed shows" className="flex flex-col">{followed.map((s) => <ShowRow key={s.id} show={s} onOpen={() => setOpen(s)} />)}</ul>
          </Section>
        )}

        {asked === "" && (
          <Section id="podcast-suggestions" title="Try">
            <ul aria-label="Suggested searches" className="flex flex-wrap gap-2">
              {SUGGESTIONS.map((t) => <li key={t}><Button variant="secondary" onClick={() => search(t)}>{t}</Button></li>)}
            </ul>
          </Section>
        )}

        <p aria-live="polite" className="sr-only">{searching ? "Searching" : failed ? "Search failed" : ""}</p>

        {searching && (
          <div role="status" aria-label="Searching podcasts" className="flex flex-col gap-1">
            <p className="flex items-center gap-2 pb-2 text-[13px] text-secondary-text"><Loader2 size={16} className="lx-spin" aria-hidden="true" />Looking for “{asked}”…</p>
            {Array.from({ length: 5 }, (_, i) => <Skeleton key={i} className="h-[72px]" />)}
          </div>
        )}

        {failed && !searching && (
          <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
            <p>Podcasts could not be searched. Check the connection and try again.</p>
            <Button variant="ghost" onClick={() => search(asked)} className="mt-1 h-8 px-2">Retry</Button>
          </StatusNote>
        )}

        {shows !== null && !searching && shows.length > 0 && (
          <Section id="podcast-shows" title="Shows">
            <ul aria-label="Shows" className="flex flex-col">{shows.map((s) => <ShowRow key={s.id} show={s} onOpen={() => setOpen(s)} />)}</ul>
          </Section>
        )}

        {shows !== null && !searching && shows.length === 0 && (
          <EmptyState icon={<Mic size={24} />} title={`No shows found for “${asked}”`}>
            Some shows are published only on YouTube. You can look there instead.
          </EmptyState>
        )}

        {shows !== null && !searching && asked !== "" && (
          <Section id="podcast-youtube" title="Not here? Look on YouTube">
            {youtube
              ? <YouTubeEpisodes query={asked} />
              : (
                <div className="flex flex-col gap-2">
                  <p className="text-[13px] text-secondary-text">YouTube episodes play through YouTube&apos;s page: they can carry its ads and pause in the background.</p>
                  <div><Button variant="secondary" onClick={() => setYoutube(true)}>Search YouTube for “{asked}”</Button></div>
                </div>
              )}
          </Section>
        )}
      </div>
    </Screen>
  );
}
