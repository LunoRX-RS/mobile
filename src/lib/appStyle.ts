"use client";
import { useSyncExternalStore } from "react";
import { BUILTIN_FONTS, familyOf, onCustomFontsChanged } from "./textStyle";

// The app-wide font and size. The font goes into --font-sans and --font-display (every screen reads them);
// the size is the webview's zoom, so text, spacing and icons grow together and the layout reflows to the new width.
const KEY = "lunorx.appStyle";
export const SCALE_MIN = 0.8;
export const SCALE_MAX = 1.6;
export const SCALE_STEP = 0.05;
export interface AppStyle { font: string; scale: number }
const SYSTEM_STACK = '-apple-system, BlinkMacSystemFont, "SF Pro Text", Inter, system-ui, sans-serif';
const DEFAULT: AppStyle = { font: "default", scale: 1 };

const clampScale = (n: unknown): number => {
  const v = typeof n === "number" && Number.isFinite(n) ? n : 1;
  return Math.round(Math.min(SCALE_MAX, Math.max(SCALE_MIN, v)) / SCALE_STEP) * SCALE_STEP;
};
const clean = (v: unknown): AppStyle => {
  const o = (v ?? {}) as Partial<AppStyle>;
  return { font: typeof o.font === "string" && o.font.length < 80 ? o.font : "default", scale: clampScale(o.scale) };
};
const read = (): AppStyle => {
  try { return clean(JSON.parse(window.localStorage.getItem(KEY) ?? "{}")); } catch { return DEFAULT; }
};

let state: AppStyle = typeof window === "undefined" ? DEFAULT : read();
const listeners = new Set<() => void>();

function applyFont() {
  if (typeof document === "undefined") return;
  const css = familyOf(state.font, SYSTEM_STACK);
  const root = document.documentElement.style;
  if (css) { root.setProperty("--font-sans", css); root.setProperty("--font-display", css); }
  else { root.removeProperty("--font-sans"); root.removeProperty("--font-display"); }
}
let applied = 0;
async function applyScale() {
  const ticket = ++applied;
  const f = state.scale;
  try {
    const { getCurrentWebview } = await import("@tauri-apps/api/webview");
    await getCurrentWebview().setZoom(f);
    document.documentElement.style.removeProperty("zoom");
  } catch {
    // Not inside the native app (browser preview): CSS zoom is the closest stand-in.
    if (ticket === applied) document.documentElement.style.setProperty("zoom", String(f));
  }
}
const save = () => { try { window.localStorage.setItem(KEY, JSON.stringify(state)); } catch { /* lasts for this session */ } };
const emit = () => listeners.forEach((l) => l());

export function setAppFont(font: string) { state = clean({ ...state, font }); save(); applyFont(); emit(); }
export function setAppScale(scale: number) { state = clean({ ...state, scale }); save(); void applyScale(); emit(); }
export const resetAppScale = () => setAppScale(1);
export const stepAppScale = (direction: 1 | -1) => setAppScale(state.scale + direction * SCALE_STEP);
/** Applies the saved font and size. Call once on startup. */
export function initAppStyle() { applyFont(); if (state.scale !== 1) void applyScale(); }
/** After the saved custom fonts are registered: a font that is gone falls back to the default. */
export function refreshAppStyleFonts(available: string[]) {
  if (state.font.startsWith("custom:") && !available.includes(state.font.slice(7))) { state = { ...state, font: "default" }; save(); emit(); }
  applyFont();
}

onCustomFontsChanged((available) => refreshAppStyleFonts(available));

const subscribe = (cb: () => void) => { listeners.add(cb); return () => { listeners.delete(cb); }; };
export const useAppStyle = (): AppStyle => useSyncExternalStore(subscribe, () => state, () => DEFAULT);
export { BUILTIN_FONTS };
