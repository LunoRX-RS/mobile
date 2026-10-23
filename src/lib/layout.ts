"use client";

import { useSyncExternalStore } from "react";

// phone: bottom tab bar, one column. tablet: icon rail (iPad portrait, Android tablets). desktop: full sidebar
// (iPad landscape and wider). The rail reuses the sidebar's collapsed mode.
export type Layout = "phone" | "tablet" | "desktop";

const PHONE = "(max-width: 699px)";
const DESKTOP = "(min-width: 1100px)";

function read(): Layout {
  if (typeof window === "undefined") return "desktop";
  if (window.matchMedia(PHONE).matches) return "phone";
  return window.matchMedia(DESKTOP).matches ? "desktop" : "tablet";
}

function subscribe(onChange: () => void): () => void {
  const queries = [window.matchMedia(PHONE), window.matchMedia(DESKTOP)];
  queries.forEach((q) => q.addEventListener("change", onChange));
  return () => queries.forEach((q) => q.removeEventListener("change", onChange));
}

export function useLayout(): Layout {
  return useSyncExternalStore(subscribe, read, () => "desktop");
}

/** True on iPhone, iPad and Android. iPadOS reports a Mac user agent, so touch points tell it apart from a real Mac. */
export function isMobileOS(): boolean {
  if (typeof navigator === "undefined") return false;
  return /iPhone|iPad|iPod|Android/i.test(navigator.userAgent) || (/Macintosh/i.test(navigator.userAgent) && navigator.maxTouchPoints > 1);
}

// Settings that need a desktop: Discord's local socket, system and imported fonts, the webview zoom, keyboard shortcuts.
export const DESKTOP_ONLY_GROUPS = new Set(["settings-discord", "settings-appstyle", "settings-fonts"]);
export const DESKTOP_ONLY_CATEGORIES = new Set(["shortcuts"]);
