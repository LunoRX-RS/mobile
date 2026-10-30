"use client";
import { playerSetMode } from "../services/tauri/player";
import { notify } from "./notice";
import { usePlayback } from "./playback";

export type Repeat = "off" | "all" | "one";
const REPEAT_KEY = "lunorx.repeat";
const AUTOPLAY_KEY = "lunorx.autoplay";
const REPEATS: Repeat[] = ["off", "all", "one"];

function read(key: string): string | null { try { return window.localStorage.getItem(key); } catch { return null; } }
function write(key: string, value: string) { try { window.localStorage.setItem(key, value); } catch { /* the choice still applies this session */ } }

/** What core reports now, falling back to the defaults (repeat off, autoplay on). */
export const currentMode = () => {
  const a = usePlayback.getState().autoplay;
  return { repeat: (a.repeat ?? "off") as Repeat, autoplay: a.radio ?? true };
};

export function setPlayMode(next: { repeat?: Repeat; autoplay?: boolean }) {
  const cur = currentMode();
  const repeat = next.repeat ?? cur.repeat;
  const autoplay = next.autoplay ?? cur.autoplay;
  write(REPEAT_KEY, repeat);
  write(AUTOPLAY_KEY, String(autoplay));
  void playerSetMode(repeat, autoplay).catch(() => notify("Could not change the play mode."));
}

export const nextRepeat = (r: Repeat): Repeat => REPEATS[(REPEATS.indexOf(r) + 1) % REPEATS.length];

/** Sends the saved choices to core once at start, since core forgets them when the app closes. */
export function applySavedMode() {
  const repeat = read(REPEAT_KEY);
  const autoplay = read(AUTOPLAY_KEY);
  if (repeat === null && autoplay === null) return;
  void playerSetMode(REPEATS.includes(repeat as Repeat) ? (repeat as Repeat) : "off", autoplay !== "false").catch(() => {});
}
