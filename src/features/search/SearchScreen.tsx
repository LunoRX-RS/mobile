"use client";

import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { Clock, Loader2, Search as SearchIcon, TriangleAlert, X } from "lucide-react";
import {
  providersList,
  searchCancel,
  searchQuery,
  type ProviderStatusDto,
  type SearchResponseDto,
} from "../../services/tauri/search";
import { useUi } from "../../lib/store";
import { playTrack } from "../../lib/playback";
import { parseYouTubeLink } from "../../lib/ytLink";
import { playerTrackInfo } from "../../services/tauri/player";
import { LinkPlay, linkTrack } from "./LinkPlay";
import { AnimatePresence } from "motion/react";
import { CollapseLi } from "../../components/Motion";
import { Button, EmptyState, Screen, Section, Skeleton, StatusNote } from "../../components/ui";
import SearchSuggestions from "./SearchSuggestions";
import LiveFeed from "../discovery/LiveFeed";
import { ResultRow, SourceGroup, providerName } from "./SourceGroup";

const GENERIC_ERROR = "Something went wrong. Try again.";

// Plain words for what each provider can do for search right now.
const SEARCH_WORDS: Record<string, string> = {
  Supported: "ready",
  Unsupported: "search not supported",
  RequiresAuth: "needs an API key",
  RequiresEntitlement: "needs a subscription",
  TemporarilyUnavailable: "temporarily unavailable",
  Unverified: "not verified yet",
};

// One state object so async completions can check, inside the updater, that they are still the latest search.
type View = {
  id: string | null; // in-flight cancellation id
  result: SearchResponseDto | null;
  message: string;
  text: string; // the query that produced this view
};

const IDLE: View = { id: null, result: null, message: "", text: "" };

export default function SearchScreen() {
  const query = useUi((s) => s.query);
  const setQuery = useUi((s) => s.setQuery);
  const recent = useUi((s) => s.recentSearches);
  const pushRecent = useUi((s) => s.pushRecentSearch);
  const clearRecent = useUi((s) => s.clearRecentSearches);
  const pendingSearch = useUi((s) => s.pendingSearch);
  const navigate = useUi((s) => s.navigate);
  const [providers, setProviders] = useState<ProviderStatusDto[]>([]);
  const [view, setView] = useState<View>(IDLE);
  const rootRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  // The field is ready to type in as soon as the view opens. On a touch screen that would raise the keyboard over the
  // results on every visit, so there the user taps the field.
  useEffect(() => {
    if (!window.matchMedia("(pointer: coarse)").matches) inputRef.current?.focus();
  }, []);

  useEffect(() => {
    providersList()
      .then(setProviders)
      .catch(() => setView((v) => ({ ...v, message: GENERIC_ERROR })));
  }, []);

  // A search started from the command palette runs once here.
  useEffect(() => {
    if (!pendingSearch) return;
    useUi.setState({ pendingSearch: false });
    void runSearch(query);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pendingSearch]);

  async function runSearch(raw: string) {
    const text = raw.trim();
    if (text === "") return;
    // A pasted YouTube or YouTube Music link plays at once; it is not a search.
    const pasted = parseYouTubeLink(text);
    if (pasted) {
      // Title and artwork first, so the player and history show the real song; plays anyway if they cannot be read.
      void playerTrackInfo(pasted.id).then(
        (t) => playTrack(linkTrack(pasted, t)),
        () => playTrack(linkTrack(pasted)),
      );
      return;
    }
    pushRecent(text);
    const id = crypto.randomUUID();
    const previous = view.id;
    setView({ id, result: null, message: "", text });
    // Best effort: the previous search's own completion is discarded by the id check below.
    if (previous) searchCancel(previous).catch(() => {});
    try {
      const result = await searchQuery({ query: text, providers: [], limit: 10, cancellationId: id });
      setView((v) => (v.id === id ? { id: null, result, message: "", text } : v));
    } catch {
      setView((v) => (v.id === id ? { id: null, result: null, message: GENERIC_ERROR, text } : v));
    }
  }

  function cancel() {
    if (view.id) searchCancel(view.id).catch(() => setView((v) => ({ ...v, message: GENERIC_ERROR })));
  }

  function removeRecent(r: string) {
    useUi.getState().removeRecentSearch(r);
  }

  // Escape clears the field and the results; a search in flight is cancelled.
  function onInputKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "Escape" && (query !== "" || view.id || view.result)) {
      e.preventDefault();
      e.stopPropagation();
      setQuery("");
      if (view.id) searchCancel(view.id).catch(() => {});
      setView(IDLE); // a late completion fails the id check in runSearch and is dropped
    }
  }

  // ArrowUp and ArrowDown move through the result rows (from the field into the first row, and back).
  // Enter on a row plays its track through the visible play action.
  function onRootKeyDown(e: KeyboardEvent<HTMLDivElement>) {
    const rows = Array.from(rootRef.current?.querySelectorAll<HTMLElement>("[data-result-row]") ?? []);
    const active = document.activeElement as HTMLElement | null;
    if (e.key === "Enter" && active?.hasAttribute("data-result-row")) {
      active.querySelector<HTMLButtonElement>("[data-play-track]")?.click();
      return;
    }
    if ((e.key !== "ArrowDown" && e.key !== "ArrowUp") || rows.length === 0) return;
    if (active?.closest("[role='dialog']")) return;
    const i = active ? rows.findIndex((r) => r.contains(active)) : -1;
    if (i === -1 && active !== inputRef.current) return;
    e.preventDefault();
    if (e.key === "ArrowDown") rows[Math.min(i + 1, rows.length - 1)]?.focus();
    else if (i <= 0) inputRef.current?.focus();
    else rows[i - 1]?.focus();
  }

  const loading = view.id !== null;
  const result = view.result;
  const hits = result?.hits ?? [];
  const noHits = result !== null && hits.length === 0;
  const noticeGroups = result?.groups.filter((g) => g.kind !== "ok") ?? [];
  const link = parseYouTubeLink(query);
  const youtubeNeedsKey = providers.find((p) => p.provider === "YouTube")?.search === "RequiresAuth";

  return (
    <Screen id="search-title" title="Search" caption="Search YouTube Music and YouTube videos in one list.">
      <div ref={rootRef} onKeyDown={onRootKeyDown} className="flex flex-col gap-8">
        <form
          role="search"
          className="flex flex-col gap-3"
          onSubmit={(e) => {
            e.preventDefault();
            void runSearch(query);
          }}
        >
          <div className="flex items-center gap-2">
            <label htmlFor="search-query" className="sr-only">
              Search music
            </label>
            <div className="flex h-12 min-w-0 flex-1 items-center gap-3 rounded-full bg-frost-200 px-5 transition-colors focus-within:bg-frost-300 focus-within:outline-2 focus-within:outline-offset-2 focus-within:outline-primary-text">
              <SearchIcon size={18} aria-hidden="true" className="shrink-0 text-secondary-text" />
              <input
                ref={inputRef}
                id="search-query"
                type="search"
                value={query}
                onChange={(e) => { if(view.id)void searchCancel(view.id).catch(()=>{}); setView(IDLE);setQuery(e.currentTarget.value); }}
                onKeyDown={onInputKeyDown}
                placeholder="Songs, artists, videos, or paste a YouTube link"
                autoComplete="off"
                className="h-full min-w-0 flex-1 bg-transparent text-[17px] text-primary-text outline-none placeholder:text-tertiary-text [&::-webkit-search-cancel-button]:hidden"
              />
              {query !== "" && (
                <button
                  type="button"
                  aria-label="Clear search"
                  onClick={() => {
                    setQuery("");
                    inputRef.current?.focus();
                  }}
                  className="flex size-8 shrink-0 items-center justify-center rounded-full text-secondary-text hover:bg-frost-300 hover:text-primary-text"
                >
                  <X size={16} aria-hidden="true" />
                </button>
              )}
            </div>
            {loading ? (
              <Button variant="secondary" onClick={cancel} className="h-12 rounded-full px-6">
                Cancel
              </Button>
            ) : (
              <Button variant="primary" type="submit" disabled={query.trim() === ""} className="h-12 rounded-full px-6">
                Search
              </Button>
            )}
          </div>
          {providers.length > 0 && (
            <div role="group" aria-label="Provider status" className="flex flex-wrap items-center gap-x-4 gap-y-1 px-5 text-[12px] leading-4 text-secondary-text">
              {providers.map((p) => (
                <span key={p.provider}>
                  {providerName(p.provider)}: {SEARCH_WORDS[p.search] ?? "status unknown"}
                </span>
              ))}
              {youtubeNeedsKey && (
                <button type="button" onClick={() => navigate("settings")} className="-my-2 inline-flex h-8 items-center rounded-[6px] px-2 text-primary-text hover:bg-frost-100">
                  Add a key in Settings
                </button>
              )}
            </div>
          )}
        </form>

        {!loading && !link && query.trim() !== view.text && <SearchSuggestions query={query} onChoose={(text)=>{setQuery(text);void runSearch(text);}} />}

        <p aria-live="polite" className="sr-only">
          {loading ? "Searching" : view.message}
        </p>

        {loading && (
          <div role="status" aria-label="Searching connected providers" className="flex flex-col gap-1">
            <p className="flex items-center gap-2 pb-2 text-[13px] text-secondary-text">
              <Loader2 size={16} className="lx-spin" aria-hidden="true" />
              Searching {view.text ? `for “${view.text}”` : "connected providers"}…
            </p>
            {Array.from({ length: 5 }, (_, i) => (
              <Skeleton key={i} className="h-14" />
            ))}
          </div>
        )}

        {view.message && !loading && (
          <StatusNote role="alert" icon={<TriangleAlert size={16} />} tone="coral">
            <p>{view.message}</p>
            <Button variant="ghost" onClick={() => void runSearch(view.text || query)} className="mt-1 h-8 px-2">
              Retry
            </Button>
          </StatusNote>
        )}

        {link && <LinkPlay link={link} />}

        {!loading && !link && hits.length > 0 && (
          <Section id="search-songs" title="Songs"><ul aria-label="Search results" className="lx-stagger flex flex-col">
            {hits.map((hit, i) => <ResultRow key={`${hit.provider}:${hit.providerTrackId}`} hit={hit} following={hits.slice(i + 1)} />)}
          </ul></Section>
        )}
        {!loading && noticeGroups.map(group => <SourceGroup key={group.provider} group={group} onRetry={() => void runSearch(view.text)} />)}

        {!loading && noHits && noticeGroups.length === 0 && (
          <EmptyState icon={<SearchIcon size={24} />} title={`No results for “${view.text}”`}>
            Try another spelling, or search for the artist or song name only.
          </EmptyState>
        )}

        {!loading && !result && !view.message && (
          <>
            {recent.length > 0 && (
              <Section
                id="search-recent"
                title="Recent searches"
                action={
                  <Button variant="ghost" onClick={clearRecent}>
                    Clear all
                  </Button>
                }
              >
                <ul aria-label="Recent searches" className="flex flex-col">
                  <AnimatePresence initial={false}>
                  {recent.map((r) => (
                    <CollapseLi key={r}>
                      <div className="flex h-12 items-center gap-1 rounded-[8px] pr-1 pl-2 hover:bg-frost-200 focus-within:bg-frost-200">
                      <button
                        type="button"
                        onClick={() => {
                          setQuery(r);
                          void runSearch(r);
                        }}
                        title={r}
                        className="flex h-10 min-w-0 flex-1 items-center gap-3 rounded-[6px] text-left text-[13px] text-primary-text"
                      >
                        <Clock size={16} aria-hidden="true" className="shrink-0 text-secondary-text" />
                        <span className="truncate">{r}</span>
                      </button>
                      <button
                        type="button"
                        aria-label={`Remove ${r} from recent searches`}
                        title="Remove"
                        onClick={() => removeRecent(r)}
                        className="flex size-8 shrink-0 items-center justify-center rounded-[8px] text-secondary-text hover:bg-frost-300 hover:text-primary-text"
                      >
                        <X size={16} aria-hidden="true" />
                      </button>
                      </div>
                    </CollapseLi>
                  ))}
                  </AnimatePresence>
                </ul>
              </Section>
            )}
            <LiveFeed kind="home" limit={6} />
          </>
        )}
      </div>
    </Screen>
  );
}
