"use client";
import { useEffect, useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { create } from "zustand";
import { cx } from "./ui";

export type MenuItem =
  | { kind?: "item"; label: string; icon?: ReactNode; onSelect: () => void; danger?: boolean; disabled?: boolean }
  | { kind: "separator" }
  | { kind: "heading"; label: string };

interface MenuState { open: boolean; x: number; y: number; items: MenuItem[]; token: number }
const useMenu = create<MenuState>(() => ({ open: false, x: 0, y: 0, items: [], token: 0 }));
let token = 0;

export const closeMenu = () => useMenu.setState({ open: false });

/**
 * Opens the context menu at the pointer. `load` adds items once they are known (for example the playlists to add to);
 * a menu that was closed or replaced in the meantime ignores them.
 */
export function openMenu(event: MouseEvent | { clientX: number; clientY: number; preventDefault: () => void; stopPropagation: () => void }, items: MenuItem[], load?: () => Promise<MenuItem[]>) {
  event.preventDefault();
  event.stopPropagation();
  const mine = ++token;
  useMenu.setState({ open: true, x: event.clientX, y: event.clientY, items, token: mine });
  if (load) {
    void load().then((more) => {
      if (useMenu.getState().token === mine && useMenu.getState().open) useMenu.setState((s) => ({ items: [...s.items, ...more] }));
    }, () => { /* the base items stay */ });
  }
}

/** Props for any element that should open a menu on right click (and on the Menu key / Shift+F10). */
const LONG_PRESS_MS = 450;
const LONG_PRESS_SLOP = 10;

export function menuHandlers(build: () => MenuItem[], load?: () => Promise<MenuItem[]>) {
  // iOS WebKit fires no `contextmenu` on touch, and Android's long press also scrolls on a slight drift, so a held
  // finger opens the menu itself. A move past the slop, a lift or a second finger cancels it.
  let timer = 0;
  let firedAt = 0;
  let origin = { x: 0, y: 0 };
  const cancel = () => { window.clearTimeout(timer); timer = 0; };
  return {
    onTouchStart: (e: React.TouchEvent) => {
      if (e.touches.length !== 1) return cancel();
      const t = e.touches[0];
      origin = { x: t.clientX, y: t.clientY };
      cancel();
      timer = window.setTimeout(() => {
        timer = 0;
        firedAt = Date.now();
        navigator.vibrate?.(10);
        openMenu({ clientX: origin.x, clientY: origin.y, preventDefault: () => {}, stopPropagation: () => {} }, build(), load);
      }, LONG_PRESS_MS);
    },
    onTouchMove: (e: React.TouchEvent) => {
      const t = e.touches[0];
      if (timer && t && Math.hypot(t.clientX - origin.x, t.clientY - origin.y) > LONG_PRESS_SLOP) cancel();
    },
    // Lifting the finger after a long press would otherwise "click" the row underneath and start the track.
    onClickCapture: (e: React.MouseEvent) => { if (Date.now() - firedAt < 700) { e.preventDefault(); e.stopPropagation(); } },
    onTouchEnd: cancel,
    onTouchCancel: cancel,
    onContextMenu: (e: MouseEvent) => openMenu(e, build(), load),
    onKeyDown: (e: React.KeyboardEvent) => {
      if (e.key === "ContextMenu" || (e.shiftKey && e.key === "F10")) {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        openMenu({ clientX: r.left + 24, clientY: r.bottom - 8, preventDefault: () => e.preventDefault(), stopPropagation: () => e.stopPropagation() }, build(), load);
      }
    },
  };
}

const MARGIN = 8;

/** One host for the whole app: portalled to <body>, closes on Escape, outside press, scroll, resize and blur. */
export function ContextMenuHost() {
  const { open, x, y, items } = useMenu();
  const ref = useRef<HTMLDivElement>(null);
  const [pos, setPos] = useState({ left: x, top: y });
  const [active, setActive] = useState(-1);
  const selectable = items.map((it, i) => (it.kind === undefined || it.kind === "item") && !(it as { disabled?: boolean }).disabled ? i : -1).filter((i) => i >= 0);

  useLayoutEffect(() => {
    if (!open || !ref.current) return;
    const r = ref.current.getBoundingClientRect();
    setPos({
      left: Math.max(MARGIN, Math.min(x, window.innerWidth - r.width - MARGIN)),
      top: Math.max(MARGIN, Math.min(y, window.innerHeight - r.height - MARGIN)),
    });
  }, [open, x, y, items.length]);

  useEffect(() => {
    if (!open) { setActive(-1); return; }
    setActive(selectable[0] ?? -1);
    ref.current?.focus();
    const away = (e: PointerEvent) => { if (!ref.current?.contains(e.target as Node)) closeMenu(); };
    const stop = () => closeMenu();
    document.addEventListener("pointerdown", away, true);
    window.addEventListener("blur", stop);
    window.addEventListener("resize", stop);
    window.addEventListener("scroll", stop, true);
    window.addEventListener("wheel", stop, { passive: true });
    return () => {
      document.removeEventListener("pointerdown", away, true);
      window.removeEventListener("blur", stop);
      window.removeEventListener("resize", stop);
      window.removeEventListener("scroll", stop, true);
      window.removeEventListener("wheel", stop);
    };
  }, [open]); // eslint-disable-line react-hooks/exhaustive-deps

  if (!open || typeof document === "undefined") return null;

  const choose = (i: number) => {
    const it = items[i];
    if (!it || (it.kind !== undefined && it.kind !== "item") || (it as { disabled?: boolean }).disabled) return;
    closeMenu();
    (it as { onSelect: () => void }).onSelect();
  };
  const move = (dir: 1 | -1) => {
    if (selectable.length === 0) return;
    const at = selectable.indexOf(active);
    setActive(selectable[(at + dir + selectable.length) % selectable.length]);
  };

  return createPortal(
    <div
      ref={ref}
      role="menu"
      tabIndex={-1}
      aria-label="Context menu"
      onContextMenu={(e) => e.preventDefault()}
      onKeyDown={(e) => {
        if (e.key === "Escape") { e.preventDefault(); e.stopPropagation(); closeMenu(); }
        else if (e.key === "ArrowDown") { e.preventDefault(); move(1); }
        else if (e.key === "ArrowUp") { e.preventDefault(); move(-1); }
        else if (e.key === "Home") { e.preventDefault(); setActive(selectable[0] ?? -1); }
        else if (e.key === "End") { e.preventDefault(); setActive(selectable[selectable.length - 1] ?? -1); }
        else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); choose(active); }
        else if (e.key === "Tab") { e.preventDefault(); closeMenu(); }
      }}
      className="glass-solid lx-pop-in fixed z-[80] flex min-w-[220px] max-w-[320px] flex-col gap-0.5 rounded-[10px] p-1.5 text-[13px] shadow-[0_24px_64px_rgba(0,0,0,0.55)] outline-none"
      style={{ left: pos.left, top: pos.top }}
    >
      {items.map((it, i) => {
        if (it.kind === "separator") return <div key={i} role="separator" className="mx-1 my-1 h-px bg-white/10" />;
        if (it.kind === "heading") return <p key={i} className="px-2.5 pt-1.5 pb-0.5 text-[11px] font-semibold text-secondary-text">{it.label}</p>;
        return (
          <button
            key={i}
            type="button"
            role="menuitem"
            disabled={it.disabled}
            onPointerEnter={() => !it.disabled && setActive(i)}
            onClick={() => choose(i)}
            className={cx(
              "flex h-8 w-full items-center gap-2.5 rounded-[6px] px-2.5 text-left transition-colors duration-100 disabled:opacity-40",
              it.danger ? "text-critical-coral" : "text-primary-text",
              active === i && "bg-white/10",
            )}
          >
            <span aria-hidden="true" className="flex size-4 shrink-0 items-center justify-center">{it.icon}</span>
            <span className="min-w-0 flex-1 truncate">{it.label}</span>
          </button>
        );
      })}
    </div>,
    document.body,
  );
}
