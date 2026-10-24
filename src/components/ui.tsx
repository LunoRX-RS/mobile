"use client";

import { useEffect, useId, useState, type ButtonHTMLAttributes, type ReactNode } from "react";
import { ChevronRight, CircleDot } from "lucide-react";
import { ActivePill } from "./Motion";

export function cx(...parts: (string | false | null | undefined)[]): string {
  return parts.filter(Boolean).join(" ");
}

// True after the first client render. Anything that needs a style prop (the CSP forbids style attributes in the
// static HTML) mounts only once this is true; style set from script is allowed.
export function useMounted(): boolean {
  const [m, setM] = useState(false);
  useEffect(() => setM(true), []);
  return m;
}

type ButtonVariant = "primary" | "secondary" | "ghost";

const VARIANT: Record<ButtonVariant, string> = {
  primary: "bg-primary-text text-on-ink hover:bg-ink-bright font-semibold",
  secondary: "bg-frost-200 text-primary-text hover:bg-frost-300",
  ghost: "text-secondary-text hover:bg-frost-100 hover:text-primary-text",
};

export function Button({
  variant = "secondary",
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: ButtonVariant }) {
  return (
    <button
      type="button"
      data-ln-btn=""
      {...rest}
      className={cx(
        "inline-flex h-8 items-center justify-center gap-1.5 rounded-[8px] px-3.5 text-[13px] font-medium lx-press disabled:pointer-events-none disabled:opacity-40",
        VARIANT[variant],
        className,
      )}
    >
      {children}
    </button>
  );
}

export function IconButton({
  label,
  active = false,
  className,
  children,
  ...rest
}: ButtonHTMLAttributes<HTMLButtonElement> & { label: string; active?: boolean }) {
  return (
    <button
      type="button"
      aria-label={label}
      title={label}
      aria-pressed={rest["aria-pressed"]}
      data-ln-icon=""
      {...rest}
      className={cx(
        "inline-flex size-8 shrink-0 items-center justify-center rounded-[8px] lx-press disabled:pointer-events-none disabled:opacity-40",
        active ? "text-lumen-lavender" : "text-secondary-text hover:bg-frost-100 hover:text-primary-text",
        className,
      )}
    >
      {children}
    </button>
  );
}

// Slim scrubber used for seek and volume. A transparent native range sits on top for pointer, keyboard and
// screen readers; the visible fill and knob are drawn under it and appear after mount.
export function Scrubber({
  value,
  max,
  onChange,
  onCommit,
  label,
  valueText,
  disabled,
  className,
}: {
  value: number;
  max: number;
  /** Fires on every move. Use for cheap, live values such as volume. */
  onChange?: (v: number) => void;
  /** Fires once on release (or after a key press). While dragging, the knob follows the pointer. */
  onCommit?: (v: number) => void;
  label: string;
  valueText?: string;
  disabled?: boolean;
  className?: string;
}) {
  const mounted = useMounted();
  const [drag, setDrag] = useState<number | null>(null);
  // Pointer held: the fill and knob follow the pointer with no transition. Otherwise progress eases linearly.
  const [held, setHeld] = useState(false);
  const shown = drag ?? value;
  const pct = max > 0 ? Math.min(100, Math.max(0, (shown / max) * 100)) : 0;
  const commit = () => {
    if (drag !== null) {
      onCommit?.(drag);
      setDrag(null);
    }
  };
  return (
    <div data-ln-scrubber="" className={cx("group relative flex h-6 min-w-0 touch-none items-center", className)}>
      <div aria-hidden="true" className="relative h-[3px] w-full rounded-full bg-frost-300 transition-[height] duration-150 group-hover:h-1">
        {mounted && (
          <>
            <div
              className={cx("absolute inset-y-0 left-0 rounded-full bg-secondary-text group-hover:bg-primary-text", !(held || drag !== null) && "lx-fill")}
              style={{ width: `${pct}%` }}
            />
            <div
              className={cx(
                "absolute top-1/2 size-3 -translate-x-1/2 -translate-y-1/2 rounded-full bg-primary-text opacity-0 shadow-[0_1px_3px_rgba(0,0,0,0.5)] group-focus-within:opacity-100 group-hover:opacity-100",
                held || drag !== null ? "lx-knob-drag" : "lx-knob",
              )}
              style={{ left: `${pct}%` }}
            />
          </>
        )}
      </div>
      <input
        type="range"
        aria-label={label}
        aria-valuetext={valueText}
        min={0}
        max={Math.max(max, 1)}
        step="any"
        value={shown}
        disabled={disabled}
        onChange={(e) => {
          const v = Number(e.currentTarget.value);
          if (onCommit) setDrag(v);
          onChange?.(v);
        }}
        onPointerDown={() => setHeld(true)}
        onPointerUp={() => {
          setHeld(false);
          commit();
        }}
        onPointerCancel={() => {
          setHeld(false);
          setDrag(null);
        }}
        onKeyUp={commit}
        onBlur={() => {
          setHeld(false);
          commit();
        }}
        className="absolute inset-0 size-full cursor-pointer opacity-0 disabled:cursor-not-allowed"
      />
    </div>
  );
}

// Page frame: large left-aligned title, optional quiet caption, then the content.
export function Screen({
  id,
  title,
  caption,
  actions,
  children,
}: {
  id: string;
  title: string;
  caption?: ReactNode;
  actions?: ReactNode;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={id} className="flex flex-col gap-6 md:gap-8">
      <header className="flex flex-col gap-1">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <h1 id={id} className="font-display text-[28px] leading-9 font-bold tracking-[-0.022em] md:text-[34px] md:leading-[40px]">
            {title}
          </h1>
          {actions && <div className="flex flex-wrap items-center gap-2">{actions}</div>}
        </div>
        {caption && <p className="max-w-[72ch] text-[13px] leading-[18px] text-secondary-text">{caption}</p>}
      </header>
      {children}
    </section>
  );
}

export function Section({
  id,
  title,
  action,
  rule = false,
  children,
}: {
  id: string;
  title: string;
  action?: ReactNode;
  rule?: boolean;
  children: ReactNode;
}) {
  return (
    <section aria-labelledby={id} className="flex flex-col gap-3">
      <div className={cx("flex items-center justify-between gap-3", rule && "border-b border-hairline pb-2")}>
        <h2 id={id} className="min-w-0 truncate font-display text-[19px] leading-7 font-semibold tracking-[-0.015em] md:text-[22px]">
          {title}
        </h2>
        {action && <div className="shrink-0">{action}</div>}
      </div>
      {children}
    </section>
  );
}

// "See All" style accent link.
export function SeeAll({ onClick, label }: { onClick: () => void; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      aria-label={label}
      className="inline-flex min-h-8 items-center gap-0.5 rounded-[6px] px-2 text-[13px] font-medium text-secondary-text hover:bg-frost-100 hover:text-primary-text"
    >
      See All
      <ChevronRight size={14} aria-hidden="true" />
    </button>
  );
}

// Apple Settings style group: rounded 10px, rows divided by hairlines. Used only where a form or list needs grouping.
export function Group({ id, title, children }: { id: string; title?: string; children: ReactNode }) {
  return (
    <section aria-labelledby={title ? id : undefined} className="flex flex-col gap-1.5">
      {title && (
        <h2 id={id} className="px-4 text-[13px] leading-[18px] font-semibold text-secondary-text">
          {title}
        </h2>
      )}
      <div className="overflow-hidden rounded-[10px] bg-frost-100">{children}</div>
    </section>
  );
}

export function GroupRow({ label, children, action }: { label: string; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="flex items-start justify-between gap-6 border-t border-hairline px-4 py-2.5 first:border-t-0">
      <div className="grid min-w-0 flex-1 grid-cols-[minmax(140px,220px)_minmax(0,1fr)] gap-4 text-[13px] leading-[18px]">
        <p className="text-secondary-text">{label}</p>
        <div className="min-w-0 text-primary-text">{children}</div>
      </div>
      {action}
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  children,
  action,
}: {
  icon: ReactNode;
  title: string;
  children?: ReactNode;
  action?: ReactNode;
}) {
  return (
    <div className="flex flex-col items-start gap-2 py-6">
      <span className="text-tertiary-text" aria-hidden="true">
        {icon}
      </span>
      <p className="text-[15px] leading-5 font-semibold">{title}</p>
      {children && <div className="max-w-[60ch] text-[13px] leading-[18px] text-secondary-text">{children}</div>}
      {action}
    </div>
  );
}

export function Skeleton({ className }: { className?: string }) {
  return <div aria-hidden="true" className={cx("lx-shimmer rounded-[8px] bg-frost-200", className)} />;
}

// Honest status line: icon plus words. Tone colours the icon only.
export function StatusNote({
  icon,
  tone = "neutral",
  children,
  role,
}: {
  icon?: ReactNode;
  tone?: "neutral" | "blue" | "amber" | "coral";
  children: ReactNode;
  role?: "status" | "alert";
}) {
  const iconTone = {
    neutral: "text-secondary-text",
    blue: "text-synced-blue",
    amber: "text-caution-amber",
    coral: "text-critical-coral",
  }[tone];
  return (
    <div role={role} className="lx-note flex items-start gap-3 rounded-[10px] bg-frost-100 px-4 py-3 text-[13px] leading-[18px] text-secondary-text">
      <span className={cx("mt-0.5 shrink-0", iconTone)} aria-hidden="true">
        {icon ?? <CircleDot size={16} />}
      </span>
      <div className="flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2">{children}</div>
    </div>
  );
}

// Segmented control (Apple style): a recessed track with a raised selected segment.
export function Segmented<T extends string>({
  label,
  value,
  options,
  onChange,
  selected = true,
}: {
  label: string;
  value: T;
  options: { key: T; label: string }[];
  onChange: (v: T) => void;
  selected?: boolean;
}) {
  const pill = useId();
  return (
    <div role="tablist" aria-label={label} className="inline-flex h-8 items-center gap-0.5 rounded-[9px] bg-frost-200 p-0.5">
      {options.map((o) => {
        const on = selected && value === o.key;
        return (
          <button
            key={o.key}
            type="button"
            role="tab"
            aria-selected={on}
            onClick={() => onChange(o.key)}
            className={cx(
              "relative isolate h-7 rounded-[7px] px-3.5 text-[13px] font-medium transition-colors duration-150",
              on ? "text-primary-text" : "text-secondary-text hover:text-primary-text",
            )}
          >
            {on && <ActivePill layoutId={pill} radius={7} className="bg-frost-300 shadow-[0_1px_2px_rgba(0,0,0,0.3)]" />}
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
