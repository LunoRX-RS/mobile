"use client";
import { useEffect, useState } from "react";
import { Button, Screen } from "../../components/ui";
import LiveFeed from "../discovery/LiveFeed";
import { loadFeed } from "../../lib/discovery";
import { discoveryGenres, type Genre } from "../../services/tauri/discovery";

// Shown until YouTube Music's own moods and genres arrive, and kept if they never do.
const FALLBACK: Genre[] = [["thai", "Thai music"], ["pop", "Pop"], ["hiphop", "Hip-hop"], ["focus", "Focus"], ["chill", "Chill"], ["acoustic", "Acoustic"]]
  .map(([id, name]) => ({ id: `fixed:${id}`, name, mood: false }));
const KEY = "lunorx.exploreGenres";
const SHOWN = 12;
const topicOf = (g: Genre) => (g.id.startsWith("fixed:") ? g.id.slice(6) : `genre:${g.id}`);

function readGenres(): Genre[] | null {
  try { const v = JSON.parse(window.localStorage.getItem(KEY) ?? "null"); return Array.isArray(v) && v.length ? v : null; } catch { return null; }
}

export default function ExploreScreen() {
  const [topic, setTopic] = useState("charts");
  const [genres, setGenres] = useState<Genre[]>(FALLBACK);
  const [all, setAll] = useState(false);
  useEffect(() => {
    let live = true;
    const cached = readGenres();
    if (cached) setGenres(cached);
    discoveryGenres().then((list) => {
      if (!live || list.length === 0) return;
      setGenres(list);
      try { window.localStorage.setItem(KEY, JSON.stringify(list)); } catch { /* uncached */ }
    }, () => {});
    return () => { live = false; };
  }, []);
  const shown = all ? genres : genres.slice(0, SHOWN);
  const chip = (id: string, title: string) => (
    <Button key={id} variant={topic === id ? "primary" : "secondary"} aria-pressed={topic === id} onPointerEnter={() => void loadFeed("explore", id)} onFocus={() => void loadFeed("explore", id)} onClick={() => setTopic(id)}>{title}</Button>
  );
  return (
    <Screen id="explore-title" title="Explore" caption="New releases, charts, moods and genres from YouTube Music. Choose a song to play or like.">
      <div className="flex flex-col gap-8">
        <nav aria-label="Explore music categories" className="flex flex-wrap gap-2">
          {chip("charts", "Discover")}
          {shown.map((g) => chip(topicOf(g), g.name))}
          {genres.length > SHOWN && <Button variant="ghost" onClick={() => setAll(!all)}>{all ? "Fewer" : `All ${genres.length}`}</Button>}
        </nav>
        <LiveFeed kind="explore" topic={topic} limit={20} />
      </div>
    </Screen>
  );
}
