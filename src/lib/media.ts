export type Tone = "slate" | "violet" | "amber" | "rose" | "moss" | "ink";

const TONES: Tone[] = ["slate", "violet", "amber", "rose", "moss", "ink"];

/** The cover tone for an id (FNV-1a). One function everywhere, so a playlist has the same cover in every view. */
export function toneFor(seed: string): Tone {
  let h = 2166136261;
  for (let i = 0; i < seed.length; i++) h = Math.imul(h ^ seed.charCodeAt(i), 16777619);
  return TONES[(h >>> 0) % TONES.length];
}

export interface LyricLine {
  at: number; // seconds
  text: string;
}

export type ProviderLabel = "YouTube Music" | "Spotify";

export interface QueueTrack {
  id: string;
  provider: ProviderLabel;
  title: string;
  artist: string;
  album: string;
  durationSec: number;
  tone: Tone;
  lyrics?: LyricLine[];
}

export function formatDuration(totalSec: number): string {
  const s = Math.max(0, Math.floor(totalSec));
  const m = Math.floor(s / 60);
  return `${m}:${String(s % 60).padStart(2, "0")}`;
}
