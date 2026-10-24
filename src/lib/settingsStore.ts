"use client";

import { create } from "zustand";

interface SettingsState {
  devMode: boolean;
  toggleDevMode: () => void;
}

const DEV_MODE_KEY = "lunorx.settings.dev_mode";

export const useSettingsStore = create<SettingsState>((set) => ({
  devMode: (() => {
    try {
      return localStorage.getItem(DEV_MODE_KEY) === "true";
    } catch {
      return false;
    }
  })(),
  toggleDevMode: () => set((s) => {
    const next = !s.devMode;
    try { localStorage.setItem(DEV_MODE_KEY, String(next)); } catch {}
    return { devMode: next };
  }),
}));
