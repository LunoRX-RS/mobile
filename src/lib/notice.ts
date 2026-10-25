// A short message for an action that failed with no screen of its own (a menu item, a button with no status line).
// One at a time, gone after a few seconds. Fixed text only: never an error's own message.
import { create } from "zustand";

export const useNotice = create<{ text: string | null }>(() => ({ text: null }));

let timer: number | undefined;
export function notify(text: string) {
  useNotice.setState({ text });
  window.clearTimeout(timer);
  timer = window.setTimeout(() => useNotice.setState({ text: null }), 6000);
}

/** Runs an action; if it rejects, shows `failure` instead of swallowing it. */
export function attempt(action: Promise<unknown>, failure: string): void {
  action.catch(() => notify(failure));
}
