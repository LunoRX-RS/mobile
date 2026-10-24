"use client";
import { playerAudioSettings, playerSetAudioSettings, subscribeAudioSettings, type AudioSettings } from "../services/tauri/player";

export const AUDIO_CHANGED = "lunorx:audio-changed";

const KEY = "lunorx.audio";

export function saveAudioSettings(settings: AudioSettings) {
  try { window.localStorage.setItem(KEY, JSON.stringify(settings)); } catch { /* the choice still applies this session */ }
}

/** Sends the saved sound settings to core once at start, since core forgets them when the app closes. A bad or old file is ignored. */
export async function applySavedAudio() {
  let saved: unknown;
  try { saved = JSON.parse(window.localStorage.getItem(KEY) ?? "null"); } catch { return; }
  if (!saved || typeof saved !== "object") return;
  try {
    const current = (await playerAudioSettings()).settings;
    await playerSetAudioSettings({ ...current, ...(saved as Partial<AudioSettings>), effects: { ...current.effects, ...((saved as Partial<AudioSettings>).effects ?? {}) } });
  } catch { /* core refused it (not available, or out of range): keep the defaults */ }
}

/** Keeps the saved sound settings in step with changes made outside the panel (the macOS Sound menu), and tells the panel. */
export function watchAudioSettings(): () => void {
  let off: (() => void) | null = null;
  let live = true;
  subscribeAudioSettings((dto) => {
    saveAudioSettings(dto.settings);
    window.dispatchEvent(new CustomEvent(AUDIO_CHANGED, { detail: dto }));
  }).then((fn) => { if (live) off = fn; else fn(); }, () => {});
  return () => { live = false; off?.(); };
}
