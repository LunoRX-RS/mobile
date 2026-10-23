import { invoke } from "@tauri-apps/api/core";

// Mirrors commands/telemetry.rs. Opt-in and anonymous: counter names and counts only, never titles, searches or accounts.
export interface TelemetryViewDto { enabled: boolean; installId: string; pending: Record<string, number> }

export const telemetryGet = () => invoke<TelemetryViewDto>("telemetry_get");
export const telemetrySet = (enabled: boolean) => invoke<TelemetryViewDto>("telemetry_set", { enabled });
export const telemetryRecord = (name: string) => invoke<void>("telemetry_record", { name });
export const telemetryFlush = () => invoke<void>("telemetry_flush");
/** Deletes this install's data on the server, turns telemetry off and issues a new anonymous id. */
export const telemetryDelete = () => invoke<TelemetryViewDto>("telemetry_delete");
