"use client";

import { Cable, Info, Keyboard, RefreshCw, SlidersHorizontal, UserRound, Volume2, type LucideIcon } from "lucide-react";
import { useCallback, useEffect, useState, type ReactNode } from "react";
import { CacheSettings } from "../offline/OfflinePlaylist";
import pkg from "../../../package.json";
import { Button } from "../../components/ui";
import { usePlayback } from "../../lib/playback";
import { useUi } from "../../lib/store";
import { syncStatus, type SyncStatusDto } from "../../services/tauri/sync";
import { AudioSettingsPanel } from "./AudioSettings";
import { ProviderKeys } from "./ProviderKeys";
import { LyricsOrder } from "./LyricsOrder";
import { TextStyleSettings } from "./TextStyleSettings";
import { DiscordSettings } from "./DiscordSettings";
import { isMobileOS } from "../../lib/layout";
import { TelemetrySettings } from "./TelemetrySettings";
import { Inset, NoteRow, SettingRow, SkeletonRows, StatusRow, syncSummary } from "./rows";

export type CategoryKey = "general" | "playback" | "accounts" | "sync" | "shortcuts" | "about";

export interface Category {
  key: CategoryKey;
  label: string;
  description: string;
  Icon: LucideIcon;
}

export const CATEGORIES: Category[] = [
  { key: "general", label: "General", description: "Appearance and what happens when the app starts.", Icon: SlidersHorizontal },
  { key: "playback", label: "Playback & audio", description: "Stream quality, audio output and what can play in the app.", Icon: Volume2 },
  { key: "accounts", label: "Accounts & providers", description: "Music provider sign-in and your LUNOR-X account.", Icon: UserRound },
  { key: "sync", label: "Sync", description: "How your playlists reach the LUNOR-X server.", Icon: Cable },
  { key: "shortcuts", label: "Keyboard shortcuts", description: "Keys that work across the app.", Icon: Keyboard },
  { key: "about", label: "About & diagnostics", description: "Version, platform and what the app can report about itself.", Icon: Info },
];

// Search index: row labels per category and the group heading to focus. Static on purpose; keep in step with the panes below.
// AudioSettingsPanel and ProviderKeys are owned elsewhere, so only their group ids and visible labels are listed.
export const SEARCH_INDEX: { cat: CategoryKey; group: string; label: string }[] = [
  { cat: "general", group: "settings-appearance", label: "Theme" },
  { cat: "general", group: "settings-appearance", label: "Reduced motion" },
  { cat: "general", group: "settings-appearance", label: "Reduced transparency" },
  { cat: "general", group: "settings-appstyle", label: "App font" },
  { cat: "general", group: "settings-appstyle", label: "Text size" },
  { cat: "general", group: "settings-text-lyrics", label: "Lyrics font" },
  { cat: "general", group: "settings-text-lyrics", label: "Lyrics motion" },
  { cat: "general", group: "settings-text-title", label: "Song title font" },
  { cat: "general", group: "settings-text-title", label: "Song title motion" },
  { cat: "general", group: "settings-fonts", label: "Imported fonts" },
  { cat: "general", group: "settings-usage", label: "Usage data" },
  { cat: "general", group: "settings-discord", label: "Discord Rich Presence" },
  { cat: "general", group: "settings-discord", label: "Discord Application ID" },
  { cat: "general", group: "settings-startup", label: "Restore last track" },
  { cat: "playback", group: "settings-audio", label: "Stream quality" },
  { cat: "playback", group: "settings-effects", label: "Sound shaping equalizer bass treble speed" },
  { cat: "playback", group: "settings-effects", label: "Nightcore slowed reverb 8D 4D moving sound keep pitch" },
  { cat: "playback", group: "settings-cache", label: "Disk cache & offline storage" },
  { cat: "playback", group: "settings-audio", label: "PCM output" },
  { cat: "playback", group: "settings-audio", label: "Sample rate" },
  { cat: "playback", group: "settings-audio", label: "Negotiated audio output" },
  { cat: "playback", group: "settings-lyrics", label: "Auto order" },
  { cat: "playback", group: "settings-lyrics", label: "Lyrics sources" },
  { cat: "playback", group: "settings-lyrics", label: "Use this source" },
  { cat: "playback", group: "settings-playback", label: "YouTube audio in the app" },
  { cat: "playback", group: "settings-playback", label: "Spotify audio" },
  { cat: "accounts", group: "settings-providers", label: "YouTube Music" },
  { cat: "accounts", group: "settings-providers", label: "Spotify" },
  { cat: "accounts", group: "settings-lunor-account", label: "LUNOR-X account" },
  { cat: "sync", group: "settings-sync-server", label: "Server endpoint" },
  { cat: "sync", group: "settings-sync-state", label: "Sync state" },
  { cat: "sync", group: "settings-sync-state", label: "Waiting changes" },
  { cat: "shortcuts", group: "settings-keys-nav", label: "Back and forward" },
  { cat: "shortcuts", group: "settings-keys-nav", label: "Collapse or expand the sidebar" },
  { cat: "shortcuts", group: "settings-keys-play", label: "Play or pause" },
  { cat: "shortcuts", group: "settings-keys-maker", label: "Lyric Maker" },
  { cat: "shortcuts", group: "settings-keys-search", label: "Quick search and commands" },
  { cat: "shortcuts", group: "settings-keys-mac", label: "Playback menu (macOS)" },
  { cat: "about", group: "settings-about", label: "App version" },
  { cat: "about", group: "settings-about", label: "Platform" },
  { cat: "about", group: "settings-about", label: "Native player (libmpv)" },
  { cat: "about", group: "settings-about", label: "Cache management" },
  { cat: "about", group: "settings-about", label: "Core status" },
];

type Load<T> = { state: "loading" } | { state: "error" } | { state: "ready"; value: T };

// Same IPC call Accounts and Sync use (sync_status); read-only here.
function useSyncStatus() {
  const [data, setData] = useState<Load<SyncStatusDto>>({ state: "loading" });
  const load = useCallback(() => {
    setData({ state: "loading" });
    let live = true;
    syncStatus().then(
      (value) => live && setData({ state: "ready", value }),
      () => live && setData({ state: "error" }),
    );
    return () => {
      live = false;
    };
  }, []);
  useEffect(() => load(), [load]);
  return { data, retry: load };
}

function RetryRow({ children, onRetry }: { children: ReactNode; onRetry: () => void }) {
  return (
    <NoteRow tone="bad" role="alert">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <span>{children}</span>
        <Button onClick={onRetry}>
          <RefreshCw size={14} strokeWidth={1.75} aria-hidden="true" />
          Retry
        </Button>
      </div>
    </NoteRow>
  );
}

function General() {
  return (
    <>
      <Inset id="settings-appearance" title="Appearance">
        <SettingRow label="Theme" hint="The one theme on all platforms.">LUNOR dark</SettingRow>
        <SettingRow label="Reduced motion" hint="Follows your operating system setting.">System</SettingRow>
        <SettingRow label="Reduced transparency" hint="Follows your operating system setting. The sidebar and player become solid.">System</SettingRow>
      </Inset>
      <TextStyleSettings />
      {!isMobileOS() && <DiscordSettings />}
      <TelemetrySettings />
      <Inset id="settings-startup" title="Startup">
        <SettingRow label="Restore last track" hint="Read-only. A restored track waits for you to press Play.">Resumes paused</SettingRow>
      </Inset>
    </>
  );
}

function Playback() {
  const nativeAvailable = usePlayback((s) => s.available);
  return (
    <>
      <AudioSettingsPanel />
      <CacheSettings />
      <LyricsOrder />
      <Inset id="settings-playback" title="What can play">
        <StatusRow
          label="YouTube audio in the app"
          tone={nativeAvailable ? "ok" : "off"}
          status={nativeAvailable ? "Available" : "Not confirmed"}
          hint="Plays through the native player (libmpv). Confirmed once a track has loaded in the Player."
        />
        <StatusRow
          label="Spotify audio"
          tone="off"
          status="Unsupported"
          hint="Spotify’s third-party playback SDK requires Premium. Spotify Free playback in LUNOR-X is unavailable."
        />
      </Inset>
    </>
  );
}

function Accounts() {
  const navigate = useUi((s) => s.navigate);
  const { data, retry } = useSyncStatus();
  const sum = data.state === "ready" ? syncSummary(data.value.state) : null;
  return (
    <>
      <ProviderKeys />
      <Inset id="settings-lunor-account" title="LUNOR-X account">
        {data.state === "loading" && <SkeletonRows rows={1} />}
        {data.state === "error" && <RetryRow onRetry={retry}>Could not read your account status.</RetryRow>}
        {sum && (
          <StatusRow
            label="LUNOR-X account"
            tone={sum.tone}
            status={sum.text}
            hint="Sign-in and sign-out for LUNOR-X sync. Separate from music provider sign-in."
          >
            <Button onClick={() => navigate("accounts")}>Open Accounts</Button>
          </StatusRow>
        )}
      </Inset>
    </>
  );
}

function Sync() {
  const navigate = useUi((s) => s.navigate);
  const { data, retry } = useSyncStatus();
  const v = data.state === "ready" ? data.value : null;
  const sum = v ? syncSummary(v.state) : null;
  return (
    <>
      <Inset id="settings-sync-server" title="Server">
        <SettingRow
          label="Server endpoint"
          hint="Read-only. No command exposes the configured endpoint or lets you change it. The core’s source default is http://127.0.0.1:8080."
        >
          Not reported
        </SettingRow>
      </Inset>
      <Inset id="settings-sync-state" title="Status">
        {data.state === "loading" && <SkeletonRows rows={2} />}
        {data.state === "error" && <RetryRow onRetry={retry}>Could not read the sync status.</RetryRow>}
        {v && sum && (
          <>
            <StatusRow label="Sync state" tone={sum.tone} status={sum.text}>
              <Button onClick={() => navigate("sync")}>Open Sync</Button>
            </StatusRow>
            <SettingRow label="Waiting changes" hint={v.conflicts.length > 0 ? `${v.conflicts.length} playlist conflict${v.conflicts.length === 1 ? "" : "s"} need a decision in Sync.` : undefined}>
              {v.pendingWrites === 0 ? "None" : String(v.pendingWrites)}
            </SettingRow>
          </>
        )}
      </Inset>
    </>
  );
}

function Shortcuts({ mac }: { mac: boolean }) {
  const groups: { id: string; title: string; hint?: string; rows: [string, string][] }[] = [
    {
      id: "settings-keys-nav",
      title: "Navigation",
      rows: [
        ["Back and forward", "⌘[ / ⌘]"],
        ["Collapse or expand the sidebar", "⌘B / Ctrl+B"],
        ["Larger, smaller, reset text size", "⌘+ ⌘− ⌘0 / Ctrl+ Ctrl− Ctrl0"],
        ["Close Now Playing, the command list or a dialog", "Esc"],
      ],
    },
    { id: "settings-keys-play", title: "Playback", rows: [["Play or pause the demo (not while typing)", "Space"]] },
    {
      id: "settings-keys-maker",
      title: "Lyric Maker",
      hint: "Open it from the Make lyrics file button in Now Playing.",
      rows: [
        ["Type along: first letter stamps a line's start", "Any letter"],
        ["Type along: stamp again, or 0.2 s earlier", "Tab / Shift+Tab"],
        ["Type along: add the line, or stamp the next line now", "Enter / ⌘Enter"],
        ["Tap along: stamp the next line, or un-stamp the last", "Space / Backspace"],
        ["Nudge a selected line by 0.1 s, or delete it", "[ ] / Delete"],
        ["Undo and redo (field empty)", "⌘Z / ⇧⌘Z"],
        ["Back 5 s", "⇧⌘B"],
        ["Close the maker, then Now Playing", "Esc"],
      ],
    },
    {
      id: "settings-keys-search",
      title: "Search",
      rows: [
        ["Quick search and commands", "⌘K / Ctrl+K"],
        ["Move and choose in Search and the command list", "↑ ↓ Enter"],
      ],
    },
  ];
  return (
    <>
      {groups.map((g) => (
        <Inset key={g.id} id={g.id} title={g.title}>
          {g.rows.map(([action, keys]) => (
            <SettingRow key={action} label={action}>{keys}</SettingRow>
          ))}
        </Inset>
      ))}
      {mac && (
        <Inset id="settings-keys-mac" title="Playback menu (macOS)">
          <NoteRow tone="off">macOS only. These work from the Playback menu in the menu bar.</NoteRow>
          {(
            [
              ["Play or pause", "⌘P"],
              ["Stop", "⌘."],
              ["Seek back / forward", "⌘← / ⌘→"],
              ["Volume up / down", "⌘↑ / ⌘↓"],
              ["Mute", "⌥⌘↓"],
            ] as const
          ).map(([action, keys]) => (
            <SettingRow key={action} label={action}>{keys}</SettingRow>
          ))}
        </Inset>
      )}
    </>
  );
}

function About({ platform }: { platform: string }) {
  const nativeAvailable = usePlayback((s) => s.available);
  return (
    <Inset id="settings-about" title="This app">
      <SettingRow label="App version">{pkg.version}</SettingRow>
      <SettingRow label="Platform">{platform || "Reading…"}</SettingRow>
      <StatusRow
        label="Native player (libmpv)"
        tone={nativeAvailable ? "ok" : "off"}
        status={nativeAvailable ? "Available" : "Not confirmed"}
        hint="Confirmed once a track has loaded in the Player."
      />
      <SettingRow label="Cache management" hint="Manage disk space in Playback & audio.">Disk cache + offline playlists</SettingRow>
      <SettingRow label="Core status" hint="Not reported: no diagnostics command yet.">Not reported</SettingRow>
    </Inset>
  );
}

export function CategoryPane({ cat, mac, platform }: { cat: CategoryKey; mac: boolean; platform: string }) {
  switch (cat) {
    case "general": return <General />;
    case "playback": return <Playback />;
    case "accounts": return <Accounts />;
    case "sync": return <Sync />;
    case "shortcuts": return <Shortcuts mac={mac} />;
    case "about": return <About platform={platform} />;
  }
}
