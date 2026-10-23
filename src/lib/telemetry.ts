"use client";
import { telemetryFlush, telemetryRecord } from "../services/tauri/telemetry";

/** Counts one use of a feature. Does nothing unless the user opted in; failures are ignored on purpose. */
export function track(name: string) {
  void telemetryRecord(name).catch(() => {});
}

/** Uploads pending counters a minute after start and then hourly. Returns the cleanup. */
export function initTelemetry(): () => void {
  const flush = () => { void telemetryFlush().catch(() => {}); };
  const first = window.setTimeout(flush, 60_000);
  const every = window.setInterval(flush, 3_600_000);
  return () => { window.clearTimeout(first); window.clearInterval(every); };
}
