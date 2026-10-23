"use client";

import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { Search } from "lucide-react";
import { useUi, type View } from "../../lib/store";
import { useLocalPlaylists } from "../../features/playlists/usePlaylists";
import { cx } from "../ui";
import { DUR, tween } from "../../lib/motion";

type Command = { key: string; label: string; hint?: string; run: () => void };

export default function CommandPalette() {
  const s = useUi();
  const reduce = useReducedMotion();
  const { playlists } = useLocalPlaylists();
  const [text, setText] = useState("");
  const [active, setActive] = useState(0);
  const listRef = useRef<HTMLUListElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  const close = () => s.setPalette(false);

  const commands = useMemo<Command[]>(() => {
    const go = (view: View) => () => {
      s.navigate(view);
      close();
    };
    const list: Command[] = [
      { key: "nav-home", label: "Go to Listen Now", run: go("home") },
      { key: "nav-explore", label: "Go to Explore", run: go("explore") },
      { key: "nav-search", label: "Go to Search", run: go("search") },
      { key: "nav-library", label: "Go to Library", run: () => { s.openLibrary("playlists"); close(); } },
      { key: "nav-create", label: "Create playlist", run: go("playlists") },
      { key: "nav-connections", label: "Provider connections", run: go("connections") },
      { key: "nav-accounts", label: "Accounts", run: go("accounts") },
      { key: "nav-settings", label: "Settings", run: go("settings") },
      { key: "nav-sync", label: "Sync", run: go("sync") },
      { key: "nav-player", label: "Playback status", run: go("player") },
      {
        key: "now",
        label: "Open Now Playing (demo)",
        hint: "Demo",
        run: () => { s.setNowPlaying(true); close(); },
      },
      { key: "sidebar", label: "Toggle sidebar", hint: "⌘B", run: () => { s.toggleSidebar(); close(); } },
    ];
    for (const p of playlists) {
      list.push({
        key: `pl-${p.id}`,
        label: `Open playlist: ${p.name}`,
        hint: `${p.entryCount} ${p.entryCount === 1 ? "entry" : "entries"}`,
        run: () => { s.setSelectedPlaylist(p.id); s.navigate("playlists"); close(); },
      });
    }
    const q = text.trim();
    if (q !== "") {
      list.unshift({
        key: "search",
        label: `Search music for “${q}”`,
        run: () => { s.searchFor(q); close(); },
      });
    }
    return list;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [playlists, text, s.queue]);

  const visible = useMemo(() => {
    const needle = text.trim().toLowerCase();
    const out = needle === "" ? commands : commands.filter((c) => c.key === "search" || c.label.toLowerCase().includes(needle));
    return out.slice(0, 14);
  }, [commands, text]);

  useEffect(() => {
    if (!s.paletteOpen) return;
    setText("");
    setActive(0);
    const id = window.setTimeout(() => inputRef.current?.focus(), 0);
    return () => window.clearTimeout(id);
  }, [s.paletteOpen]);

  useEffect(() => {
    setActive(0);
  }, [text]);

  useEffect(() => {
    listRef.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  function onKeyDown(e: KeyboardEvent<HTMLInputElement>) {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setActive((i) => Math.min(i + 1, visible.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setActive((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      visible[active]?.run();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close();
    }
  }

  return (
    <AnimatePresence>
      {s.paletteOpen && (
        <motion.div
          key="palette"
          className="fixed inset-0 z-50 flex items-start justify-center bg-scrim pt-[12vh]"
          initial={reduce ? false : { opacity: 0 }}
          animate={{ opacity: 1 }}
          exit={{ opacity: 0, transition: tween(DUR.exit) }}
          transition={tween(0.16)}
          onMouseDown={(e) => {
            if (e.target === e.currentTarget) close();
          }}
        >
          <motion.div
            role="dialog"
            aria-modal="true"
            aria-label="Quick search"
            initial={reduce ? false : { opacity: 0, scale: 0.98 }}
            animate={{ opacity: 1, scale: 1 }}
            exit={{ opacity: 0, scale: 0.98, transition: tween(DUR.exit) }}
            transition={tween(0.16)}
            className="glass-solid w-[min(640px,calc(100vw-32px))] overflow-hidden rounded-[12px] shadow-[0_24px_64px_rgba(0,0,0,0.55)]"
          >
            <div className="flex items-center gap-3 px-4 py-3">
              <Search size={18} aria-hidden="true" className="text-secondary-text" />
              <input
                ref={inputRef}
                value={text}
                onChange={(e) => setText(e.currentTarget.value)}
                onKeyDown={onKeyDown}
                role="combobox"
                aria-expanded="true"
                aria-controls="palette-list"
                aria-activedescendant={visible[active] ? `palette-${active}` : undefined}
                aria-label="Quick search"
                placeholder="Search or type a command"
                className="h-10 min-w-0 flex-1 bg-transparent text-[17px] text-primary-text outline-none placeholder:text-tertiary-text"
              />
              <kbd className="rounded-[6px] bg-frost-200 px-1.5 py-0.5 text-[11px] text-secondary-text">Esc</kbd>
            </div>
            <ul id="palette-list" role="listbox" aria-label="Results" ref={listRef} className="max-h-[360px] overflow-y-auto border-t border-hairline p-2">
              {visible.length === 0 && <li className="px-3 py-6 text-center text-sm text-tertiary-text">No matches.</li>}
              {visible.map((c, i) => (
                <li
                  key={c.key}
                  id={`palette-${i}`}
                  role="option"
                  aria-selected={i === active}
                  data-index={i}
                  onMouseEnter={() => setActive(i)}
                  onClick={c.run}
                  className={cx(
                    "flex h-10 cursor-pointer items-center justify-between gap-3 rounded-[8px] px-3 text-[13px]",
                    i === active ? "bg-lumen-wash text-primary-text" : "text-primary-text",
                  )}
                >
                  <span className="truncate">{c.label}</span>
                  {c.hint && <span className="shrink-0 text-[11px] tabular-nums text-secondary-text">{c.hint}</span>}
                </li>
              ))}
            </ul>
          </motion.div>
        </motion.div>
      )}
    </AnimatePresence>
  );
}
