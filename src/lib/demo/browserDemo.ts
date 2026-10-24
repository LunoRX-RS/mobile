"use client";

// Development only, and only in a plain browser (no Tauri): answers the app's IPC calls with sample songs so the
// screens can be looked at and clicked through. Nothing here streams anything: playback is a generated tone melody and
// every song, artist and lyric line is made up. The app shows a "Demo" badge while this is active (see AppShell).
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";

interface Song { id: string; title: string; artists: string[]; durationMs: number; hue: number }
type Args = Record<string, unknown>;

const NAMES = [
  ["Neon Harbour", "Paper Tigers"], ["Slow Orbit", "Night Shift Club"], ["Glass Rivers", "Mira Vale"], ["Midnight Tram", "Kite & Co"],
  ["Blue Static", "Ollie Strand"], ["Paper Moon", "Mira Vale"], ["Afterglow Drive", "Night Shift Club"], ["Cold Coffee", "Paper Tigers"],
  ["Violet Hour", "Juno Park"], ["Low Tide", "Kite & Co"], ["Satellite Sleep", "Ollie Strand"], ["Rooftop Rain", "Juno Park"],
  ["Daydream Index", "Mira Vale"], ["Amber Signal", "Paper Tigers"], ["Quiet Engine", "Night Shift Club"], ["Last Train Home", "Kite & Co"],
];
const pad = (n: number) => `demo${String(n).padStart(7, "0")}`;
const song = (n: number): Song => {
  const [title, artist] = NAMES[n % NAMES.length];
  const round = Math.floor(n / NAMES.length);
  return { id: pad(n), title: round ? `${title} (${round + 1})` : title, artists: [artist], durationMs: (150 + ((n * 37) % 90)) * 1000, hue: (n * 47) % 360 };
};
const CATALOG = Array.from({ length: NAMES.length }, (_, i) => song(i));

const cover = (s: Song) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 120 120"><defs><linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="hsl(${s.hue} 70% 55%)"/><stop offset="1" stop-color="hsl(${(s.hue + 60) % 360} 60% 25%)"/></linearGradient></defs><rect width="120" height="120" fill="url(#g)"/><text x="60" y="76" font-size="46" text-anchor="middle" fill="rgba(255,255,255,.75)" font-family="sans-serif">♪</text></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
};
const hit = (s: Song) => ({
  source: "YouTube Music" as const, provider: "YouTube", providerTrackId: s.id, canonicalUrl: `https://www.youtube.com/watch?v=${s.id}`,
  title: s.title, artists: s.artists, artistRefs: s.artists.map((name) => ({ provider: "YouTube", id: `demo-${name.replace(/\W+/g, "-").toLowerCase()}`, name })), artworkUrl: cover(s), durationMs: s.durationMs,
});
/** A wide, photo-like picture for the artist header. */
const portrait = (hue: number) => {
  const svg = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1200 800"><defs><radialGradient id="a" cx=".5" cy=".35" r=".8"><stop offset="0" stop-color="hsl(${hue} 75% 62%)"/><stop offset="1" stop-color="hsl(${(hue + 80) % 360} 55% 18%)"/></radialGradient></defs><rect width="1200" height="800" fill="url(#a)"/><circle cx="600" cy="300" r="150" fill="rgba(255,255,255,.18)"/><ellipse cx="600" cy="760" rx="300" ry="260" fill="rgba(255,255,255,.14)"/></svg>`;
  return `data:image/svg+xml;utf8,${encodeURIComponent(svg)}`;
};
const offline = (s: Song) => ({ provider: "YouTube", id: s.id, title: s.title, artists: s.artists, artworkUrl: cover(s), durationMs: s.durationMs });
const fromArgs = (a: Args): Song => {
  const known = CATALOG.find((s) => s.id === a.providerTrackId);
  if (known) return known;
  const n = Number(String(a.providerTrackId ?? "").replace(/\D/g, "")) || 0;
  return { ...song(n), id: String(a.providerTrackId), title: String(a.title ?? "Demo song"), artists: (a.artists as string[] | undefined) ?? ["Demo artist"] };
};

// --- generated sound -------------------------------------------------------------------------------------------
const SCALE = [0, 3, 5, 7, 10, 12, 15, 17];
class Synth {
  private ctx: AudioContext | null = null;
  private master: GainNode | null = null;
  private timer: number | null = null;
  private step = 0;
  private volume = 1;
  start(seed: number) {
    this.stop();
    try {
      this.ctx ??= new AudioContext();
      void this.ctx.resume();
      this.master ??= this.ctx.createGain();
      this.master.gain.value = this.volume * 0.4;
      this.master.connect(this.ctx.destination);
    } catch { return; }
    const base = 196 * 2 ** ((seed % 7) / 12);
    this.timer = window.setInterval(() => {
      const ctx = this.ctx, out = this.master;
      if (!ctx || !out) return;
      const note = SCALE[(this.step * 3 + seed) % SCALE.length];
      const osc = ctx.createOscillator(), env = ctx.createGain();
      osc.type = this.step % 4 === 0 ? "triangle" : "sine";
      osc.frequency.value = base * 2 ** (note / 12);
      env.gain.setValueAtTime(0.0001, ctx.currentTime);
      env.gain.exponentialRampToValueAtTime(0.5, ctx.currentTime + 0.03);
      env.gain.exponentialRampToValueAtTime(0.0001, ctx.currentTime + 0.42);
      osc.connect(env).connect(out);
      osc.start();
      osc.stop(ctx.currentTime + 0.45);
      this.step += 1;
    }, 320);
  }
  stop() { if (this.timer !== null) window.clearInterval(this.timer); this.timer = null; }
  setVolume(v: number) { this.volume = Math.max(0, Math.min(1, v / 100)); if (this.master) this.master.gain.value = this.volume * 0.4; }
}

// --- player state ----------------------------------------------------------------------------------------------
type Name = "idle" | "ready" | "playing" | "paused" | "ended";
const synth = new Synth();
const demo = {
  state: "idle" as Name, revision: 0, current: null as Song | null, posMs: 0, volume: 100,
  queue: [] as Song[], back: [] as Song[], repeat: "off" as "off" | "all" | "one", radio: true,
  likes: [] as Song[], history: [] as Song[], nextN: NAMES.length,
};
const playerDto = () => ({ version: 1, state: demo.state, revision: demo.revision });
const progressDto = () => ({
  autoplay: { state: demo.queue.length ? "ready" : "idle", nextTitle: demo.queue[0]?.title ?? null, queue: demo.queue.map(offline), repeat: demo.repeat, radio: demo.radio },
  player: playerDto(),
  track: demo.current ? hit(demo.current) : null,
  positionMs: demo.posMs, durationMs: demo.current?.durationMs ?? 0, available: true, volume: demo.volume,
});
const push = () => { void emit("core://player/progress", progressDto()); };
const setState = (state: Name) => { demo.state = state; demo.revision += 1; };
const more = (count: number) => { const out: Song[] = []; for (let i = 0; i < count; i += 1) out.push(song(demo.nextN++)); return out; };

function load(track: Song, following: Song[], playing: boolean) {
  if (demo.current) demo.back.push(demo.current);
  demo.current = track;
  demo.posMs = 0;
  demo.queue = following.length ? following : more(12);
  demo.history = [track, ...demo.history.filter((s) => s.id !== track.id)].slice(0, 30);
  setState(playing ? "playing" : "ready");
  if (playing) synth.start(Number(track.id.slice(-3)) || track.hue);
}
function advance(forward: boolean) {
  if (!forward) {
    const previous = demo.back.pop();
    if (previous) { demo.current = previous; demo.posMs = 0; setState("playing"); synth.start(previous.hue); }
    else { demo.posMs = 0; }
    return;
  }
  if (demo.queue.length < 3) demo.queue.push(...more(10));
  const next = demo.queue.shift();
  if (next) { if (demo.current) demo.back.push(demo.current); demo.current = next; demo.posMs = 0; setState("playing"); synth.start(next.hue); }
}
function startClock() {
  let last = Date.now();
  window.setInterval(() => {
  const now = Date.now(), dt = now - last;
  last = now;
  if (demo.state !== "playing" || !demo.current) return;
  demo.posMs += dt;
  if (demo.posMs >= demo.current.durationMs) {
    if (demo.repeat === "one") demo.posMs = 0;
    else if (demo.queue.length || demo.radio) advance(true);
    else { setState("ended"); synth.stop(); }
  }
  push();
  }, 500);
}

const lyrics = (s: Song) => Array.from({ length: Math.floor(s.durationMs / 5000) }, (_, i) => ({ startMs: i * 5000, text: i % 4 === 3 ? "♪ ♪ ♪" : `Demo line ${i + 1} of ${s.title}` }));
const trackInfo = (id: string) => {
  const s = CATALOG.find((x) => x.id === id) ?? { ...song(Number(id.replace(/\D/g, "")) || 0), id };
  const lines = lyrics(s);
  const text = lines.map((l) => l.text).join("\n");
  return {
    providerTrackId: s.id, title: s.title, artists: s.artists, artworkUrl: cover(s), lyrics: text, lyricsSource: "Demo", lyricsProvider: "LRCLIB",
    lyricsStatus: "available", lyricsLines: lines, lyricsSynced: true, album: { id: `demo-album-${s.id}`, name: `${s.title} EP` },
    lyricsSources: [{ id: "lrclib", label: "Demo lyrics", available: true, text, lines, synced: true, attribution: "Generated sample text", status: "available" }],
  };
};
const AUDIO = {
  settings: { quality: "high", pcmFormat: "float32", sampleRate: "auto", loudness: "off", effects: { enabled: true, preamp: 0, bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bass: 0, treble: 0, stereoWidth: 100, crossfeed: 0, reverb: "off", compressor: "off", speed: 100, keepPitch: true, spatial: "off" } },
  available: true, output: { driver: "demo", format: null, sampleRate: null, sourceFormat: null, sourceSampleRate: null },
};
const sections = () => [
  { title: "Demo · More from your listening", tracks: CATALOG.slice(0, 10).map(hit) },
  { title: "Demo · Quick picks", tracks: CATALOG.slice(6, 16).map(hit) },
  { title: "Demo · New this week", tracks: [...CATALOG.slice(10), ...CATALOG.slice(0, 4)].map(hit), albums: CATALOG.slice(0, 6).map((s) => ({ id: s.id, name: `${s.title} EP`, year: 2026, artists: s.artists, artworkUrl: cover(s) })) },
];

function handle(cmd: string, a: Args): unknown {
  switch (cmd) {
    case "discovery_feed": return sections();
    case "discovery_genres": return [];
    case "providers_list": return [{ provider: "YouTube", search: "Supported", playback: "Supported", externalPlayback: "Supported" }];
    case "search_suggestions": return [];
    case "search_cancel": return null;
    case "search_query": {
      const q = String((a.args as { query?: string } | undefined)?.query ?? "").toLowerCase();
      const hits = [...CATALOG, ...more(0)].filter((s) => `${s.title} ${s.artists.join(" ")}`.toLowerCase().includes(q)).map(hit);
      return { query: q, groups: [{ kind: "ok", provider: "YouTube", hits }], hits };
    }
    case "library_likes": return demo.likes.map(hit);
    case "library_set_like": {
      const t = a.track as { providerTrackId: string }; const s = fromArgs(t as unknown as Args);
      demo.likes = a.liked ? [s, ...demo.likes.filter((x) => x.id !== s.id)] : demo.likes.filter((x) => x.id !== s.id);
      return demo.likes.map(hit);
    }
    case "library_history": return demo.history.map(hit);
    case "library_clear_history": demo.history = []; return [];
    case "library_artist": return { provider: "YouTube", id: String(a.id), name: String(a.id).replace(/^demo-/, "").replace(/-/g, " ").replace(/\b\w/g, (c) => c.toUpperCase()), description: "Made-up artist for the browser demo. Every name, song and line here is invented.", artworkUrl: portrait(String(a.id).length * 37), subscribers: 4610, tracks: CATALOG.slice(0, 8).map(hit), albums: [] };
    case "library_album": return { provider: "YouTube", id: String(a.id), name: "Demo album", kind: "Album", year: 2026, artists: [{ provider: "YouTube", id: null, name: "Demo artist" }], artworkUrl: cover(CATALOG[1]), tracks: CATALOG.slice(0, 8).map(hit) };
    case "youtube_connection_status": return "disconnected";
    case "sync_status": return { state: "signedOut", pendingWrites: 0, conflicts: [] };
    case "sync_list_local_playlists": return [];
    case "lyrics_settings_get": return { order: ["lrclib"], disabled: [], sources: [{ id: "lrclib", label: "Demo lyrics" }] };
    case "offline_status": return { keeping: false, running: false, changed: false, tracks: [], usedBytes: 0, pinnedBytes: 0, limitBytes: 0 };
    case "telemetry_get": return { enabled: false, installId: "demo", pending: {} };
    case "telemetry_record": return null;
    case "lrc_load": case "lrc_list": return null;
    // player
    case "player_state": return playerDto();
    case "player_progress": return progressDto();
    case "player_load": {
      const args = a.args as Args & { following?: Args[]; queueSelection?: boolean };
      const track = fromArgs(args);
      const following = (args.following ?? []).map(fromArgs);
      if (args.queueSelection) { const i = demo.queue.findIndex((s) => s.id === track.id); load(track, i >= 0 ? demo.queue.slice(i + 1) : following, false); }
      else load(track, following, false);
      push();
      return playerDto();
    }
    case "player_play": case "player_resume": if (demo.current) { setState("playing"); synth.start(demo.current.hue); } push(); return playerDto();
    case "player_pause": setState("paused"); synth.stop(); push(); return playerDto();
    case "player_stop": setState("idle"); synth.stop(); demo.current = null; demo.posMs = 0; push(); return playerDto();
    case "player_seek": demo.posMs = Number(a.positionMs) || 0; push(); return playerDto();
    case "player_set_volume": demo.volume = Number(a.volume) || 0; synth.setVolume(demo.volume); push(); return playerDto();
    case "player_skip": advance(Boolean(a.forward)); push(); return null;
    case "player_set_mode": { const m = a.args as { repeat: "off" | "all" | "one"; autoplay: boolean }; demo.repeat = m.repeat; demo.radio = m.autoplay; push(); return null; }
    case "player_queue_edit": {
      const e = a.args as { op: string; id?: string; from?: number; to?: number; track?: Args };
      if (e.op === "remove") demo.queue = demo.queue.filter((s) => s.id !== e.id);
      else if (e.op === "shuffle") demo.queue = [...demo.queue].sort(() => Math.random() - 0.5);
      else if (e.op === "move" && e.from !== undefined && e.to !== undefined) { const [s] = demo.queue.splice(e.from, 1); if (s) demo.queue.splice(e.to, 0, s); }
      else if (e.op === "next" && e.track) { const s = fromArgs({ providerTrackId: e.track.id, title: e.track.title, artists: e.track.artists }); demo.queue = [s, ...demo.queue.filter((x) => x.id !== s.id)]; }
      push();
      return null;
    }
    case "player_queue_more": { const add = more(10); demo.queue.push(...add); push(); return new Promise((r) => window.setTimeout(() => r(add.length), 250)); }
    case "player_track_info": return trackInfo(String(a.providerTrackId));
    case "player_audio_settings": return AUDIO;
    case "player_set_audio_settings": return { ...AUDIO, settings: a.settings };
    case "player_effect_presets": return [];
    case "player_open_external": return null;
    default:
      if (cmd.startsWith("plugin:")) return null;
      throw { category: "unsupportedCapability", message: "not part of the browser demo" };
  }
}

export function installBrowserDemo(): boolean {
  if (typeof window === "undefined" || "__TAURI_INTERNALS__" in window) return false;
  mockIPC((cmd, payload) => handle(cmd, (payload ?? {}) as Args), { shouldMockEvents: true });
  startClock();
  (window as unknown as { __lunorDemo?: boolean }).__lunorDemo = true;
  return true;
}
