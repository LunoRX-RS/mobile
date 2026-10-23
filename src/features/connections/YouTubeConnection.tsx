"use client";
import { useEffect, useState } from "react";
import type { UnlistenFn } from "@tauri-apps/api/event";
import { Button } from "../../components/ui";
import { usePlayback } from "../../lib/playback";
import { importSummary, useImport } from "../../lib/youtubeImport";
import { youtubeConnect, youtubeConnectFinish, youtubeConnectionStatus, youtubeDisconnect, youtubeImport,
  subscribeYouTubeConnection, type YouTubeConnectionStatus } from "../../services/tauri/youtube";

const LABELS: Record<YouTubeConnectionStatus, string> = {
  restoring: "Restoring your YouTube session…",
  disconnected: "Not connected. Public tracks can still play.",
  connecting: "Complete Google sign-in in the separate window. The app connects automatically when YouTube opens.",
  connected: "Connected to YouTube with your Google session.",
  expired: "Your Google session could not be restored. Sign in again.",
  storeError: "The OS key store is unavailable or locked. Unlock it, then reconnect.",
  unavailable: "YouTube connection is unavailable in this runtime.",
};
function errorText(error: unknown): string {
  const category = (error as { category?: string } | null)?.category;
  if (category === "signInIncomplete") return "Finish signing in to Google, then open YouTube in the sign-in window.";
  if (category === "sessionRejected") return "YouTube did not accept this session. Complete sign-in and try again.";
  if (category === "storeAccess") return "The OS key store refused access. Your session was not saved. Unlock it and reconnect.";
  if (category === "windowFailed") return "Could not open or read the sign-in window. Try again.";
  return "Could not connect YouTube. Open the desktop app and try again.";
}

export function YouTubeConnection() {
  const [status, setStatus] = useState<YouTubeConnectionStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState("");
  const imp = useImport();
  async function importNow() {
    setError("");
    try { await youtubeImport(); }
    catch (e) { if (e !== "inUse") setError("Could not import from YouTube Music. Check your connection and try again."); }
  }
  useEffect(() => {
    let active = true;
    let stop: UnlistenFn | undefined;
    subscribeYouTubeConnection((next) => { if (active) setStatus(next); })
      .then(async (unlisten) => {
        if (!active) { unlisten(); return; }
        stop = unlisten;
        const next = await youtubeConnectionStatus();
        if (active) setStatus(next);
      }).catch(() => { if (active) setStatus("unavailable"); });
    return () => { active = false; stop?.(); };
  }, []);
  async function run(action: () => Promise<YouTubeConnectionStatus>) {
    if (busy) return;
    setBusy(true); setError("");
    try { setStatus(await action()); }
    catch (error) { setError(errorText(error)); }
    finally { setBusy(false); }
  }
  async function disconnect() {
    const next = await youtubeDisconnect();
    usePlayback.setState({ requested: null, track: null, positionMs: 0, durationMs: 0 });
    return next;
  }
  return (
    <div className="flex flex-col gap-2 text-[13px]">
      <p role="status">{status === null ? "Checking YouTube connection…" : LABELS[status]}</p>
      <div className="flex gap-2">
        <Button variant="secondary" disabled={busy || status === null || status === "restoring" || status === "unavailable"}
          onClick={() => void run(youtubeConnect)}>
          {status === "connected" ? "Reconnect Google" : status === "connecting" ? "Show Google sign-in" : "Connect Google"}
        </Button>
        {status === "connecting" && <Button variant="secondary" disabled={busy} onClick={() => void run(youtubeConnectFinish)}>Finish sign-in</Button>}
        {status !== null && !["disconnected", "unavailable", "restoring"].includes(status) && (
          <Button variant="ghost" disabled={busy} onClick={() => void run(disconnect)}>{status === "connecting" ? "Cancel" : "Disconnect"}</Button>
        )}
      </div>
      {status === "connected" && (
        <div className="flex flex-wrap items-center gap-3">
          <Button variant="secondary" disabled={imp.state === "running"} onClick={() => void importNow()}>
            {imp.state === "running" ? "Importing…" : "Import from YouTube and YouTube Music"}
          </Button>
          <p role="status" className="text-secondary-text">
            {imp.state === "running" ? "Reading your liked songs and videos, playlists, Watch later and history." : imp.state === "failed" ? "The last import did not finish." : imp.report ? importSummary(imp.report) : "Liked songs and videos, saved playlists, Watch later and recent history are imported when you sign in and every few hours after."}
          </p>
        </div>
      )}
      <p className="text-secondary-text">Sign in on Google’s own page. YouTube session cookies stay in this device’s OS key store and are never sent to the LUNOR-X server.</p>
      {error && <p role="alert" className="text-critical-coral">{error}</p>}
    </div>
  );
}
