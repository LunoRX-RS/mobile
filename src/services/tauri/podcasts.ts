import { invoke } from "@tauri-apps/api/core";
import { prefetchCovers } from "../../lib/prefetch";
import type { SearchHitDto } from "./search";

// Mirrors commands/podcasts.rs. Episodes are ordinary YouTube videos of at least 20 minutes.

export interface PodcastPageDto {
  hits: SearchHitDto[];
  /** Pass back as `continuation` for the next page; null when the list ends. */
  continuation: string | null;
}

export function podcastBrowse(query: string, continuation: string | null = null): Promise<PodcastPageDto> {
  return invoke<PodcastPageDto>("podcast_browse", { query, continuation }).then((page) => {
    prefetchCovers(page.hits, 16);
    return page;
  });
}
