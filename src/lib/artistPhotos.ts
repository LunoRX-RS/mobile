"use client";
import { useEffect, useState } from "react";
import { libraryArtist } from "../services/tauri/library";
import type { ArtistRef } from "../services/tauri/search";
import { prefetchImages } from "./prefetch";

// Artist photos only exist inside the full artist profile, so each one costs a provider request. They are fetched two at a
// time (core allows two), remembered on disk, and warmed as soon as the liked songs load, so the Artists page opens with
// every photo already known. A failed fetch is never remembered as "no photo": it is retried after a minute.
const KEY = "lunorx.artistPhotos";
const MAX_SAVED = 300;
const RETRY_MS = 60_000;

const photos = new Map<string, string>(); // key -> url
const failedAt = new Map<string, number>();
const inflight = new Map<string, Promise<string | null>>();
const waiting: Array<() => void> = [];
let running = 0;
let loaded = false;
const listeners = new Set<() => void>();

const keyOf = (a: ArtistRef) => `${a.provider}:${a.id}`;

function load(): void {
  if (loaded || typeof window === "undefined") return;
  loaded = true;
  try {
    const raw = window.localStorage.getItem(KEY);
    const saved = raw ? (JSON.parse(raw) as Record<string, string>) : {};
    for (const [k, v] of Object.entries(saved)) if (typeof v === "string") photos.set(k, v);
    prefetchImages(photos.values());
  } catch { /* corrupt or blocked storage: start empty */ }
}
function save(): void {
  try {
    const entries = Array.from(photos.entries()).slice(-MAX_SAVED);
    window.localStorage.setItem(KEY, JSON.stringify(Object.fromEntries(entries)));
  } catch { /* storage full or blocked: photos are refetched next session */ }
}
function pump(): void {
  while (running < 2 && waiting.length > 0) { running++; waiting.shift()!(); }
}

/** The cached photo URL, or null when none is known yet. */
export function cachedArtistPhoto(a: ArtistRef): string | null {
  load();
  return a.id ? photos.get(keyOf(a)) ?? null : null;
}

/** Fetches one artist's photo (queued, two at a time). Resolves null when there is none or the fetch failed. */
export function fetchArtistPhoto(a: ArtistRef): Promise<string | null> {
  load();
  if (!a.id) return Promise.resolve(null);
  const key = keyOf(a);
  const known = photos.get(key);
  if (known) return Promise.resolve(known);
  if ((failedAt.get(key) ?? 0) + RETRY_MS > Date.now()) return Promise.resolve(null);
  const pending = inflight.get(key);
  if (pending) return pending;
  const job = new Promise<string | null>((resolve) => {
    waiting.push(() => {
      libraryArtist(a.provider, a.id!)
        .then((p) => p.artworkUrl ?? null, () => { failedAt.set(key, Date.now()); return null; })
        .then((url) => {
          if (url) { photos.set(key, url); save(); prefetchImages([url]); listeners.forEach((l) => l()); }
          inflight.delete(key); resolve(url); running--; pump();
        });
    });
    pump();
  });
  inflight.set(key, job);
  return job;
}

/** Starts fetching photos for these artists in the background (most recent first). */
export function warmArtistPhotos(artists: Iterable<ArtistRef>): void {
  for (const a of artists) void fetchArtistPhoto(a);
}

export function useArtistPhoto(a: ArtistRef): string | null {
  const [url, setUrl] = useState<string | null>(() => cachedArtistPhoto(a));
  useEffect(() => {
    let live = true;
    const sync = () => { if (live) setUrl(cachedArtistPhoto(a)); };
    sync();
    listeners.add(sync);
    void fetchArtistPhoto(a).then(() => sync());
    return () => { live = false; listeners.delete(sync); };
  }, [a.provider, a.id]); // eslint-disable-line react-hooks/exhaustive-deps
  return url;
}
