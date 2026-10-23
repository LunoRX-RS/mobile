import { invoke } from "@tauri-apps/api/core";
import type { SearchHitDto } from "./search";
import { prefetchCovers, prefetchImages } from "../../lib/prefetch";
export interface FeedAlbum { id: string; name: string; year: number | null; artists: string[]; artworkUrl: string | null }
export interface FeedSection { title: string; tracks: SearchHitDto[]; albums?: FeedAlbum[] }
/** A mood or genre from YouTube Music's Explore page. Its topic id is `genre:<id>`. */
export interface Genre { id: string; name: string; mood: boolean }
export const discoveryGenres = () => invoke<Genre[]>("discovery_genres");
export const discoveryFeed = (kind: "home" | "explore", topic = "charts") =>
  invoke<FeedSection[]>("discovery_feed", { kind, topic }).then((sections) => { for (const s of sections) { prefetchCovers(s.tracks); prefetchImages((s.albums ?? []).map((a) => a.artworkUrl)); } return sections; });
export const searchSuggestions = (query: string) => invoke<string[]>("search_suggestions", { query });
