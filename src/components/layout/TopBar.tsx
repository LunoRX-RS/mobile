"use client";

import { useEffect, useState } from "react";
import { ChevronLeft, ChevronRight, Search, User } from "lucide-react";
import { useUi } from "../../lib/store";
import { IconButton, cx } from "../ui";
import { isMobileOS, useLayout } from "../../lib/layout";

// Window title bar, drawn by the app: a frosted strip with a soft fade into the page (no border). The macOS traffic
// lights overlay the reserved left zone, vertically centred by `trafficLightPosition` in tauri.conf.json.
// Empty areas drag the window.
export default function TopBar() {
  const { back, forward, goBack, goForward, setPalette, navigate, view } = useUi();
  const layout = useLayout();
  const touch = layout === "tablet";
  const [mac, setMac] = useState(false);
  // The traffic-light gap is for a Mac window only; iPadOS reports a Mac user agent too.
  useEffect(() => setMac(/Mac/i.test(navigator.userAgent) && !isMobileOS()), []);
  const side = mac ? "w-[184px]" : "w-24";
  return (
    <header
      data-tauri-drag-region
      className="relative flex h-[var(--height-topbar)] shrink-0 items-center gap-3 bg-gradient-to-b from-white/[0.08] to-white/[0.02] px-4 backdrop-blur-xl"
    >
      <div data-tauri-drag-region className={cx("flex items-center gap-0.5", side, mac && "pl-[66px]")}>
        <IconButton label="Back" onClick={goBack} disabled={back.length === 0} className={touch ? "size-11" : "size-8"}>
          <ChevronLeft size={18} />
        </IconButton>
        <IconButton label="Forward" onClick={goForward} disabled={forward.length === 0} className={touch ? "size-11" : "size-8"}>
          <ChevronRight size={18} />
        </IconButton>
      </div>

      <div data-tauri-drag-region className="flex min-w-0 flex-1 justify-center">
        <button
          type="button"
          onClick={() => setPalette(true)}
          aria-label="Search, Command K"
          aria-keyshortcuts="Meta+K Control+K"
          className={cx("group flex w-full max-w-[440px] items-center gap-2 rounded-full bg-black/30 px-3.5 text-left text-[13px] text-secondary-text shadow-[inset_0_0_0_1px_rgba(255,255,255,0.06)] transition-colors duration-150 hover:bg-black/40 hover:text-primary-text", touch ? "h-11" : "h-8")}
        >
          <Search size={14} aria-hidden="true" />
          <span className="flex-1 truncate">Search songs, albums, artists</span>
          <kbd className="rounded-[5px] bg-white/10 px-1.5 text-[11px] leading-5 text-tertiary-text">{mac ? "⌘K" : "Ctrl K"}</kbd>
        </button>
      </div>

      <div data-tauri-drag-region className={cx("flex justify-end", side)}>
        <button
          type="button"
          aria-label="Account"
          title="Account"
          aria-current={view === "accounts" ? "page" : undefined}
          onClick={() => navigate("accounts")}
          className={cx(
            "flex items-center justify-center rounded-full transition-colors duration-150",
            touch ? "size-11" : "size-7",
            view === "accounts" ? "bg-lumen-wash text-lumen-lavender" : "bg-white/10 text-secondary-text hover:bg-white/20 hover:text-primary-text",
          )}
        >
          <User size={15} />
        </button>
      </div>
    </header>
  );
}
