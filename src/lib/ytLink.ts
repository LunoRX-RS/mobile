// Recognises a YouTube or YouTube Music link pasted into Search, so it can be played directly.
// Only the video id is taken from the link; everything else in it (playlist, time, tracking) is ignored.

const ID = /^[A-Za-z0-9_-]{11}$/;
const HOSTS = new Set(["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "youtu.be", "www.youtu.be"]);

export interface YouTubeLink {
  id: string;
  /** music.youtube.com: shown as YouTube Music. */
  music: boolean;
}

export function parseYouTubeLink(text: string): YouTubeLink | null {
  const raw = text.trim();
  if (raw === "" || /\s/.test(raw)) return null;
  let url: URL;
  try {
    url = new URL(/^https?:\/\//i.test(raw) ? raw : `https://${raw}`);
  } catch {
    return null;
  }
  const host = url.hostname.toLowerCase();
  if (!HOSTS.has(host) || (url.protocol !== "https:" && url.protocol !== "http:")) return null;
  let id: string | null = null;
  if (host.endsWith("youtu.be")) {
    id = url.pathname.split("/")[1] ?? null;
  } else if (url.pathname === "/watch") {
    id = url.searchParams.get("v");
  } else {
    const m = /^\/(?:shorts|embed|live|v)\/([^/]+)/.exec(url.pathname);
    id = m ? m[1] : null;
  }
  return id && ID.test(id) ? { id, music: host === "music.youtube.com" } : null;
}
