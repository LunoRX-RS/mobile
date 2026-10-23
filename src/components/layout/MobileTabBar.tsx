"use client";

import { useEffect, useRef, useState } from "react";
import { ChartColumn, CirclePlay, Compass, EllipsisVertical, Library, Search, Settings, Cable, User, CloudSync, X, type LucideIcon } from "lucide-react";
import { useUi, type View } from "../../lib/store";
import { cx } from "../ui";

type Tab = { view: View; label: string; Icon: LucideIcon };

const TABS: Tab[] = [
  { view: "home", label: "Listen Now", Icon: CirclePlay },
  { view: "search", label: "Search", Icon: Search },
  { view: "explore", label: "Explore", Icon: Compass },
  { view: "library", label: "Library", Icon: Library },
];

// Everything the desktop sidebar footer holds, reachable from "More".
const MORE: Tab[] = [
  { view: "stats", label: "Stats", Icon: ChartColumn },
  { view: "sync", label: "Sync", Icon: CloudSync },
  { view: "connections", label: "Connections", Icon: Cable },
  { view: "accounts", label: "Account", Icon: User },
  { view: "settings", label: "Settings", Icon: Settings },
];

// Playlists and the artist/album pages sit under Library in the tab model.
const TAB_OF: Partial<Record<View, View>> = { playlists: "library", artist: "library", album: "library", player: "home", queue: undefined } as Partial<Record<View, View>>;

export default function MobileTabBar() {
  const view = useUi((s) => s.view);
  const navigate = useUi((s) => s.navigate);
  const [more, setMore] = useState(false);
  const closeRef = useRef<HTMLButtonElement>(null);
  const opener = useRef<HTMLButtonElement>(null);
  // Modal sheet: focus moves in, Escape (hardware keyboard, Android back via TalkBack) closes, focus returns to "More".
  useEffect(() => {
    if (!more) return;
    closeRef.current?.focus();
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setMore(false); };
    window.addEventListener("keydown", onKey);
    const returnTo = opener.current;
    return () => { window.removeEventListener("keydown", onKey); returnTo?.focus(); };
  }, [more]);
  const current = TAB_OF[view] ?? view;
  const inMore = MORE.some((m) => m.view === view);
  return (
    <>
      {more && (
        <div className="fixed inset-0 z-[70]" role="presentation" onClick={() => setMore(false)}>
          <div className="absolute inset-0 bg-scrim" />
          <div
            role="dialog"
            aria-modal="true"
            aria-label="More"
            onClick={(e) => e.stopPropagation()}
            className="glass-solid ln-safe-bottom absolute inset-x-0 bottom-0 rounded-t-[20px] px-4 pt-3 pb-4"
          >
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-[15px] leading-6 font-semibold">More</h2>
              <button ref={closeRef} type="button" aria-label="Close" onClick={() => setMore(false)} className="ln-touch flex items-center justify-center rounded-full text-secondary-text">
                <X size={20} />
              </button>
            </div>
            <ul className="flex flex-col">
              {MORE.map(({ view: v, label, Icon }) => (
                <li key={v}>
                  <button
                    type="button"
                    aria-current={view === v ? "page" : undefined}
                    onClick={() => { setMore(false); navigate(v); }}
                    className={cx("ln-touch flex w-full items-center gap-3 rounded-[12px] px-3 text-left text-[16px]", view === v ? "bg-lumen-wash text-lumen-lavender" : "text-primary-text active:bg-frost-200")}
                  >
                    <Icon size={20} aria-hidden="true" />
                    {label}
                  </button>
                </li>
              ))}
            </ul>
          </div>
        </div>
      )}
      <nav aria-label="Main" className="glass-bar ln-safe-bottom relative z-[50] shrink-0 border-t border-hairline">
        <ul className="grid grid-cols-5">
          {TABS.map(({ view: v, label, Icon }) => {
            const active = current === v;
            return (
              <li key={v}>
                <button
                  type="button"
                  aria-current={active ? "page" : undefined}
                  onClick={() => navigate(v)}
                  className={cx("flex h-[52px] w-full flex-col items-center justify-center gap-0.5 text-[10px] leading-3 font-medium", active ? "text-lumen-lavender" : "text-secondary-text")}
                >
                  <Icon size={22} aria-hidden="true" />
                  {label}
                </button>
              </li>
            );
          })}
          <li>
            <button
              ref={opener}
              type="button"
              aria-haspopup="dialog"
              aria-current={inMore ? "page" : undefined}
              onClick={() => setMore(true)}
              className={cx("flex h-[52px] w-full flex-col items-center justify-center gap-0.5 text-[10px] leading-3 font-medium", inMore ? "text-lumen-lavender" : "text-secondary-text")}
            >
              <EllipsisVertical size={22} aria-hidden="true" />
              More
            </button>
          </li>
        </ul>
      </nav>
    </>
  );
}
