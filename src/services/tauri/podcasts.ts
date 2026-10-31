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

// Podcasts read from their own RSS feeds (commands/podcasts.rs). Shows are found through Apple's public directory.

export interface ShowDto {
  /** The show's feed address. */
  id: string;
  title: string;
  author: string;
  artworkUrl: string | null;
}

export interface EpisodeDto {
  /** The episode's audio address: it is also the track's id and address when played. */
  id: string;
  title: string;
  show: string;
  published: string | null;
  durationMs: number | null;
  artworkUrl: string | null;
}

export function podcastShows(query: string): Promise<ShowDto[]> {
  return invoke<ShowDto[]>("podcast_shows", { query }).then((shows) => { prefetchCovers(shows, 12); return shows; });
}

export function podcastEpisodes(feedUrl: string): Promise<EpisodeDto[]> {
  return invoke<EpisodeDto[]>("podcast_episodes", { feedUrl });
}

/** An episode as the player takes it. */
export function episodeTrack(e: EpisodeDto) {
  return {
    provider: "Podcast",
    providerTrackId: e.id,
    canonicalUrl: e.id,
    title: e.title,
    artists: [e.show],
    artworkUrl: e.artworkUrl,
    durationMs: e.durationMs,
  };
}

/** The long uploads (ten minutes or more) of a YouTube channel: talks, episodes and stories, not songs. */
export function podcastChannel(channelId: string, name: string, continuation: string | null = null): Promise<PodcastPageDto> {
  return invoke<PodcastPageDto>("podcast_channel", { channelId, name, continuation }).then((page) => {
    prefetchCovers(page.hits, 12);
    return page;
  });
}
