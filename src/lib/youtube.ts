export function youtubeVideoId(input: string): string | null {
  const value = input.trim();
  const valid = (id: string | null): id is string => id !== null && /^[A-Za-z0-9_-]{11}$/.test(id);
  if (valid(value)) return value;
  try {
    const url = new URL(value);
    if (url.protocol !== "https:" || url.username || url.password || url.port) return null;
    let id: string | null = null;
    if (url.hostname === "youtu.be") id = url.pathname.slice(1);
    else if (["youtube.com", "www.youtube.com", "music.youtube.com", "m.youtube.com"].includes(url.hostname)) {
      if (url.pathname === "/watch") id = url.searchParams.get("v");
      else if (/^\/(shorts|embed|live)\//.test(url.pathname)) id = url.pathname.split("/")[2] ?? null;
    }
    return valid(id) ? id : null;
  } catch { return null; }
}
