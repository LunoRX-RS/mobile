"use client";

import { useEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { ListPlus, Plus } from "lucide-react";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import type { SearchHitDto } from "../../services/tauri/search";
import {
  syncAddLocalEntry,
  syncCreateLocalPlaylist,
  syncListLocalPlaylists,
  type ErrorCategory,
  type LocalEntryDto,
  type LocalPlaylistDto,
  type Provider,
} from "../../services/tauri/sync";

// Rejections are bare category strings; anything else falls back to internal.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e)
    ? (e as ErrorCategory)
    : "internal";
}

// Spotify entries store ids only: the backend rejects title, artists, artwork and duration on them.
function toEntry(hit: SearchHitDto): LocalEntryDto {
  const provider = hit.provider as Provider;
  const spotify = provider === "Spotify";
  return {
    provider,
    providerTrackId: hit.providerTrackId,
    providerUrl: hit.canonicalUrl,
    title: spotify ? null : hit.title,
    artists: spotify ? null : hit.artists,
    artworkUrl: spotify ? null : hit.artworkUrl,
    durationMs: spotify ? null : hit.durationMs,
  };
}

/** Adds one track, or every track in `all` (an album), to a chosen local playlist. `label` shows a text button instead of the icon. */
export function AddToPlaylist({ hit, all, label }: { hit: SearchHitDto; all?: SearchHitDto[]; label?: string }) {
  const [open, setOpen] = useState(false);
  const [playlists, setPlaylists] = useState<LocalPlaylistDto[] | null>(null);
  const [loadError, setLoadError] = useState<ErrorCategory | null>(null);
  const [addError, setAddError] = useState<ErrorCategory | null>(null);
  const [busy, setBusy] = useState(false);
  const [done, setDone] = useState("");
  const [creating, setCreating] = useState(false);
  const [newName, setNewName] = useState("");
  const triggerRef = useRef<HTMLButtonElement>(null);
  const dialogRef = useRef<HTMLDivElement>(null);

  // Reload each time the picker opens so new playlists show up.
  useEffect(() => {
    if (!open) return;
    let cancelled = false;
    setPlaylists(null);
    setLoadError(null);
    setAddError(null);
    setCreating(false);
    syncListLocalPlaylists()
      .then((list) => {
        if (!cancelled) setPlaylists(list);
      })
      .catch((e) => {
        if (!cancelled) setLoadError(toCategory(e));
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  useEffect(() => {
    if (open) dialogRef.current?.focus();
  }, [open]);

  // Closing always returns focus to the trigger, for Escape, Cancel and a successful add.
  function close() {
    setOpen(false);
    triggerRef.current?.focus();
  }

  async function createAndAdd(e: FormEvent) {
    e.preventDefault();
    const name = newName.trim();
    if (name === "" || busy) return;
    setBusy(true);
    setAddError(null);
    setDone("");
    try {
      const id = await syncCreateLocalPlaylist(name, null);
      const items = all ?? [hit];
      for (const item of items) await syncAddLocalEntry(id, toEntry(item));
      setDone(`Saved ${items.length > 1 ? `${items.length} songs` : "song"} to new playlist ${name}.`);
      close();
    } catch (e) {
      setAddError(toCategory(e));
    } finally {
      setBusy(false);
    }
  }

  async function add(p: LocalPlaylistDto) {
    setBusy(true);
    setAddError(null);
    setDone("");
    try {
      const items = all ?? [hit];
      for (const item of items) await syncAddLocalEntry(p.id, toEntry(item));
      setDone(`Added ${items.length > 1 ? `${items.length} songs` : "song"} to ${p.name}.`);
      close();
    } catch (e) {
      setAddError(toCategory(e));
    } finally {
      setBusy(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLSpanElement>) {
    if (open && e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  }

  return (
    <span onKeyDown={onKeyDown} className="relative inline-flex">
      <button
        ref={triggerRef}
        type="button"
        aria-label={label ?? `Add to playlist: ${hit.title}`}
        title="Add to playlist"
        aria-haspopup="dialog"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
        className={label ? "inline-flex h-10 items-center gap-2 rounded-full bg-frost-200 px-5 text-[14px] font-semibold text-primary-text transition-colors duration-150 hover:bg-frost-300" : "inline-flex size-8 items-center justify-center rounded-[8px] text-secondary-text transition-colors duration-150 hover:bg-frost-300 hover:text-primary-text"}
      >
        <ListPlus size={18} aria-hidden="true" />{label}
      </button>
      {open && (
        <div
          ref={dialogRef}
          role="dialog"
          aria-label="Choose a playlist"
          tabIndex={-1}
          className="glass-solid lx-pop-in absolute top-full right-0 z-20 mt-2 flex w-64 flex-col gap-0.5 rounded-[10px] p-1.5 text-[13px] shadow-[0_24px_64px_rgba(0,0,0,0.55)]"
        >
          {playlists === null && loadError === null && (
            <p role="status" className="px-3 py-2 text-secondary-text">Loading playlists…</p>
          )}
          {loadError && <p role="alert" className="lx-note px-3 py-2 text-critical-coral">{CATEGORY_MESSAGES[loadError]}</p>}
          {playlists && playlists.length > 0 && (
            <ul>
              {playlists.map((p) => (
                <li key={p.id}>
                  <button type="button" disabled={busy} onClick={() => add(p)} className="flex h-8 w-full items-center rounded-[6px] px-3 text-left text-[13px] text-primary-text hover:bg-lumen-wash disabled:opacity-40">
                    {p.name}
                  </button>
                </li>
              ))}
            </ul>
          )}
          {creating ? (
            <form onSubmit={createAndAdd} className="flex flex-col gap-1.5 px-1.5 py-1" aria-label="New playlist">
              <input
                autoFocus
                value={newName}
                onChange={(e) => setNewName(e.target.value)}
                maxLength={120}
                aria-label="Playlist name"
                placeholder="Playlist name"
                className="h-8 rounded-[6px] bg-frost-200 px-3 text-[13px] text-primary-text outline-none placeholder:text-tertiary-text focus-visible:outline-2 focus-visible:outline-lumen-lavender"
              />
              <button type="submit" disabled={busy || newName.trim() === ""} className="flex h-8 items-center justify-center rounded-[6px] bg-lumen-lavender text-[13px] font-semibold text-black hover:opacity-90 disabled:opacity-40">
                Create and save
              </button>
            </form>
          ) : (
            <button type="button" disabled={busy} onClick={() => { setNewName(all ? "" : hit.title); setCreating(true); }} className="flex h-8 items-center gap-2 rounded-[6px] px-3 text-left text-[13px] text-primary-text hover:bg-lumen-wash disabled:opacity-40">
              <Plus size={14} aria-hidden="true" />Save as new playlist
            </button>
          )}
          {addError && <p role="alert" className="lx-note px-3 py-2 text-critical-coral">{CATEGORY_MESSAGES[addError]}</p>}
          <button type="button" onClick={close} disabled={busy} className="mt-0.5 flex h-8 items-center rounded-[6px] px-3 text-[13px] text-secondary-text hover:bg-frost-300 hover:text-primary-text disabled:opacity-40">
            Cancel
          </button>
        </div>
      )}
      <span role="status" className="sr-only">{done}</span>
    </span>
  );
}
