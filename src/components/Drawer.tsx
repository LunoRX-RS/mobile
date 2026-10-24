"use client";

import { AnimatePresence, motion, useDragControls } from "motion/react";
import { useEffect, type ReactNode } from "react";
import { X } from "lucide-react";
import { cx } from "./ui";

/** A sheet that springs up from the bottom. Drag the handle down, tap outside or press Escape to close it.
 * `height="60"` is 60% of the screen; "auto" fits the content. `fixed` covers the whole window; otherwise it fills the
 * nearest positioned parent (needed inside a modal dialog, which sits above everything else). */
export function Drawer({ open, onClose, title, height = "auto", fixed = false, z = "z-40", children, footer }: {
  open: boolean; onClose: () => void; title: string; height?: "auto" | "60"; fixed?: boolean; z?: string; children: ReactNode; footer?: ReactNode;
}) {
  const drag = useDragControls();
  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") { e.stopPropagation(); onClose(); } };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [open, onClose]);
  return (
    <AnimatePresence>
      {open && (
        <div key="drawer" role="presentation" className={cx(fixed ? "fixed" : "absolute", "inset-0", z)}>
          <motion.div aria-hidden="true" className="absolute inset-0 bg-black/55 will-change-[opacity]" initial={{ opacity: 0 }} animate={{ opacity: 1 }} exit={{ opacity: 0 }} transition={{ duration: 0.22 }} onClick={onClose} />
          <motion.div role="dialog" aria-modal="true" aria-label={title}
            initial={{ y: "100%" }} animate={{ y: 0 }} exit={{ y: "100%" }} transition={{ type: "spring", stiffness: 340, damping: 36, mass: 0.9 }}
            drag="y" dragControls={drag} dragListener={false} dragConstraints={{ top: 0, bottom: 0 }} dragElastic={{ top: 0, bottom: 0.5 }}
            onDragEnd={(_, info) => { if (info.offset.y > 100 || info.velocity.y > 500) onClose(); }}
            className={cx("glass-solid ln-safe-bottom absolute inset-x-0 bottom-0 flex transform-gpu flex-col rounded-t-[22px] shadow-[0_-12px_48px_rgba(0,0,0,0.5)] [backface-visibility:hidden] [contain:layout_paint] will-change-transform", height === "60" ? "h-[60dvh]" : "max-h-[88dvh]")}>
            <div onPointerDown={(e) => drag.start(e)} className="flex shrink-0 touch-none flex-col items-center pt-2.5">
              <span aria-hidden="true" className="h-1.5 w-10 rounded-full bg-white/25" />
              <div className="flex h-12 w-full items-center justify-between px-5">
                <h2 className="text-[16px] leading-6 font-semibold">{title}</h2>
                <button type="button" aria-label="Close" onPointerDown={(e) => e.stopPropagation()} onClick={onClose} className="-mr-2 flex size-11 items-center justify-center rounded-full text-secondary-text active:bg-frost-200"><X size={20} /></button>
              </div>
            </div>
            <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain px-4 pb-4">{children}</div>
            {footer && <div className="shrink-0 border-t border-hairline px-4 py-3">{footer}</div>}
          </motion.div>
        </div>
      )}
    </AnimatePresence>
  );
}
