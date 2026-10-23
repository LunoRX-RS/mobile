"use client";

import { Search } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { ActivePill } from "../../components/Motion";
import { Screen, cx } from "../../components/ui";
import { CATEGORIES as ALL_CATEGORIES, CategoryPane, SEARCH_INDEX as ALL_INDEX, type CategoryKey } from "./categories";
import { DESKTOP_ONLY_CATEGORIES, DESKTOP_ONLY_GROUPS, isMobileOS } from "../../lib/layout";
import { FIELD, InsetLevel } from "./rows";

const STORE_KEY = "lunorx.settings.category";
const MOBILE = typeof window !== "undefined" && isMobileOS();
const CATEGORIES = MOBILE ? ALL_CATEGORIES.filter((c) => !DESKTOP_ONLY_CATEGORIES.has(c.key)) : ALL_CATEGORIES;
const SEARCH_INDEX = MOBILE ? ALL_INDEX.filter((r) => !DESKTOP_ONLY_GROUPS.has(r.group) && !DESKTOP_ONLY_CATEGORIES.has(r.cat)) : ALL_INDEX;
const isKey = (v: unknown): v is CategoryKey => CATEGORIES.some((c) => c.key === v);

// Settings, split into categories (DESIGN.md, Settings). Left rail from 1100px, a wrapping tab row below.
// The chosen category is local state, remembered per viewer in localStorage. `initialCategory` lets other screens link later.
export default function SettingsScreen({ initialCategory }: { initialCategory?: CategoryKey } = {}) {
  const uid = useId();
  const [cat, setCat] = useState<CategoryKey>(initialCategory ?? "general");
  const [wide, setWide] = useState(true);
  const [mac, setMac] = useState(false);
  const [platform, setPlatform] = useState("");
  const [query, setQuery] = useState("");
  const [focusGroup, setFocusGroup] = useState<string | null>(null);
  const tabRefs = useRef<Record<string, HTMLButtonElement | null>>({});

  useEffect(() => {
    const nav = navigator as Navigator & { userAgentData?: { platform?: string } };
    setPlatform(nav.userAgentData?.platform || nav.platform || "Unknown");
    setMac(/Mac/i.test(navigator.userAgent));
    if (!initialCategory) {
      try {
        const saved = localStorage.getItem(STORE_KEY);
        if (isKey(saved)) setCat(saved);
      } catch {
        /* storage blocked: stay on General */
      }
    }
    const mq = window.matchMedia("(min-width: 1100px)");
    const sync = () => setWide(mq.matches);
    sync();
    mq.addEventListener("change", sync);
    return () => mq.removeEventListener("change", sync);
  }, [initialCategory]);

  function choose(next: CategoryKey) {
    setCat(next);
    try {
      localStorage.setItem(STORE_KEY, next);
    } catch {
      /* storage blocked: the choice lasts for this visit */
    }
  }

  // Other screens (the lyrics source sheet) can ask an open Settings to jump to a category and group.
  useEffect(() => {
    const open = (event: Event) => {
      const d = (event as CustomEvent<{ cat?: unknown; group?: unknown }>).detail;
      if (isKey(d?.cat)) { setCat(d.cat); if (typeof d.group === "string") setFocusGroup(d.group); }
    };
    window.addEventListener("lunorx:settings-open", open);
    return () => window.removeEventListener("lunorx:settings-open", open);
  }, []);

  // After a search result is chosen, move focus to that group's heading once its pane has rendered.
  useEffect(() => {
    if (!focusGroup) return;
    document.getElementById(focusGroup)?.focus();
    setFocusGroup(null);
  }, [focusGroup, cat]);

  const q = query.trim().toLowerCase();
  const rows = useMemo(
    () => (q ? SEARCH_INDEX.filter((r) => (mac || r.group !== "settings-keys-mac") && r.label.toLowerCase().includes(q)) : []),
    [q, mac],
  );
  const visible = q
    ? CATEGORIES.filter((c) => c.label.toLowerCase().includes(q) || rows.some((r) => r.cat === c.key))
    : CATEGORIES;

  function onTabKey(e: KeyboardEvent<HTMLButtonElement>) {
    const prev = wide ? "ArrowUp" : "ArrowLeft";
    const next = wide ? "ArrowDown" : "ArrowRight";
    const i = visible.findIndex((c) => c.key === cat);
    let to = -1;
    if (e.key === next) to = (i + 1) % visible.length;
    else if (e.key === prev) to = (i - 1 + visible.length) % visible.length;
    else if (e.key === "Home") to = 0;
    else if (e.key === "End") to = visible.length - 1;
    if (to < 0) return;
    e.preventDefault();
    const key = visible[to].key;
    choose(key);
    tabRefs.current[key]?.focus();
  }

  const active = CATEGORIES.find((c) => c.key === cat) ?? CATEGORIES[0];
  const tabId = (k: string) => `${uid}-tab-${k}`;
  const panelId = `${uid}-panel`;

  return (
    <div data-pg className="flex max-w-[1000px] flex-col gap-8">
      <Screen id="settings-title" title="Settings">
        <div className="flex flex-col gap-6 min-[1100px]:flex-row min-[1100px]:items-start min-[1100px]:gap-10">
          <div className="flex flex-col gap-3 min-[1100px]:sticky min-[1100px]:top-0 min-[1100px]:w-[200px] min-[1100px]:shrink-0">
            <div className="relative max-w-[320px]">
              <Search size={16} strokeWidth={1.75} aria-hidden="true" className="pointer-events-none absolute top-1/2 left-3 -translate-y-1/2 text-secondary-text" />
              <input
                type="search"
                aria-label="Search settings"
                placeholder="Search settings"
                autoComplete="off"
                value={query}
                onChange={(e) => setQuery(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Escape" && query) {
                    e.preventDefault();
                    e.stopPropagation();
                    setQuery("");
                  }
                }}
                className={cx(FIELD, "ln-sc-search h-9 w-full pl-9 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-text focus-visible:outline-solid")}
              />
            </div>

            {visible.length > 0 && (
              <div
                role="tablist"
                aria-label="Settings categories"
                aria-orientation={wide ? "vertical" : "horizontal"}
                className="flex flex-row flex-wrap gap-1 min-[1100px]:flex-col"
              >
                {visible.map((c) => {
                  const on = c.key === cat;
                  return (
                    <button
                      key={c.key}
                      ref={(el) => {
                        tabRefs.current[c.key] = el;
                      }}
                      type="button"
                      role="tab"
                      id={tabId(c.key)}
                      aria-selected={on}
                      aria-controls={panelId}
                      tabIndex={on ? 0 : -1}
                      onClick={() => choose(c.key)}
                      onKeyDown={onTabKey}
                      className={cx(
                        "relative isolate flex h-9 items-center gap-2.5 rounded-[8px] px-3 text-left text-[13px] leading-[18px] font-medium transition-colors duration-150",
                        "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-text focus-visible:outline-solid",
                        on ? "text-primary-text" : "text-secondary-text hover:bg-frost-100 hover:text-primary-text",
                      )}
                    >
                      {on && <ActivePill layoutId="settings-cat-pill" className="bg-frost-200" />}
                      <c.Icon size={18} strokeWidth={1.75} aria-hidden="true" className="shrink-0" />
                      <span>{c.label}</span>
                    </button>
                  );
                })}
              </div>
            )}

            {q && rows.length > 0 && (
              <ul aria-label="Matching settings" className="flex flex-col gap-0.5">
                {rows.map((r) => (
                  <li key={`${r.group}-${r.label}`}>
                    <button
                      type="button"
                      onClick={() => {
                        choose(r.cat);
                        setQuery("");
                        setFocusGroup(r.group);
                      }}
                      className="flex min-h-9 w-full flex-col justify-center rounded-[8px] px-3 py-1 text-left hover:bg-frost-100 focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-text focus-visible:outline-solid"
                    >
                      <span className="text-[13px] leading-[18px] text-primary-text">{r.label}</span>
                      <span className="text-[12px] leading-4 text-secondary-text">{CATEGORIES.find((c) => c.key === r.cat)?.label}</span>
                    </button>
                  </li>
                ))}
              </ul>
            )}
            {q && visible.length === 0 && (
              <p role="status" className="px-3 text-[13px] leading-[18px] text-secondary-text">
                No settings match
              </p>
            )}
          </div>

          <div id={panelId} role="tabpanel" aria-labelledby={tabId(cat)} className="flex min-w-0 max-w-[640px] flex-1 flex-col gap-8">
            <header className="flex flex-col gap-1">
              <h2 className="font-display text-[22px] leading-7 font-semibold tracking-[-0.01em]">{active.label}</h2>
              <p className="text-[13px] leading-[18px] text-secondary-text">{active.description}</p>
            </header>
            <InsetLevel value={3}>
              <CategoryPane cat={cat} mac={mac} platform={platform} />
            </InsetLevel>
          </div>
        </div>
      </Screen>
    </div>
  );
}
