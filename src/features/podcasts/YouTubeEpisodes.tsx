"use client";

import { useEffect, useRef, useState } from "react";
import { Loader2, TriangleAlert } from "lucide-react";
import { podcastBrowse } from "../../services/tauri/podcasts";
import type { SearchHitDto } from "../../services/tauri/search";
import { Button, Skeleton, StatusNote } from "../../components/ui";
import { ResultRow } from "../search/SourceGroup";

// A page can hold only short clips; look further this many pages before saying there is nothing.
const EMPTY_PAGES_TOLERATED = 3;

type Listing = { hits: SearchHitDto[]; next: string | null; loading: boolean; more: boolean; failed: boolean };

/**
 * Long YouTube videos matching `query`, for a show that has no feed of its own. They play through YouTube's page, so
 * they can carry its ads and stop in the background: the feed episodes are the better way to listen.
 */
export default function YouTubeEpisodes({ query }: { query: string }) {
  const [list, setList] = useState<Listing>({ hits: [], next: null, loading: true, more: false, failed: false });
  // Only the latest request may write its answer.
  const latest = useRef(0);

  async function fill(token: string | null, first: boolean) {
    const mine = ++latest.current;
    setList((l) => (first ? { hits: [], next: null, loading: true, more: false, failed: false } : { ...l, more: true, failed: false }));
    let hits: SearchHitDto[] = [];
    let next = token;
    try {
      for (let empty = 0; empty <= EMPTY_PAGES_TOLERATED; empty++) {
        const page = await podcastBrowse(query, next);
        if (latest.current !== mine) return;
        next = page.continuation;
        hits = page.hits;
        if (hits.length > 0 || next === null) break;
      }
    } catch {
      if (latest.current === mine) setList((l) => ({ ...l, loading: false, more: false, failed: true }));
      return;
    }
    setList((l) => ({
      hits: first ? hits : [...l.hits, ...hits.filter((h) => !l.hits.some((o) => o.providerTrackId === h.providerTrackId))],
      next, loading: false, more: false, failed: false,
    }));
  }

  useEffect(() => { void fill(null, true); return () => { latest.current++; }; }, [query]); // eslint-disable-line react-hooks/exhaustive-deps

  if (list.loading) {
    return (
      <div role="status" aria-label="Loading episodes from YouTube" className="flex flex-col gap-1">
        <p className="flex items-center gap-2 pb-2 text-[13px] text-secondary-text"><Loader2 size={16} className="lx-spin" aria-hidden="true" />Looking on YouTube…</p>
        {Array.from({ length: 3 }, (_, i) => <Skeleton key={i} className="h-14" />)}
      </div>
    );
  }
  if (list.failed && list.hits.length === 0) {
    return (
      <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
        <p>YouTube could not be reached.</p>
        <Button variant="ghost" onClick={() => void fill(null, true)} className="mt-1 h-8 px-2">Retry</Button>
      </StatusNote>
    );
  }
  if (list.hits.length === 0) return <p className="text-[13px] text-secondary-text">Nothing long enough on YouTube either.</p>;
  return (
    <div>
      <ul aria-label="YouTube episodes" className="flex flex-col">
        {list.hits.map((hit, i) => <ResultRow key={hit.providerTrackId} hit={hit} large following={list.hits.slice(i + 1)} />)}
      </ul>
      {list.next !== null && (
        <div className="pt-4">
          <Button variant="secondary" disabled={list.more} onClick={() => void fill(list.next, false)}>{list.more ? "Loading…" : "More from YouTube"}</Button>
        </div>
      )}
    </div>
  );
}
