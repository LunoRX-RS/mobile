"use client";

import { useUi } from "./store";

// The phone's back button and back gesture, and the swipe gesture, all walk the same stack: first whatever is open on
// top (queue, Now Playing, search palette), then the screens the user visited. Without this the Android back button
// closes the app. The webview's own history only mirrors that depth so the system has something to go back to.

const depth = () => {
  const s = useUi.getState();
  return s.back.length + Number(s.view === "settings" && s.settingsDetail) + Number(s.npQueue) + Number(s.npPanel !== null) + Number(s.nowPlayingOpen && s.lyricsOpen) + Number(s.queueOpen) + Number(s.nowPlayingOpen) + Number(s.paletteOpen);
};

/** One step back in the app. False when there is nothing left to go back to. */
export function backStep(): boolean {
  const s = useUi.getState();
  if (s.npPanel) useUi.setState({ npPanel: null });
  else if (s.npQueue) useUi.setState({ npQueue: false });
  else if (s.nowPlayingOpen && s.lyricsOpen) s.setLyrics(false);
  else if (s.paletteOpen) s.setPalette(false);
  else if (s.queueOpen) useUi.setState({ queueOpen: false });
  else if (s.nowPlayingOpen) s.setNowPlaying(false);
  else if (s.view === "settings" && s.settingsDetail) useUi.setState({ settingsDetail: false });
  else if (s.back.length > 0) s.goBack();
  else return false;
  return true;
}

export function forwardStep(): boolean {
  const s = useUi.getState();
  if (s.forward.length === 0) return false;
  s.goForward();
  return true;
}

/** Starts the history mirror and the swipe gesture. Returns the cleanup. */
export function initBackNav(): () => void {
  let pushed = 0;
  let ignore = 0;
  const sync = () => {
    const want = depth();
    if (want > pushed) {
      for (; pushed < want; pushed++) window.history.pushState({ lunor: pushed + 1 }, "");
    } else if (want < pushed) {
      const steps = pushed - want;
      pushed = want;
      ignore += 1;
      window.history.go(-steps);
    }
  };
  const onPop = () => {
    if (ignore > 0) { ignore -= 1; return; }
    if (pushed > 0) pushed -= 1;
    backStep();
    sync();
  };
  const stop = useUi.subscribe(sync);
  window.addEventListener("popstate", onPop);
  sync();

  // Swipe right = back, swipe left = forward. Ignored on anything that scrolls or drags sideways itself.
  let start: { x: number; y: number; t: number } | null = null;
  const blocked = (el: EventTarget | null) => {
    for (let n = el as HTMLElement | null; n && n !== document.body; n = n.parentElement) {
      if (n.dataset?.noSwipe !== undefined || n.matches?.('input, textarea, select, [role="slider"], [contenteditable="true"]')) return true;
      if (n.scrollWidth > n.clientWidth + 2 && ["auto", "scroll"].includes(getComputedStyle(n).overflowX)) return true;
    }
    return false;
  };
  const down = (e: TouchEvent) => {
    const t = e.touches[0];
    start = e.touches.length === 1 && t && !blocked(e.target) ? { x: t.clientX, y: t.clientY, t: Date.now() } : null;
  };
  const up = (e: TouchEvent) => {
    const t = e.changedTouches[0];
    const from = start;
    start = null;
    if (!from || !t) return;
    const dx = t.clientX - from.x;
    const dy = t.clientY - from.y;
    if (Math.abs(dx) < 80 || Math.abs(dx) < Math.abs(dy) * 2 || Date.now() - from.t > 600) return;
    if (dx > 0) backStep(); else forwardStep();
  };
  document.addEventListener("touchstart", down, { passive: true });
  document.addEventListener("touchend", up, { passive: true });
  return () => {
    stop();
    window.removeEventListener("popstate", onPop);
    document.removeEventListener("touchstart", down);
    document.removeEventListener("touchend", up);
  };
}
