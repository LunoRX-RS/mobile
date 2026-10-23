import { invoke } from "@tauri-apps/api/core";
import { prefetchCovers } from "../../lib/prefetch";

// Mirrors commands/dto.rs. Tauri maps snake_case Rust argument names to camelCase JS keys.

export interface SearchQueryArgs {
  query: string;
  providers: string[]; // "Spotify" | "YouTube"; empty = all
  limit: number; // 1..=10
  cancellationId: string; // 1..=64 chars, [A-Za-z0-9-]
}

export interface ArtistRef { provider: string; id: string | null; name: string }

export interface SearchHitDto {
  source?: "YouTube" | "YouTube Music";
  provider: string;
  providerTrackId: string;
  canonicalUrl: string;
  title: string;
  artists: string[];
  artistRefs?: ArtistRef[];
  artworkUrl: string | null;
  durationMs: number | null;
}

export type SourceGroupDto =
  | { kind: "ok"; provider: string; hits: SearchHitDto[] }
  | {
      kind: "failed";
      provider: string;
      category: string; // e.g. quotaExceeded | authenticationRequired | networkError | providerError | providerUnavailable
    }
  | {
      kind: "skipped";
      provider: string;
      reason: "capabilityNotSupported" | "quotaExhausted" | "cancelled";
    };

export interface SearchResponseDto {
  query: string;
  groups: SourceGroupDto[];
  hits: SearchHitDto[];
}

export interface ProviderStatusDto {
  provider: string;
  search: string; // Supported | Unsupported | RequiresAuth | RequiresEntitlement | TemporarilyUnavailable | Unverified
  playback: string; // same value set as search; native playback readiness
  externalPlayback: string; // same value set as search; opens the provider page in the browser
}

// Thrown (rejected) by invoke. Never show it raw.
export type IpcError =
  | { category: "validation"; message: string }
  | { category: "duplicateCancellationId" }
  | { category: "internal" };

export function searchQuery(args: SearchQueryArgs): Promise<SearchResponseDto> {
  return invoke<SearchResponseDto>("search_query", { args }).then((r) => { prefetchCovers(r.hits, 16); return r; });
}

// Rust param `cancellation_id` is sent as `cancellationId`.
export function searchCancel(cancellationId: string): Promise<void> {
  return invoke<void>("search_cancel", { cancellationId });
}

export function providersList(): Promise<ProviderStatusDto[]> {
  return invoke<ProviderStatusDto[]>("providers_list");
}
