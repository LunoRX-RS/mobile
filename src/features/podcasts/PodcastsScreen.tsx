"use client";

import { useEffect, useRef, useState } from "react";
import { Loader2, Mic, Plus, Search as SearchIcon, TriangleAlert, X } from "lucide-react";
import { podcastBrowse } from "../../services/tauri/podcasts";
import type { SearchHitDto } from "../../services/tauri/search";
import { Button, EmptyState, Screen, Section, Skeleton, StatusNote } from "../../components/ui";
import { ResultRow } from "../search/SourceGroup";

const KEY = "lunorx.podcastShows";
const MAX_SHOWS = 24;
// A page can hold only short clips; look further this many pages before saying there is nothing.
const EMPTY_PAGES_TOLERATED = 3;

// Starting points only. Each one is a plain search, so a show that is not listed is found by typing its name.
const SUGGESTIONS = ["The Ghost Radio", "9arm", "Mission To The Moon", "True crime", "Business", "Comedy", "Technology", "News", "Health"];

function readShows(): string[] {
  try {
    const v: unknown = JSON.parse(window.localStorage.getItem(KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((s): s is string => typeof s === "string").slice(0, MAX_SHOWS) : [];
  } catch {
    return [];
  }
}

function writeShows(shows: string[]) {
  try { window.localStorage.setItem(KEY, JSON.stringify(shows)); } catch { /* kept for this visit only */ }
}

type Listing = {
  text: string;
  hits: SearchHitDto[];
  next: string | null;
  loading: boolean;
  more: boolean;
  message: string;
};

const IDLE: Listing = { text: "", hits: [], next: null, loading: false, more: false, message: "" };

export default function PodcastsScreen() {
  const [input, setInput] = useState("");
  const [shows, setShows] = useState<string[]>([]);
  const [list, setList] = useState<Listing>(IDLE);
  // Only the latest request may write its answer.
  const latest = useRef(0);

  useEffect(() => setShows(readShows()), []);

  async function fill(text: string, token: string | null, first: boolean) {
    const mine = ++latest.current;
    setList((l) => (first ? { ...IDLE, text, loading: true } : { ...l, more: true, message: "" }));
    let hits: SearchHitDto[] = [];
    let next = token;
    try {
      for (let empty = 0; empty <= EMPTY_PAGES_TOLERATED; empty++) {
        const page = await podcastBrowse(text, next);
        if (latest.current !== mine) return;
        next = page.continuation;
        hits = page.hits;
        if (hits.length > 0 || next === null) break;
      }
    } catch {
      if (latest.current !== mine) return;
      setList((l) => ({ ...l, loading: false, more: false, message: "Podcasts could not load. Check the connection and try again." }));
      return;
    }
    setList((l) => ({
      text,
      hits: first ? hits : [...l.hits, ...hits.filter((h) => !l.hits.some((o) => o.providerTrackId === h.providerTrackId))],
      next,
      loading: false,
      more: false,
      message: "",
    }));
  }

  function open(raw: string) {
    const text = raw.trim();
    if (text === "") return;
    setInput(text);
    void fill(text, null, true);
  }

  function follow(text: string) {
    const next = [text, ...shows.filter((s) => s.toLowerCase() !== text.toLowerCase())].slice(0, MAX_SHOWS);
    setShows(next);
    writeShows(next);
  }

  function unfollow(text: string) {
    const next = shows.filter((s) => s !== text);
    setShows(next);
    writeShows(next);
  }

  const followed = shows.some((s) => s.toLowerCase() === list.text.toLowerCase());
  const showing = list.text !== "";
  const chip = (text: string, remove?: () => void) => (
    <li key={text} className="flex items-center">
      <Button variant="secondary" onClick={() => open(text)} className={remove ? "rounded-r-none" : undefined}>{text}</Button>
      {remove && (
        <Button variant="secondary" aria-label={`Unfollow ${text}`} onClick={remove} className="rounded-l-none px-2">
          <X size={14} aria-hidden="true" />
        </Button>
      )}
    </li>
  );

  return (
    <Screen id="podcasts-title" title="Podcasts" caption="Find a show or a topic. Episodes are long YouTube videos, 20 minutes or more, and play like any song.">
      <div className="flex flex-col gap-8">
        <form role="search" className="flex items-center gap-2" onSubmit={(e) => { e.preventDefault(); open(input); }}>
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
          <Button variant="primary" type="submit" disabled={input.trim() === "" || list.loading} className="h-12 rounded-full px-6">Search</Button>
        </form>

        {shows.length > 0 && (
          <Section id="podcast-shows" title="Your shows">
            <ul aria-label="Followed shows" className="flex flex-wrap gap-2">{shows.map((s) => chip(s, () => unfollow(s)))}</ul>
          </Section>
        )}

        {!showing && (
          <Section id="podcast-suggestions" title="Try">
            <ul aria-label="Suggested searches" className="flex flex-wrap gap-2">{SUGGESTIONS.map((s) => chip(s))}</ul>
          </Section>
        )}

        <p aria-live="polite" className="sr-only">{list.loading ? "Loading episodes" : list.message}</p>

        {list.loading && (
          <div role="status" aria-label="Loading episodes" className="flex flex-col gap-1">
            <p className="flex items-center gap-2 pb-2 text-[13px] text-secondary-text">
              <Loader2 size={16} className="lx-spin" aria-hidden="true" />
              Looking for “{list.text}” episodes…
            </p>
            {Array.from({ length: 5 }, (_, i) => <Skeleton key={i} className="h-14" />)}
          </div>
        )}

        {list.message && !list.loading && (
          <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
            <p>{list.message}</p>
            <Button variant="ghost" onClick={() => void fill(list.text, list.hits.length > 0 ? list.next : null, list.hits.length === 0)} className="mt-1 h-8 px-2">Retry</Button>
          </StatusNote>
        )}

        {showing && !list.loading && list.hits.length > 0 && (
          <Section
            id="podcast-episodes"
            title={list.text}
            action={!followed ? <Button variant="ghost" onClick={() => follow(list.text)}><Plus size={14} aria-hidden="true" />Follow</Button> : undefined}
          >
            <ul aria-label="Episodes" className="lx-stagger flex flex-col">
              {list.hits.map((hit, i) => <ResultRow key={hit.providerTrackId} hit={hit} large following={list.hits.slice(i + 1)} />)}
            </ul>
            {list.next !== null && (
              <div className="pt-4">
                <Button variant="secondary" disabled={list.more} onClick={() => void fill(list.text, list.next, false)}>
                  {list.more ? "Loading…" : "More episodes"}
                </Button>
              </div>
            )}
          </Section>
        )}

        {showing && !list.loading && !list.message && list.hits.length === 0 && (
          <EmptyState icon={<Mic size={24} />} title={`No episodes found for “${list.text}”`}>
            Try the name of the show as it is written on YouTube, or a broader topic.
          </EmptyState>
        )}
      </div>
    </Screen>
  );
}
