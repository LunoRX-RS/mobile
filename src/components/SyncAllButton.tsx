"use client";
import { RefreshCw } from "lucide-react";
import { syncAll, syncAllSummary, useSyncAll } from "../lib/syncAll";
import { Button, IconButton, cx } from "./ui";

/** Syncs the LunoRX account, YouTube, cloud stats and usage data with one press. `compact` is the icon-only form for the sidebar. */
export function SyncAllButton({ compact = false, showResult = false }: { compact?: boolean; showResult?: boolean }) {
  const { running, parts } = useSyncAll();
  const summary = running ? "Syncing everything…" : syncAllSummary(parts);
  const icon = <RefreshCw size={compact ? 16 : 14} aria-hidden="true" className={cx(running && "lx-spin")} />;
  return (
    <div className="flex flex-col gap-2">
      {compact
        ? <IconButton label={running ? "Syncing everything…" : "Sync everything"} disabled={running} onClick={() => void syncAll()} className="size-9">{icon}</IconButton>
        : <Button variant="secondary" disabled={running} onClick={() => void syncAll()}>{icon}{running ? "Syncing…" : "Sync all"}</Button>}
      <p role="status" className="sr-only">{summary}</p>
      {showResult && parts.length > 0 && (
        <ul className="flex flex-col gap-1 text-[13px]">
          {parts.map((p) => (
            <li key={p.key} className="flex gap-2"><span className="w-44 shrink-0 font-semibold">{p.label}</span><span className={p.state === "failed" ? "text-critical-coral" : "text-secondary-text"}>{p.detail}</span></li>
          ))}
        </ul>
      )}
    </div>
  );
}
