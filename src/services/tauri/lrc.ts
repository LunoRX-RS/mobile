import { invoke } from "@tauri-apps/api/core";

// Mirrors commands/lrc.rs. Tauri maps the Rust `args` struct fields to these camelCase keys.
// The files are plain .lrc text written by the app on this computer. Nothing here talks to a provider or the backend.

export interface LrcLineDto {
  startMs: number;
  text: string;
}

export interface LrcLoadDto {
  exists: boolean;
  path: string | null;
  lines: LrcLineDto[];
  title: string | null;
  artists: string[];
}

export interface LrcSaveArgs {
  providerTrackId: string;
  title: string;
  artists: string[];
  durationMs: number;
  /** Sorted by startMs. An empty list deletes the file. */
  lines: LrcLineDto[];
}

export interface LrcSavedDto {
  path: string;
  count: number;
  bytes: number;
}

export interface LrcListItem {
  providerTrackId: string;
  title: string;
  artists: string[];
  lines: LrcLineDto[];
  modifiedMs: number;
}

// Rejected by invoke with this shape. Only the category is used; the message is never shown.
export type LrcError = { category: "validation" | "tooLarge" | "storageUnavailable" | "notFound" | "ioFailed" };

export function lrcLoad(providerTrackId: string): Promise<LrcLoadDto> {
  return invoke("lrc_load", { args: { providerTrackId } });
}

export function lrcSave(args: LrcSaveArgs): Promise<LrcSavedDto> {
  return invoke("lrc_save", { args });
}

export function lrcDelete(providerTrackId: string): Promise<{ deleted: boolean }> {
  return invoke("lrc_delete", { args: { providerTrackId } });
}

/** Shows the file in Finder (or the file manager). */
export function lrcReveal(providerTrackId: string): Promise<void> {
  return invoke("lrc_reveal", { args: { providerTrackId } });
}

export function lrcList(): Promise<{ items: LrcListItem[] }> {
  return invoke("lrc_list", { args: {} });
}

/** The user-facing sentence for a failed save. The lines always stay in the window. */
export function lrcErrorText(error: unknown): string {
  const category = (error as { category?: string } | null)?.category;
  const kept = "Your lines are kept in this window.";
  if (category === "storageUnavailable") return `Could not save to Documents. ${kept}`;
  if (category === "tooLarge") return `The file would be too large to save. ${kept}`;
  if (category === "validation") return `A line or the track details were not accepted. ${kept}`;
  if (category === "notFound") return `The file could not be found. ${kept}`;
  return `Writing the file failed. ${kept}`;
}
