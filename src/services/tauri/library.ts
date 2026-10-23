import { invoke } from "@tauri-apps/api/core";
import type { SearchHitDto } from "./search";
import { prefetchCovers, prefetchImages } from "../../lib/prefetch";
export interface ArtistProfile {
  provider: string; id: string; name: string; description: string | null;
  artworkUrl: string | null; subscribers: number | null; tracks: SearchHitDto[];
  albums: { id: string; name: string; year: number | null; artworkUrl: string | null }[];
}
export interface AlbumProfile {
  provider: string; id: string; name: string; kind: string; year: number | null;
  artists: { provider: string; id: string | null; name: string }[];
  artworkUrl: string | null; tracks: SearchHitDto[];
}
export const libraryAlbum = (provider: string, id: string) =>
  invoke<AlbumProfile>("library_album", { provider, id }).then((a) => { prefetchImages([a.artworkUrl]); prefetchCovers(a.tracks, 8); return a; });
export const libraryLikes = () => invoke<SearchHitDto[]>("library_likes").then((t) => { prefetchCovers(t, 30); return t; });
export const librarySetLike = (track: SearchHitDto, liked: boolean) => invoke<SearchHitDto[]>("library_set_like", { track, liked });
export const libraryArtist = (provider: string, id: string) =>
  invoke<ArtistProfile>("library_artist", { provider, id }).then((p) => { prefetchImages([p.artworkUrl, ...p.albums.map((a) => a.artworkUrl)]); prefetchCovers(p.tracks, 12); return p; });
