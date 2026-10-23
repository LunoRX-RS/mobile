"use client";

import { useCallback, useEffect, useState } from "react";

const win = async () => (await import("@tauri-apps/api/window")).getCurrentWindow();

/**
 * Native window full screen. `supported` is false until the window API answers once, and again after any call throws
 * (no Tauri runtime, tests, a missing capability), so the control hides itself instead of failing loudly.
 */
export function useFullscreen(enabled: boolean): { supported: boolean; active: boolean; toggle: () => void } {
  const [supported, setSupported] = useState(false);
  const [active, setActive] = useState(false);
  const read = useCallback(async () => {
    try { setActive(await (await win()).isFullscreen()); setSupported(true); } catch { setSupported(false); }
  }, []);
  useEffect(() => {
    if (!enabled) return;
    void read();
    let timer = 0;
    const onResize = () => { window.clearTimeout(timer); timer = window.setTimeout(() => void read(), 200); };
    window.addEventListener("resize", onResize);
    return () => { window.clearTimeout(timer); window.removeEventListener("resize", onResize); };
  }, [enabled, read]);
  const toggle = useCallback(async () => {
    try {
      const w = await win();
      await w.setFullscreen(!(await w.isFullscreen()));
      setActive(await w.isFullscreen());
    } catch { setSupported(false); }
  }, []);
  return { supported, active, toggle: () => void toggle() };
}
