export type Tone = "slate" | "violet" | "amber" | "rose" | "moss" | "ink";

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
