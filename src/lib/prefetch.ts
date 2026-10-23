// Warms the image cache before a list is drawn, so covers are ready when the row appears.
// Four loads at a time, each URL once, nothing on Data Saver. The browser's own HTTP and memory caches do the keeping;
// the Image objects are held (bounded) so decoded bitmaps are not dropped straight away.
const MAX_HELD = 400;
const held = new Map<string, HTMLImageElement>();
const queue: string[] = [];
let running = 0;

const skip = (): boolean => typeof window === "undefined" || (navigator as { connection?: { saveData?: boolean } }).connection?.saveData === true;
const isUrl = (u: unknown): u is string => typeof u === "string" && /^(https:|data:image\/)/.test(u);

function pump(): void {
  while (running < 4 && queue.length > 0) {
    const url = queue.shift()!;
    running++;
    const img = new Image();
    img.referrerPolicy = "no-referrer"; // same as TrackCover, so the cache entry is shared
    img.decoding = "async";
    const done = () => { running--; pump(); };
    img.onload = done;
    img.onerror = () => { held.delete(url); done(); };
    img.src = url;
    held.set(url, img);
    if (held.size > MAX_HELD) held.delete(held.keys().next().value as string);
  }
}

/** Queues image URLs (ignores empties and anything already requested). */
export function prefetchImages(urls: Iterable<string | null | undefined>): void {
  if (skip()) return;
  for (const url of urls) {
    if (!isUrl(url) || held.has(url) || queue.includes(url)) continue;
    queue.push(url);
  }
  pump();
}

type WithArt = { artworkUrl?: string | null; providerTrackId?: string; provider?: string };
const fallbackCover = (t: WithArt): string | null =>
  t.provider === "YouTube" && t.providerTrackId && /^[A-Za-z0-9_-]{11}$/.test(t.providerTrackId) ? `https://i.ytimg.com/vi/${t.providerTrackId}/hqdefault.jpg` : null;

/** Covers for a list of tracks: their artwork URL, or the YouTube thumbnail TrackCover falls back to. */
export function prefetchCovers(tracks: readonly WithArt[] | null | undefined, limit = 24): void {
  if (!tracks) return;
  prefetchImages(tracks.slice(0, limit).map((t) => t.artworkUrl || fallbackCover(t)));
}
