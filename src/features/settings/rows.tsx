"use client";

import { CircleAlert, CircleCheck, CircleSlash, TriangleAlert, type LucideIcon } from "lucide-react";
import { createContext, useContext, type ReactNode } from "react";
import { Skeleton, cx } from "../../components/ui";
import type { SyncState } from "../../services/tauri/sync";

// Shared vocabulary for Settings, Connections, Accounts, Sync and Player: inset group, setting row, status row.
// Tone colours the icon only; the words carry the meaning (DESIGN.md, honest states).
export type Tone = "ok" | "off" | "warn" | "bad";

const TONES: Record<Tone, { Icon: LucideIcon; cls: string }> = {
  ok: { Icon: CircleCheck, cls: "text-synced-blue" },
  off: { Icon: CircleSlash, cls: "text-secondary-text" },
  warn: { Icon: TriangleAlert, cls: "text-caution-amber" },
  bad: { Icon: CircleAlert, cls: "text-critical-coral" },
};

// Page frame. data-pg lets globals.css cancel the legacy .lx-scope styling while a view is still wrapped in it.
export function PageRoot({ children }: { children: ReactNode }) {
  return (
    <div data-pg className="flex max-w-[760px] flex-col gap-8">
      {children}
    </div>
  );
}

// A page that already has an h2 (the Settings category pane) wraps its groups in <InsetLevel level={3}> so group titles are h3,
// including groups rendered by other components (AudioSettingsPanel, ProviderKeys).
const InsetLevelContext = createContext<2 | 3>(2);
export const InsetLevel = InsetLevelContext.Provider;

export function Inset({ id, title, children }: { id: string; title: string; children: ReactNode }) {
  const H = useContext(InsetLevelContext) === 3 ? "h3" : "h2";
  return (
    <section aria-labelledby={id} className="flex flex-col gap-1.5">
      <H id={id} tabIndex={-1} className="px-4 text-[13px] leading-[18px] font-semibold text-secondary-text">
        {title}
      </H>
      <div className="overflow-hidden rounded-[10px] bg-frost-100">{children}</div>
    </section>
  );
}

// 44px row: label (and optional hint) left, value or control right. `stack` puts the control under the label.
export function SettingRow({
  label,
  htmlFor,
  hint,
  hintId,
  stack = false,
  children,
}: {
  label: ReactNode;
  htmlFor?: string;
  hint?: ReactNode;
  hintId?: string;
  stack?: boolean;
  children?: ReactNode;
}) {
  const Label = htmlFor ? "label" : "div";
  return (
    <div
      className={cx(
        "flex min-h-11 gap-x-6 gap-y-2 border-t border-hairline px-4 py-2 first:border-t-0",
        stack ? "flex-col justify-center" : "items-center justify-between",
      )}
    >
      <div className="min-w-0">
        <Label {...(htmlFor ? { htmlFor } : {})} className="block text-[13px] leading-[18px] text-primary-text">
          {label}
        </Label>
        {hint && (
          <p id={hintId} className="text-[12px] leading-4 text-secondary-text">
            {hint}
          </p>
        )}
      </div>
      {children !== undefined && (
        <div className={cx("flex min-w-0 items-center gap-2 text-[13px] text-secondary-text", stack ? "" : "max-w-[60%] shrink-0 flex-wrap justify-end text-right")}>
          {children}
        </div>
      )}
    </div>
  );
}

export function Status({ tone, children }: { tone: Tone; children: ReactNode }) {
  const { Icon, cls } = TONES[tone];
  return (
    <span className="inline-flex items-center gap-1.5 text-primary-text">
      <Icon size={16} aria-hidden="true" className={cx("shrink-0", cls, tone === "ok" && "lx-check")} />
      <span>{children}</span>
    </span>
  );
}

export function StatusRow({
  label,
  hint,
  hintId,
  tone,
  status,
  children,
}: {
  label: ReactNode;
  hint?: ReactNode;
  hintId?: string;
  tone: Tone;
  status: ReactNode;
  children?: ReactNode;
}) {
  return (
    <SettingRow label={label} hint={hint} hintId={hintId}>
      <Status tone={tone}>{status}</Status>
      {children}
    </SettingRow>
  );
}

// A full-width message row inside a group: icon plus words, errors announced.
export function NoteRow({
  tone,
  id,
  role,
  children,
}: {
  tone: Tone;
  id?: string;
  role?: "status" | "alert";
  children: ReactNode;
}) {
  const { Icon, cls } = TONES[tone];
  return (
    <div id={id} role={role} className="lx-note flex items-start gap-3 border-t border-hairline px-4 py-3 text-[13px] leading-[18px] text-primary-text first:border-t-0">
      <Icon size={16} aria-hidden="true" className={cx("mt-0.5 shrink-0", cls, tone === "ok" && "lx-check")} />
      <div className="min-w-0">{children}</div>
    </div>
  );
}

// A one-message group for results and errors that belong to the whole page, not to one section.
export function Banner({ tone, role, children }: { tone: Tone; role?: "status" | "alert"; children: ReactNode }) {
  return (
    <div className="overflow-hidden rounded-[10px] bg-frost-100">
      <NoteRow tone={tone} role={role}>
        {children}
      </NoteRow>
    </div>
  );
}

// Loading placeholder with the same row height as a SettingRow.
export function SkeletonRows({ rows = 3 }: { rows?: number }) {
  return (
    <div aria-hidden="true">
      {Array.from({ length: rows }, (_, i) => (
        <div key={i} className="flex min-h-11 items-center justify-between gap-6 border-t border-hairline px-4 first:border-t-0">
          <Skeleton className="h-3 w-40" />
          <Skeleton className="h-3 w-24" />
        </div>
      ))}
    </div>
  );
}

// Plain-words summary of the sync engine state for status rows.
export function syncSummary(s: SyncState): { tone: Tone; text: string } {
  switch (s.state) {
    case "signedOut":
      return { tone: "off", text: "Signed out" };
    case "syncing":
      return { tone: "ok", text: "Syncing" };
    case "synced":
      return { tone: "ok", text: "Synced" };
    case "offline":
      return { tone: "warn", text: "Offline" };
    case "rateLimited":
      return { tone: "warn", text: "Sync paused: too many requests" };
    case "conflict":
      return { tone: "warn", text: "Conflict" };
    case "error":
      return { tone: "bad", text: "Sync problem" };
  }
}

export const FIELD =
  "h-8 min-w-0 rounded-[8px] bg-frost-300 px-3 text-[13px] text-primary-text placeholder:text-tertiary-text focus:bg-white/20 aria-[invalid=true]:bg-white/20 disabled:opacity-40";
