"use client";

import { useEffect, useState } from "react";
import { providersList } from "../../services/tauri/search";
import type { Tone } from "../settings/rows";

export const PROVIDER_NAMES = ["Spotify", "YouTube"] as const;

// Blueprint 8.2: unknown status is never Supported. Anything not listed here shows as Unverified.
const LABELS: Record<string, string> = {
  Supported: "Supported",
  Unsupported: "Unsupported",
  RequiresAuth: "Requires authentication",
  RequiresEntitlement: "Requires subscription",
  TemporarilyUnavailable: "Temporarily unavailable",
  Unverified: "Unverified",
};

export function capabilityLabel(status: string | undefined): string {
  return status !== undefined && Object.hasOwn(LABELS, status) ? LABELS[status] : "Unverified";
}

export type ProviderLoad =
  | { state: "loading" }
  | { state: "error" }
  | { state: "ready"; searchByProvider: Record<string, string> };

// providers_list reports only search status today. Other capabilities have no command, so callers show them as not reported.
export function useProviderStatuses(): ProviderLoad {
  const [load, setLoad] = useState<ProviderLoad>({ state: "loading" });

  useEffect(() => {
    let cancelled = false;
    providersList()
      .then((rows) => {
        if (cancelled) return;
        const searchByProvider: Record<string, string> = {};
        for (const row of rows) searchByProvider[row.provider] = row.search;
        setLoad({ state: "ready", searchByProvider });
      })
      .catch(() => {
        if (!cancelled) setLoad({ state: "error" });
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return load;
}

// Shown for a provider whose search status the core did not return.
export function searchLabelFor(load: ProviderLoad, provider: string): string {
  if (load.state === "loading") return "Loading";
  if (load.state === "error") return "Not available: could not read provider status";
  return capabilityLabel(load.searchByProvider[provider]);
}

// Icon tone for a capability status. Unknown is never "ok".
export function searchTone(load: ProviderLoad, provider: string): Tone {
  if (load.state === "error") return "bad";
  if (load.state === "loading") return "off";
  const status = load.searchByProvider[provider];
  if (status === "Supported") return "ok";
  if (status === "RequiresAuth" || status === "RequiresEntitlement" || status === "TemporarilyUnavailable") return "warn";
  return "off";
}
