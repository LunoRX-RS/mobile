"use client";
import { useEffect } from "react";
import { create } from "zustand";
import { discoveryFeed, type FeedSection } from "../services/tauri/discovery";
import { prefetchCovers } from "./prefetch";
interface Feed { sections: FeedSection[]; loading: boolean; error: string; updated: number }
const versions: Record<string,number> = {};
// The last feed of each kind/topic is kept on disk, so Home and Explore draw instantly and refresh behind the rows.
const CACHE = "lunorx.feed.";
function readCached(key: string): Feed | null {
  try {
    const raw = window.localStorage.getItem(CACHE + key);
    const v = raw ? JSON.parse(raw) as { sections?: FeedSection[]; updated?: number } : null;
    if (!v || !Array.isArray(v.sections) || typeof v.updated !== "number") return null;
    return { sections: v.sections, loading: false, error: "", updated: v.updated };
  } catch { return null; }
}
function writeCached(key: string, feed: Feed) {
  try {
    const sections = feed.sections.slice(0, 6).map((s) => ({ ...s, tracks: s.tracks.slice(0, 20) }));
    window.localStorage.setItem(CACHE + key, JSON.stringify({ sections, updated: feed.updated }));
  } catch { /* storage full or blocked: the feed still works uncached */ }
}
const EMPTY: Feed = { sections: [], loading: false, error: "", updated: 0 };
export const useFeeds = create<{ feeds: Record<string, Feed> }>(() => ({ feeds: {} }));
export async function loadFeed(kind: "home" | "explore", topic = "charts", force = false) {
  const key = `${kind}:${topic}`;
  let current = useFeeds.getState().feeds[key] ?? EMPTY;
  if (!current.updated) { const cached = readCached(key); if (cached) { current = cached; useFeeds.setState((s) => ({ feeds: { ...s.feeds, [key]: cached } })); for (const s of cached.sections) prefetchCovers(s.tracks); } }
  if (current.loading || (!force && Date.now() - current.updated < 300000)) return;
  const request = versions[key] = (versions[key] ?? 0) + 1;
  const put = (feed: Feed) => { if(versions[key] !== request)return;useFeeds.setState(s => ({ feeds: { ...s.feeds, [key]: feed } })); };
  put({ ...current, loading: true, error: "" });
  try { const next = { sections: await discoveryFeed(kind, topic), loading: false, error: "", updated: Date.now() }; put(next); if (versions[key] === request) writeCached(key, next); }
  catch { put({ ...current, loading: false, error: "Could not load music from YouTube Music. Check your connection and retry." }); }
}
export function useFeed(kind: "home" | "explore", topic = "charts") {
  const feed = useFeeds(s => s.feeds[`${kind}:${topic}`] ?? EMPTY);
  useEffect(() => { void loadFeed(kind, topic); }, [kind, topic]);
  return feed;
}

export function invalidateHomeFeed() {
  const key="home:charts";
  if(!useFeeds.getState().feeds[key])return;
  versions[key]=(versions[key] ?? 0)+1;
  useFeeds.setState(s=>{const feeds={...s.feeds};delete feeds[key];return {feeds};});
  void loadFeed("home","charts",true);
}
