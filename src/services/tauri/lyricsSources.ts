import { invoke } from "@tauri-apps/api/core";
import type { LyricsSourceDto } from "./player";

// Mirrors commands/lyrics_sources.rs. Tauri maps the Rust `args` struct fields to these camelCase keys.
// Lyrics sources are unofficial services; core sends the song title and artist to each source that is turned on.

export interface LyricsTrackArgs {
  providerTrackId: string;
  title: string;
  artists: string[];
  album?: string;
  durationMs: number;
}

/** "Try again": fetch one source for this track and return its entry. */
export function lyricsSourceFetch(args: LyricsTrackArgs & { sourceId: string }): Promise<LyricsSourceDto> {
  return invoke("lyrics_source_fetch", { args });
}

export interface LyricsCandidateDto {
  sourceId: string;
  /** Opaque: pass it back to lyricsSourceBind unchanged. */
  key: string;
  title: string;
  artist: string;
  durationMs: number;
  synced: boolean;
  words: boolean;
}

export function lyricsSourceSearch(args: { title: string; artists?: string[]; query: string; sourceId?: string }): Promise<{ candidates: LyricsCandidateDto[] }> {
  return invoke("lyrics_source_search", { args });
}

/** Persists the match for this track and source in core and returns the new source entry. */
export function lyricsSourceBind(args: { providerTrackId: string; sourceId: string; candidateKey: string; title: string; artists: string[]; durationMs: number }): Promise<LyricsSourceDto> {
  return invoke("lyrics_source_bind", { args });
}

export function lyricsSourceUnbind(args: { providerTrackId: string; sourceId: string }): Promise<void> {
  return invoke("lyrics_source_unbind", { args });
}

export interface LyricsSettingsDto {
  /** Auto order, first is tried first. */
  order: string[];
  /** Ids turned off: nothing is sent to them. */
  disabled: string[];
  sources: { id: string; label: string }[];
}

export function lyricsSettingsGet(): Promise<LyricsSettingsDto> {
  return invoke("lyrics_settings_get", { args: {} });
}

export function lyricsSettingsSet(order: string[], disabled: string[]): Promise<LyricsSettingsDto> {
  return invoke("lyrics_settings_set", { args: { order, disabled } });
}
