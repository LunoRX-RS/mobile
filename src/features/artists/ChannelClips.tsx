"use client";

import { useEffect, useRef, useState } from "react";
import { Section, Button } from "../../components/ui";
import { podcastChannel } from "../../services/tauri/podcasts";
import type { SearchHitDto } from "../../services/tauri/search";
import { ResultRow } from "../search/SourceGroup";

const PAGES_TOLERATED = 3;

/**
 * What a channel has to listen to besides songs: its uploads of ten minutes or more (episodes, stories, talks). A
 * podcast or a storyteller is a channel too, and its artist page would otherwise be empty. Shown only when there is
 * something; a failure is quiet, because the page's songs are still there.
 */
export default function ChannelClips({ channelId, name }: { channelId: string; name: string }) {
  const [hits, setHits] = useState<SearchHitDto[]>([]);
  const [next, setNext] = useState<string | null>(null);
  const [more, setMore] = useState(false);
  const [loaded, setLoaded] = useState(false);
  const latest = useRef(0);

  async function load(token: string | null, first: boolean) {
    const mine = ++latest.current;
    setMore(true);
    let page: HitsPage = { hits: [], continuation: token };
    try {
      for (let empty = 0; empty <= PAGES_TOLERATED; empty++) {
        page = await podcastChannel(channelId, name, page.continuation);
        if (latest.current !== mine) return;
        if (page.hits.length > 0 || page.continuation === null) break;
      }
    } catch {
      if (latest.current === mine) { setMore(false); setLoaded(true); }
      return;
    }
    setHits((old) => (first ? page.hits : [...old, ...page.hits.filter((h) => !old.some((o) => o.providerTrackId === h.providerTrackId))]));
    setNext(page.continuation);
    setMore(false);
    setLoaded(true);
  }

  useEffect(() => {
    setHits([]); setNext(null); setLoaded(false);
    void load(null, true);
    return () => { latest.current++; };
  }, [channelId]); // eslint-disable-line react-hooks/exhaustive-deps

  if (loaded && hits.length === 0) return null;
  return (
    <Section id="artist-clips" title="Episodes and talks">
      {!loaded && <p role="status" className="text-[13px] text-secondary-text">Looking for episodes…</p>}
      {hits.length > 0 && (
        <ul aria-label="Episodes and talks">
          {hits.slice(0, 60).map((hit, i) => <ResultRow key={hit.providerTrackId} hit={hit} large following={hits.slice(i + 1, i + 51)} />)}
        </ul>
      )}
      {next !== null && hits.length > 0 && (
        <div className="pt-4"><Button variant="secondary" disabled={more} onClick={() => void load(next, false)}>{more ? "Loading…" : "More"}</Button></div>
      )}
    </Section>
  );
}

type HitsPage = { hits: SearchHitDto[]; continuation: string | null };
