"use client";
import { create } from "zustand";
import { track } from "./telemetry";
import { togglePlayback, usePlayback } from "./playback";

/** Pauses playback once after `minutes`. Held in memory only: closing the app cancels it. */
export const useSleepTimer = create<{ endsAt: number | null }>(() => ({ endsAt: null }));
let handle: ReturnType<typeof setTimeout> | undefined;

export function setSleepTimer(minutes: number | null) {
  clearTimeout(handle);
  if (!minutes) { useSleepTimer.setState({ endsAt: null }); return; }
  track("feature.sleep-timer");
  useSleepTimer.setState({ endsAt: Date.now() + minutes * 60_000 });
  handle = setTimeout(() => {
    useSleepTimer.setState({ endsAt: null });
    const state = usePlayback.getState().player.state;
    if (state === "playing" || state === "buffering") void togglePlayback();
  }, minutes * 60_000);
}
