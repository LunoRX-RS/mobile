"use client";

import { loadCustomFonts } from "../../lib/textStyle";
import { useEffect, useRef, useState, type ReactNode } from "react";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AnimatePresence, MotionConfig, motion, useReducedMotion } from "motion/react";
import { initHistory } from "../../lib/history";
import { initBackNav } from "../../lib/backNav";
import { installBrowserDemo } from "../../lib/demo/browserDemo";
import { initTelemetry, track } from "../../lib/telemetry";
import { initRemote } from "../../lib/remote";
import { hydrateRecentSearches } from "../../lib/store";
import { useUi } from "../../lib/store";
import ArtistScreen, { AlbumScreen, SavedLibraryScreen } from "../../features/artists/ArtistsScreen";
import { Button, cx } from "../ui";
import { useLibrary } from "../../lib/library";
import { refreshLikes } from "../../lib/library";
import HomeScreen from "../../features/home/HomeScreen";
import ExploreScreen from "../../features/explore/ExploreScreen";
import SearchScreen from "../../features/search/SearchScreen";
import LibraryScreen from "../../features/library/LibraryScreen";
import PlaylistEditor from "../../features/playlists/PlaylistEditor";
import PlayerScreen from "../../features/player/PlayerScreen";
import ConnectionsScreen from "../../features/connections/ConnectionsScreen";
import AccountsScreen from "../../features/accounts/AccountsScreen";
import { LoginGate } from "../../features/accounts/LoginGate";
import LoginPage from "../../features/accounts/LoginPage";
import SettingsScreen from "../../features/settings/SettingsScreen";
import StatsScreen from "../../features/stats/StatsScreen";
import ReplayScreen from "../../features/replay/ReplayScreen";
import SharedScreen from "../../features/shared/SharedScreen";
import { SHARE_OPEN_EVENT, sharedLinkTake } from "../../services/tauri/share";
import SyncPanel from "../../features/sync/SyncPanel";
import Sidebar from "./Sidebar";
import TopBar from "./TopBar";
import PlayerBar from "./PlayerBar";
import NowPlaying from "./NowPlaying";
import CommandPalette from "./CommandPalette";
import QueueScreen from "../../features/queue/QueueScreen";
import NativeQueue from "../../features/queue/NativeQueue";
import { PlaybackEvents } from "./NativePlayer";
import { usePlayback, togglePlayback } from "../../lib/playback";
import { DUR, tween } from "../../lib/motion";
import { ContextMenuHost } from "../ContextMenu";
import { loadFeed } from "../../lib/discovery";
import { initYouTubeImport } from "../../lib/youtubeImport";
import { applySavedAudio, watchAudioSettings } from "../../lib/audioSettings";
import { applySavedMode } from "../../lib/playMode";
import { initAppStyle, resetAppScale, stepAppScale } from "../../lib/appStyle";
import { useLayout } from "../../lib/layout";
import MobileTabBar from "./MobileTabBar";
import MobilePlayerBar from "./MobilePlayerBar";


export type { View } from "../../lib/store";

function isTypingTarget(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  return target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT", "BUTTON"].includes(target.tagName);
}

// Keyboard shortcuts. Cmd on macOS, Ctrl elsewhere.
function useGlobalKeys() {
  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      const mod = e.metaKey || e.ctrlKey;
      const s = useUi.getState();
      if (mod && e.key.toLowerCase() === "k") {
        e.preventDefault();
        s.setPalette(!s.paletteOpen);
        return;
      }
      if (mod && e.key === "[") {
        e.preventDefault();
        s.goBack();
        return;
      }
      if (mod && e.key === "]") {
        e.preventDefault();
        s.goForward();
        return;
      }
      if (mod && (e.key === "=" || e.key === "+")) { e.preventDefault(); stepAppScale(1); return; }
      if (mod && (e.key === "-" || e.key === "_")) { e.preventDefault(); stepAppScale(-1); return; }
      if (mod && e.key === "0") { e.preventDefault(); resetAppScale(); return; }
      if (mod && e.key.toLowerCase() === "b") {
        e.preventDefault();
        s.toggleSidebar();
        return;
      }
      if (e.key === " " && !mod && !s.paletteOpen && !isTypingTarget(e.target)) {
        e.preventDefault();
        if (usePlayback.getState().requested || usePlayback.getState().track) void togglePlayback();
        else s.togglePlay();
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);
}

// Motion writes inline style attributes when rendered on the server, and the Tauri CSP forbids those.
// The animated wrapper therefore mounts only after the first client render; the static HTML stays plain.
function ViewFrame({ mounted, children }: { mounted: boolean; children: ReactNode }) {
  const reduce = useReducedMotion();
  if (!mounted) return <div>{children}</div>;
  return (
    <motion.div
      initial={{ opacity: 0, y: reduce ? 0 : 8 }}
      animate={{ opacity: 1, y: 0, transition: tween(DUR.base) }}
      exit={{ opacity: 0, transition: tween(DUR.exit) }}
    >
      {children}
    </motion.div>
  );
}

// Plain browser during development only: sample songs and generated tones stand in for the Tauri core.
if (process.env.NODE_ENV === "development") installBrowserDemo();

export default function AppShell() {
  const reduce = useReducedMotion();
  const layout = useLayout();
  const phone = layout === "phone";
  const [mounted, setMounted] = useState(false);
  useEffect(() => { void loadCustomFonts(); }, []);
  const openShared = useUi((s) => s.openShared);
  // Share links open the shared-playlist view: live events, plus a link that arrived before this window listened.
  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void listen<string>(SHARE_OPEN_EVENT, (event) => openShared(event.payload)).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    });
    void sharedLinkTake().then((token) => { if (token && active) openShared(token); }).catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, [openShared]);
  useEffect(() => {
    setMounted(true);
    hydrateRecentSearches();
    void refreshLikes();
    const stopImport = initYouTubeImport();
    const stopBackNav = initBackNav();
    void applySavedAudio();
    const stopAudio = watchAudioSettings();
    applySavedMode();
    initAppStyle();
    // Warm the first screens while the window is still settling: both feeds, and Explore's first topic.
    const warm = () => { void loadFeed("home"); void loadFeed("explore", "charts"); };
    const idle = (window as { requestIdleCallback?: (f: () => void) => number }).requestIdleCallback;
    if (idle) idle(warm); else window.setTimeout(warm, 300);
    return () => { stopImport(); stopAudio(); stopBackNav(); };
  }, []);
  const mainRef = useRef<HTMLElement>(null);
  const libraryError = useLibrary(s => s.error);
  const libraryTab = useUi(s => s.libraryTab);
  const view = useUi((s) => s.view);
  const selectedPlaylistId = useUi((s) => s.selectedPlaylistId);
  const queueOpen = useUi((s) => s.queueOpen);
  const nativeQueue = usePlayback((p) => p.requested !== null || p.track !== null || p.resume !== null);
  const navigate = useUi((s) => s.navigate);
  const setSelectedPlaylist = useUi((s) => s.setSelectedPlaylist);
  useGlobalKeys();
  useEffect(() => initHistory(), []);
  useEffect(() => initTelemetry(), []);
  useEffect(() => initRemote(), []);
  // Likes can change on another device: the sync engine emits the same event a local toggle does.
  useEffect(() => {
    let active = true;
    let unlisten: UnlistenFn | undefined;
    void listen("core://library/changed", () => void refreshLikes()).then((fn) => {
      if (active) unlisten = fn;
      else fn();
    }).catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);
  useEffect(() => { track(`feature.view.${view}`); }, [view]);

  // Demo playback clock. Only advances while the simulated state is "playing".
  useEffect(() => {
    const id = window.setInterval(() => useUi.getState().tick(), 1000);
    return () => window.clearInterval(id);
  }, []);

  // The static export and the hydration pass assume the desktop layout (no window to measure). Showing it for a
  // moment on a phone is the flash of the PC layout, so the canvas stays empty until the real width is known.
  if (!mounted) return <div className="h-dvh bg-night-canvas" />;

  return (
    <MotionConfig reducedMotion="user">
      <PlaybackEvents />
      {(window as unknown as { __lunorDemo?: boolean }).__lunorDemo && (
        <div role="note" className="pointer-events-none fixed top-1 right-2 z-[80] rounded-full bg-frost-300 px-2.5 py-0.5 text-[11px] text-secondary-text backdrop-blur">Demo · sample songs, generated tones</div>
      )}
      <a
        href="#main-content"
        className="glass-solid sr-only top-2 left-2 z-[60] rounded-[8px] px-3 py-2 text-[13px] focus:not-sr-only focus:fixed focus:inline-flex focus:min-h-8 focus:items-center"
      >
        Skip to content
      </a>
      <div className={cx("ln-safe-x flex h-dvh flex-col bg-night-canvas", !(view === "artist" && phone) && "ln-safe-top")}>
        {!phone && <TopBar />}
        <div className="flex min-h-0 flex-1">
          {!phone && <Sidebar />}
          <div className="flex min-w-0 flex-1 flex-col">
            {libraryError && <div role="alert" className="mx-8 mt-2 mb-3 flex shrink-0 items-center justify-between gap-4 rounded-lg bg-frost-200 px-4 py-2 text-[13px] text-secondary-text"><p>{libraryError}</p><Button className="shrink-0" onClick={() => void refreshLikes()}>Retry likes</Button></div>}
            <main ref={mainRef} id="main-content" tabIndex={-1} className="min-h-0 flex-1 overflow-y-auto outline-none">
              <div className={view === "artist" ? "w-full pb-6 md:pb-16" : phone ? "mx-auto w-full max-w-content px-4 pt-3 pb-6" : layout === "tablet" ? "mx-auto w-full max-w-content px-6 pt-4 pb-16" : "mx-auto w-full max-w-content px-8 pt-4 pb-16"}>
                <AnimatePresence mode="wait" initial={false} onExitComplete={() => mainRef.current?.scrollTo({ top: 0 })}>
                  <ViewFrame key={view} mounted={mounted}>
                    {view === "home" && <HomeScreen onNavigate={navigate} onOpenPlaylist={(id) => { setSelectedPlaylist(id); navigate("playlists"); }} />}
                    {view === "explore" && <ExploreScreen />}
                    {view === "search" && <SearchScreen />}
                    {view === "artist" && <ArtistScreen />}
                    {view === "album" && <AlbumScreen />}
                    {view === "library" && (libraryTab === "songs" || libraryTab === "artists") && <SavedLibraryScreen artists={libraryTab === "artists"} />}
                    {view === "library" && libraryTab !== "songs" && libraryTab !== "artists" && <LibraryScreen onOpenPlaylist={(id) => { setSelectedPlaylist(id); navigate("playlists"); }} />}
                    {view === "playlists" && (
                      <PlaylistEditor selectedId={selectedPlaylistId} onSelect={setSelectedPlaylist} onNavigate={navigate} />
                    )}
                    {view === "player" && <div className="lx-scope"><PlayerScreen /></div>}
                    {view === "connections" && <div className="lx-scope"><ConnectionsScreen /></div>}
                    {view === "accounts" && <AccountsScreen />}
                    {view === "login" && <LoginPage />}
                    {view === "settings" && <SettingsScreen />}
                    {view === "stats" && <StatsScreen />}
                    {view === "replay" && <ReplayScreen />}
                    {view === "shared" && <SharedScreen />}
                    {view === "sync" && <div className="lx-scope"><SyncPanel /></div>}
                  </ViewFrame>
                </AnimatePresence>
              </div>
            </main>
          </div>
        </div>
        {phone ? <><MobilePlayerBar /><MobileTabBar /></> : <PlayerBar />}
      </div>
      <AnimatePresence>
        {queueOpen && (
          <motion.aside
            key="queue"
            aria-label={nativeQueue ? "Queue" : "Queue (demo)"}
            initial={{ opacity: 0, scale: reduce ? 1 : 0.97 }}
            animate={{ opacity: 1, scale: 1, transition: tween(0.18) }}
            exit={{ opacity: 0, scale: reduce ? 1 : 0.97, transition: tween(DUR.exit) }}
            style={{ transformOrigin: "100% 100%" }}
            className={phone ? "glass-solid ln-safe-top ln-safe-bottom fixed inset-0 z-[65] overflow-y-auto p-3" : "glass-solid fixed top-[var(--height-topbar)] right-4 bottom-[calc(var(--height-player)+12px)] z-[45] w-[min(440px,calc(100vw-32px))] overflow-y-auto rounded-[12px] p-3 shadow-[0_16px_48px_rgba(0,0,0,0.5)]"}
          >
            {phone && (
              <div className="mb-2 flex justify-end">
                <Button onClick={() => useUi.getState().toggleQueue()} className="ln-touch">Done</Button>
              </div>
            )}
            {nativeQueue ? <NativeQueue /> : <QueueScreen />}
          </motion.aside>
        )}
      </AnimatePresence>
      <NowPlaying />
      <CommandPalette />
      <ContextMenuHost />
      <LoginGate />
    </MotionConfig>
  );
}
