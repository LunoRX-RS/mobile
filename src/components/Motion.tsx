"use client";

import { useEffect, useRef, type ReactNode } from "react";
import { AnimatePresence, motion } from "motion/react";
import { DUR, EASE, SPRING, tween } from "../lib/motion";
import { cx, useMounted } from "./ui";

// Motion writes inline style attributes when it renders, and the Tauri CSP forbids them in the static HTML.
// Everything here renders plain markup until the first client render (useMounted), then upgrades.

/** One shared highlight that slides between the active items of a group (nav, chips, tabs). */
export function ActivePill({ layoutId, className, radius = 8 }: { layoutId: string; className: string; radius?: number }) {
  const mounted = useMounted();
  const cls = cx("pointer-events-none absolute inset-0 -z-10", className);
  if (!mounted) return <span aria-hidden="true" className={cls} />;
  return <motion.span layoutId={layoutId} aria-hidden="true" transition={tween(DUR.base)} style={{ borderRadius: radius }} className={cls} />;
}

/** Cross-fades its child when `k` changes (artwork on track change). The old child stays until the new one is in. */
export function Crossfade({ k, children, className }: { k: string; children: ReactNode; className?: string }) {
  const mounted = useMounted();
  if (!mounted) return <span className={cx("grid", className)}>{children}</span>;
  return (
    <span className={cx("grid", className)}>
      <AnimatePresence initial={false}>
        <motion.span key={k} initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={tween(DUR.base)} className="[grid-area:1/1]">
          {children}
        </motion.span>
      </AnimatePresence>
    </span>
  );
}

/** List item that slides when siblings reorder or disappear. Only for short lists (under 50 items). */
export function LayoutLi({ children }: { children: ReactNode }) {
  const mounted = useMounted();
  if (!mounted) return <li>{children}</li>;
  return (
    <motion.li layout="position" transition={tween(DUR.base)}>
      {children}
    </motion.li>
  );
}

/** Removable row: collapses with height and opacity. Wrap the list in <AnimatePresence initial={false}>. */
export function CollapseLi({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <motion.li initial={false} exit={{ opacity: 0, height: 0, overflow: "hidden" }} transition={tween(DUR.base)} className={className}>
      {children}
    </motion.li>
  );
}

/** Quick 1 -> 1.2 -> 1 pop (180ms) when `on` turns true. Not on first render. */
export function PopOnTrue({ on, children }: { on: boolean; children: ReactNode }) {
  const ref = useRef<HTMLSpanElement>(null);
  const prev = useRef(on);
  useEffect(() => {
    const el = ref.current;
    const was = prev.current;
    prev.current = on;
    if (!on || was || !el || typeof el.animate !== "function") return;
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) return;
    el.animate([{ transform: "scale(1)" }, { transform: "scale(1.2)", offset: 0.5 }, { transform: "scale(1)" }], { duration: 180, easing: "cubic-bezier(0.16, 1, 0.3, 1)" });
  }, [on]);
  return (
    <span ref={ref} className="inline-flex">
      {children}
    </span>
  );
}

export { SPRING, EASE };
