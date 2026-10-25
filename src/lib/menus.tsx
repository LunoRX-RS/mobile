"use client";
import { ArrowDown, ArrowUp, Download, ListEnd, ExternalLink, Heart, HeartOff, Link2, ListPlus, Play, Trash2, User, FolderOpen } from "lucide-react";
import type { MenuItem } from "../components/ContextMenu";
import { playTrack } from "./playback";
import { toggleSaved, trackKey, useLibrary } from "./library";
import { useUi } from "./store";
import { addToPlaylist, downloadPlaylist, listPlaylists, playPlaylist } from "./playlistActions";
import { playerOpenExternal, playerQueueEdit } from "../services/tauri/player";
import type { SearchHitDto } from "../services/tauri/search";
import type { LocalPlaylistDto } from "../services/tauri/sync";
import { copyText } from "./clipboard";

const ICON = 14;

/** Right-click menu for a track row. `onRemove` adds "Remove from this playlist" when the row sits in one. */
export function trackMenu(hit: SearchHitDto, opts: { onRemove?: () => void; onMoveUp?: () => void; onMoveDown?: () => void; following?: SearchHitDto[] } = {}) {
  const youtube = hit.provider === "YouTube";
  const build = (): MenuItem[] => {
    const liked = useLibrary.getState().tracks.some((t) => trackKey(t) === trackKey(hit));
    const artist = hit.artistRefs?.[0] ?? (hit.artists[0] ? { provider: hit.provider, id: null, name: hit.artists[0] } : null);
    const items: MenuItem[] = [];
    if (youtube) items.push({ label: "Play now", icon: <Play size={ICON} fill="currentColor" />, onSelect: () => void playTrack(hit, false, (opts.following ?? []).filter((t) => t.provider === "YouTube")) });
    if (youtube) items.push({ label: "Play next", icon: <ListEnd size={ICON} />, onSelect: () => void playerQueueEdit({ op: "next", track: { provider: "YouTube", id: hit.providerTrackId, title: hit.title, artists: hit.artists, artworkUrl: hit.artworkUrl ?? null, durationMs: hit.durationMs ?? null } }).catch(() => {}) });
    items.push({ label: liked ? "Remove from Liked Songs" : "Add to Liked Songs", icon: liked ? <HeartOff size={ICON} /> : <Heart size={ICON} />, onSelect: () => void toggleSaved(hit) });
    if (artist) items.push({ label: `Go to ${artist.name}`, icon: <User size={ICON} />, onSelect: () => useUi.getState().openArtist(artist) });
    if (youtube) items.push({ label: "Open on YouTube", icon: <ExternalLink size={ICON} />, onSelect: () => void playerOpenExternal({ provider: hit.provider, providerTrackId: hit.providerTrackId, canonicalUrl: hit.canonicalUrl }).catch(() => {}) });
    items.push({ label: "Copy link", icon: <Link2 size={ICON} />, onSelect: () => void copyText(hit.canonicalUrl) });
    if (opts.onMoveUp || opts.onMoveDown) items.push({ kind: "separator" }, { label: "Move up", icon: <ArrowUp size={ICON} />, disabled: !opts.onMoveUp, onSelect: () => opts.onMoveUp?.() }, { label: "Move down", icon: <ArrowDown size={ICON} />, disabled: !opts.onMoveDown, onSelect: () => opts.onMoveDown?.() });
    if (opts.onRemove) items.push({ kind: "separator" }, { label: "Remove from this playlist", icon: <Trash2 size={ICON} />, danger: true, onSelect: opts.onRemove });
    return items;
  };
  // The playlists to add to arrive after the menu is open.
  const load = async (): Promise<MenuItem[]> => {
    const lists = await listPlaylists();
    if (lists.length === 0) return [];
    return [
      { kind: "separator" },
      { kind: "heading", label: "Add to playlist" },
      ...lists.slice(0, 8).map((p): MenuItem => ({ label: p.name, icon: <ListPlus size={ICON} />, onSelect: () => void addToPlaylist(p.id, hit).catch(() => {}) })),
    ];
  };
  return { build, load };
}

/** Right-click menu for a playlist (sidebar and editor). */
export function playlistMenu(p: Pick<LocalPlaylistDto, "id" | "name" | "remoteId" | "entryCount">, opts: { onOpen?: () => void } = {}) {
  return (): MenuItem[] => [
    ...(opts.onOpen ? [{ label: "Open", icon: <FolderOpen size={ICON} />, onSelect: opts.onOpen } as MenuItem] : []),
    { label: "Play now", icon: <Play size={ICON} fill="currentColor" />, disabled: p.entryCount === 0, onSelect: () => void playPlaylist(p.id).catch(() => {}) },
    { label: "Download to local", icon: <Download size={ICON} />, disabled: p.entryCount === 0, onSelect: () => void downloadPlaylist(p.id).catch(() => {}) },
    { kind: "separator" },
    { label: p.remoteId ? "Delete (uploaded playlists can't be deleted yet)" : "Delete playlist…", icon: <Trash2 size={ICON} />, danger: true, disabled: !!p.remoteId, onSelect: () => useUi.getState().requestDelete(p.id) },
  ];
}
