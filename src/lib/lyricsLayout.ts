export type LyricsLayout = "split" | "full";
export const LYRICS_LAYOUT_KEY = "lunorx.lyricsLayout";

/** Remembered per viewer. Full window is the default. */
export function readLyricsLayout(): LyricsLayout {
  try { return window.localStorage.getItem(LYRICS_LAYOUT_KEY) === "split" ? "split" : "full"; } catch { return "full"; }
}

export function writeLyricsLayout(layout: LyricsLayout): void {
  try { window.localStorage.setItem(LYRICS_LAYOUT_KEY, layout); } catch { /* storage unavailable: the choice lasts for this session */ }
}
