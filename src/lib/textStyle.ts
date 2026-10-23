import { useSyncExternalStore } from "react";

/** Text zones that can carry their own font and motion. Add a zone here and read it with `useZoneStyle`. */
export type Zone = "lyrics" | "title";
export type Effect = "none" | "blur" | "typewriter" | "scramble" | "shiny" | "wave" | "fadeUp" | "fadeDown" | "slideLeft" | "slideRight" | "zoomIn" | "zoomOut" | "pop" | "drop" | "rise" | "flipX" | "flipY" | "spin" | "blurChar" | "fade" | "roll" | "spread" | "squeeze" | "focus" | "wipe" | "wipeUp" | "curtain" | "bounce" | "float" | "swing" | "colorWave" | "pulse" | "breathe" | "rainbow" | "flow" | "neon" | "glitch" | "flicker" | "shake" | "underline" | "outline";
export interface ZoneStyle { font: string; effect: Effect }

export const ZONES: { id: Zone; label: string; hint: string }[] = [
  { id: "lyrics", label: "Lyrics", hint: "The line being sung in Now Playing. Lines with real word timing keep their own word highlight." },
  { id: "title", label: "Song title", hint: "The title in Now Playing." },
];
export type EffectUnit = "none" | "word" | "char" | "whole" | "js";
/** `unit` is what gets split for the motion; `loop` effects repeat and pause while playback is paused. CSS lives under `text-style effects` in globals.css. */
export const EFFECTS: { id: Effect; label: string; unit: EffectUnit; group: "Basic" | "Entrance" | "Loop"; loop: boolean }[] = [
  { id: "none", label: "None", unit: "none", group: "Basic", loop: false },
  { id: "blur", label: "Blur in, word by word", unit: "word", group: "Entrance", loop: false },
  { id: "typewriter", label: "Typewriter", unit: "char", group: "Entrance", loop: false },
  { id: "scramble", label: "Scramble", unit: "js", group: "Entrance", loop: false },
  { id: "shiny", label: "Shiny sweep", unit: "whole", group: "Loop", loop: true },
  { id: "wave", label: "Wave", unit: "char", group: "Loop", loop: true },
  { id: "fadeUp", label: "Fade up", unit: "word", group: "Entrance", loop: false },
  { id: "fadeDown", label: "Fade down", unit: "word", group: "Entrance", loop: false },
  { id: "slideLeft", label: "Slide from left", unit: "word", group: "Entrance", loop: false },
  { id: "slideRight", label: "Slide from right", unit: "word", group: "Entrance", loop: false },
  { id: "zoomIn", label: "Zoom in", unit: "word", group: "Entrance", loop: false },
  { id: "zoomOut", label: "Zoom out", unit: "word", group: "Entrance", loop: false },
  { id: "pop", label: "Pop", unit: "char", group: "Entrance", loop: false },
  { id: "drop", label: "Drop and bounce", unit: "char", group: "Entrance", loop: false },
  { id: "rise", label: "Rise", unit: "char", group: "Entrance", loop: false },
  { id: "flipX", label: "Flip (horizontal axis)", unit: "char", group: "Entrance", loop: false },
  { id: "flipY", label: "Flip (vertical axis)", unit: "char", group: "Entrance", loop: false },
  { id: "spin", label: "Spin in", unit: "char", group: "Entrance", loop: false },
  { id: "blurChar", label: "Blur in, letter by letter", unit: "char", group: "Entrance", loop: false },
  { id: "fade", label: "Fade, letter by letter", unit: "char", group: "Entrance", loop: false },
  { id: "roll", label: "Roll up", unit: "char", group: "Entrance", loop: false },
  { id: "spread", label: "Spread out", unit: "whole", group: "Entrance", loop: false },
  { id: "squeeze", label: "Squeeze in", unit: "whole", group: "Entrance", loop: false },
  { id: "focus", label: "Focus", unit: "whole", group: "Entrance", loop: false },
  { id: "wipe", label: "Wipe right", unit: "whole", group: "Entrance", loop: false },
  { id: "wipeUp", label: "Wipe up", unit: "whole", group: "Entrance", loop: false },
  { id: "curtain", label: "Curtain open", unit: "whole", group: "Entrance", loop: false },
  { id: "bounce", label: "Bounce", unit: "char", group: "Loop", loop: true },
  { id: "float", label: "Float", unit: "char", group: "Loop", loop: true },
  { id: "swing", label: "Swing", unit: "char", group: "Loop", loop: true },
  { id: "colorWave", label: "Colour wave", unit: "char", group: "Loop", loop: true },
  { id: "pulse", label: "Pulse", unit: "whole", group: "Loop", loop: true },
  { id: "breathe", label: "Breathe", unit: "whole", group: "Loop", loop: true },
  { id: "rainbow", label: "Rainbow", unit: "whole", group: "Loop", loop: true },
  { id: "flow", label: "Gradient flow", unit: "whole", group: "Loop", loop: true },
  { id: "neon", label: "Neon glow", unit: "whole", group: "Loop", loop: true },
  { id: "glitch", label: "Glitch", unit: "whole", group: "Loop", loop: true },
  { id: "flicker", label: "Flicker", unit: "whole", group: "Loop", loop: true },
  { id: "shake", label: "Shake", unit: "whole", group: "Loop", loop: true },
  { id: "underline", label: "Underline sweep", unit: "whole", group: "Loop", loop: true },
  { id: "outline", label: "Outline fill", unit: "whole", group: "Loop", loop: true },
];
export const EFFECT_META = new Map(EFFECTS.map((e) => [e.id, e]));
/** Built-in choices are system font stacks, so nothing is downloaded or shipped. Imported files are listed after them. */
export const BUILTIN_FONTS: { id: string; label: string; css: string | null }[] = [
  { id: "default", label: "LUNOR default", css: null },
  { id: "serif", label: "Serif", css: 'ui-serif, "New York", Georgia, Thonburi, serif' },
  { id: "rounded", label: "Rounded", css: 'ui-rounded, "SF Pro Rounded", "Arial Rounded MT Bold", system-ui, sans-serif' },
  { id: "mono", label: "Monospace", css: 'ui-monospace, "SF Mono", Menlo, monospace' },
  { id: "condensed", label: "Condensed", css: '"Avenir Next Condensed", "Arial Narrow", system-ui, sans-serif' },
];

const KEY = "lunorx.textStyle";
const DEFAULT: ZoneStyle = { font: "default", effect: "none" };
type State = Record<Zone, ZoneStyle>;
const effectIds = new Set<string>(EFFECTS.map((e) => e.id));

const clean = (v: unknown): ZoneStyle => {
  const o = (v ?? {}) as Partial<ZoneStyle>;
  return { font: typeof o.font === "string" && o.font.length < 80 ? o.font : "default", effect: effectIds.has(o.effect as string) ? (o.effect as Effect) : "none" };
};
const read = (): State => {
  try {
    const v = JSON.parse(window.localStorage.getItem(KEY) ?? "{}");
    return { lyrics: clean(v.lyrics), title: clean(v.title) };
  } catch { return { lyrics: DEFAULT, title: DEFAULT }; }
};

let state: State = typeof window === "undefined" ? { lyrics: DEFAULT, title: DEFAULT } : read();
const listeners = new Set<() => void>();
const SERVER: State = { lyrics: DEFAULT, title: DEFAULT };

/** `fallback` follows the chosen font in the stack. The app-wide font passes a literal stack, since it replaces --font-display itself. */
export const familyOf = (font: string, fallback = "var(--font-display)"): string | null => {
  if (font.startsWith("custom:")) return `"lx-custom-${font.slice(7)}", ${fallback}`;
  // A font already installed on this computer, used by its family name.
  if (font.startsWith("system:")) { const name = font.slice(7); return /^[^"'\\;{}<>,\u0000-\u001f]{1,80}$/.test(name) ? `"${name}", ${fallback}` : null; }
  return BUILTIN_FONTS.find((f) => f.id === font)?.css ?? null;
};
/** Pushes each zone's font into a CSS variable that globals.css reads (`--lx-font-<zone>`). */
function apply() {
  if (typeof document === "undefined") return;
  for (const zone of Object.keys(state) as Zone[]) {
    const css = familyOf(state[zone].font);
    if (css) document.documentElement.style.setProperty(`--lx-font-${zone}`, css);
    else document.documentElement.style.removeProperty(`--lx-font-${zone}`);
  }
}
apply();

export function setZoneStyle(zone: Zone, patch: Partial<ZoneStyle>) {
  state = { ...state, [zone]: clean({ ...state[zone], ...patch }) };
  try { window.localStorage.setItem(KEY, JSON.stringify(state)); } catch { /* storage unavailable: lasts for this session */ }
  apply();
  listeners.forEach((l) => l());
}
/** Re-applies fonts after the saved custom fonts are registered, and falls back to default for one that is gone. */
export function refreshFonts(available: string[]) {
  let changed = false;
  for (const zone of Object.keys(state) as Zone[]) {
    const f = state[zone].font;
    if (f.startsWith("custom:") && !available.includes(f.slice(7))) { state = { ...state, [zone]: { ...state[zone], font: "default" } }; changed = true; }
  }
  if (changed) { try { window.localStorage.setItem(KEY, JSON.stringify(state)); } catch { /* ignore */ } listeners.forEach((l) => l()); }
  apply();
}

const subscribe = (cb: () => void) => { listeners.add(cb); return () => { listeners.delete(cb); }; };
// Set by appStyle.ts, which cannot be imported here without a cycle.
let appFontHook: ((available: string[]) => void) | null = null;
export const onCustomFontsChanged = (fn: (available: string[]) => void) => { appFontHook = fn; };
export const refreshAppFont = (available: string[]) => appFontHook?.(available);
// ---- Fonts installed on this computer, listed by the native side (no file is read or copied). ----
let installedCache: string[] | null = null;
let installedLoading: Promise<void> | null = null;
const installedListeners = new Set<() => void>();
/** The installed font families, loaded once on first use. Empty until then, and if the system cannot list them. */
export function useInstalledFonts(): string[] {
  const list = useSyncExternalStore((cb) => { installedListeners.add(cb); return () => { installedListeners.delete(cb); }; }, () => installedCache ?? EMPTY, () => EMPTY);
  if (installedCache === null && installedLoading === null && typeof window !== "undefined") {
    installedLoading = import("@tauri-apps/api/core").then(({ invoke }) => invoke<string[]>("fonts_installed")).then((l) => { installedCache = Array.isArray(l) ? l : []; }, () => { installedCache = []; }).then(() => installedListeners.forEach((l) => l()));
  }
  return list;
}
const EMPTY: string[] = [];

export const useZoneStyle = (zone: Zone): ZoneStyle => useSyncExternalStore(subscribe, () => state[zone], () => SERVER[zone]);

// ---- Imported fonts: stored in IndexedDB on this device, registered with FontFace (no network, no CSP change). ----
const DB = "lunorx-fonts";
const MAX_BYTES = 8 * 1024 * 1024;
const faces = new Map<string, FontFace>();

const open = () => new Promise<IDBDatabase>((resolve, reject) => {
  const req = indexedDB.open(DB, 1);
  req.onupgradeneeded = () => req.result.createObjectStore("fonts", { keyPath: "slug" });
  req.onsuccess = () => resolve(req.result);
  req.onerror = () => reject(req.error);
});
const tx = async <T,>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> => {
  const db = await open();
  return new Promise<T>((resolve, reject) => {
    const req = run(db.transaction("fonts", mode).objectStore("fonts"));
    req.onsuccess = () => resolve(req.result);
    req.onerror = () => reject(req.error);
  });
};
interface Stored { slug: string; label: string; data: ArrayBuffer }
const register = async (f: Stored) => {
  const face = new FontFace(`lx-custom-${f.slug}`, f.data);
  await face.load();
  document.fonts.add(face);
  faces.set(f.slug, face);
};

export interface CustomFont { slug: string; label: string }
let customCache: CustomFont[] = [];
const customListeners = new Set<() => void>();
const emitCustom = () => customListeners.forEach((l) => l());
export const useCustomFonts = (): CustomFont[] => useSyncExternalStore((cb) => { customListeners.add(cb); return () => { customListeners.delete(cb); }; }, () => customCache, () => customCache);

/** Registers every saved font. Call once on startup. Unreadable fonts are skipped. */
export async function loadCustomFonts(): Promise<void> {
  try {
    const all = await tx<Stored[]>("readonly", (s) => s.getAll());
    const ok: CustomFont[] = [];
    for (const f of all) { try { await register(f); ok.push({ slug: f.slug, label: f.label }); } catch { /* corrupt file: skip */ } }
    customCache = ok;
    refreshFonts(ok.map((f) => f.slug));
    refreshAppFont(ok.map((f) => f.slug));
    emitCustom();
  } catch { /* IndexedDB unavailable: custom fonts last until reload */ }
}
/** Throws a short message the UI can show. */
export async function importFont(file: File): Promise<CustomFont> {
  if (!/\.(ttf|otf|woff2?)$/i.test(file.name)) throw new Error("Use a .ttf, .otf, .woff or .woff2 file.");
  if (file.size > MAX_BYTES) throw new Error("That font file is larger than 8 MB.");
  const label = file.name.replace(/\.[^.]+$/, "").slice(0, 40);
  const slug = `${label.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "font"}-${Date.now().toString(36)}`;
  const stored: Stored = { slug, label, data: await file.arrayBuffer() };
  try { await register(stored); } catch { throw new Error("That file is not a usable font."); }
  try { await tx("readwrite", (s) => s.put(stored)); } catch { /* not saved: usable until reload */ }
  customCache = [...customCache, { slug, label }];
  emitCustom();
  return { slug, label };
}
export async function removeFont(slug: string): Promise<void> {
  const face = faces.get(slug);
  if (face) { document.fonts.delete(face); faces.delete(slug); }
  try { await tx("readwrite", (s) => s.delete(slug)); } catch { /* ignore */ }
  customCache = customCache.filter((f) => f.slug !== slug);
  refreshFonts(customCache.map((f) => f.slug));
  refreshAppFont(customCache.map((f) => f.slug));
  emitCustom();
}
