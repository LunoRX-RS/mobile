"use client";
import { create } from "zustand";
import { track } from "./telemetry";
import { togglePlayback, usePlayback } from "./playback";

/** Pauses playback once after `minutes`, or when the current song ends (`endOfTrack` holds that song's id). Held in memory only: closing the app cancels it. */
export const useSleepTimer = create<{ endsAt: number | null; endOfTrack: string | null }>(() => ({ endsAt: null, endOfTrack: null }));
let handle: ReturnType<typeof setTimeout> | undefined;

export function setSleepTimer(minutes: number | null) {
  clearTimeout(handle);
  if (!minutes) { useSleepTimer.setState({ endsAt: null, endOfTrack: null }); return; }
  track("feature.sleep-timer");
  useSleepTimer.setState({ endsAt: Date.now() + minutes * 60_000, endOfTrack: null });
  handle = setTimeout(() => {
    useSleepTimer.setState({ endsAt: null });
    const state = usePlayback.getState().player.state;
    if (state === "playing" || state === "buffering") void togglePlayback();
  }, minutes * 60_000);
}

/** Pauses when the song that is playing now has finished (the next one starts, or playback reports the end). */
export function setSleepEndOfTrack() {
  clearTimeout(handle);
  const playback = usePlayback.getState();
  const id = playback.track?.providerTrackId ?? playback.requested?.providerTrackId;
  if (!id) return;
  track("feature.sleep-timer");
  useSleepTimer.setState({ endsAt: null, endOfTrack: id });
}

if (typeof window !== "undefined") {
  usePlayback.subscribe((state) => {
    const id = useSleepTimer.getState().endOfTrack;
    if (!id) return;
    const current = state.track?.providerTrackId;
    if (state.player.state === "ended" || (current && current !== id)) {
      useSleepTimer.setState({ endOfTrack: null });
      void import("../services/tauri/player").then((m) => m.playerCommand("pause")).catch(() => {});
    }
  });
}
