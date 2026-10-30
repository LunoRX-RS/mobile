"use client";

import { useEffect, useRef, useState } from "react";
import { ChartColumn, Sparkles, CirclePlay, Compass, EllipsisVertical, Library, Mic, Search, Settings, Cable, User, CloudSync, type LucideIcon } from "lucide-react";
import { useUi, type View } from "../../lib/store";
import { cx } from "../ui";
import { Drawer } from "../Drawer";

type Tab = { view: View; label: string; Icon: LucideIcon };

const TABS: Tab[] = [
  { view: "home", label: "Listen Now", Icon: CirclePlay },
  { view: "search", label: "Search", Icon: Search },
  { view: "explore", label: "Explore", Icon: Compass },
  { view: "library", label: "Library", Icon: Library },
];

// Everything the desktop sidebar footer holds, reachable from "More".
const MORE: Tab[] = [
  { view: "podcasts", label: "Podcasts", Icon: Mic },
  { view: "stats", label: "Stats", Icon: ChartColumn },
  { view: "replay", label: "Unified Replay", Icon: Sparkles },
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
  const opener = useRef<HTMLButtonElement>(null);
  // Focus goes back to "More" when the drawer closes (Escape and the back gesture are handled by the drawer and the app).
  useEffect(() => {
    if (!more) return;
    const returnTo = opener.current;
    return () => returnTo?.focus();
  }, [more]);
  const current = TAB_OF[view] ?? view;
  const inMore = MORE.some((m) => m.view === view);
  return (
    <>
      <Drawer open={more} onClose={() => setMore(false)} title="More" fixed z="z-[70]">
        <ul className="flex flex-col">
          {MORE.map(({ view: v, label, Icon }) => (
            <li key={v}>
              <button
                type="button"
                aria-current={view === v ? "page" : undefined}
                onClick={() => { setMore(false); navigate(v); }}
                className={cx("ln-touch flex min-h-14 w-full items-center gap-4 rounded-[12px] px-3 text-left text-[16px]", view === v ? "bg-lumen-wash text-lumen-lavender" : "text-primary-text active:bg-frost-200")}
              >
                <Icon size={22} aria-hidden="true" />
                {label}
              </button>
            </li>
          ))}
        </ul>
      </Drawer>
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
