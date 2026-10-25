"use client";

import { useEffect, useState } from "react";
import { Button, StatusNote } from "../../components/ui";
import { copyText } from "../../lib/clipboard";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import { syncListShares, syncRevokeShare, syncShare, type ErrorCategory, type ShareDto } from "../../services/tauri/sync";

// Read-only links to an uploaded playlist. Anyone with a link can read the playlist; revoking stops that link at once.
// The link contains its token, so it is shown here and never logged.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e) ? (e as ErrorCategory) : "internal";
}

// One live link per playlist (the server enforces it). The server sends its URL to the owner; this device also keeps
// the URL it made, for links from before the server kept tokens. Kept only in this device's storage, never logged.
const linkKey = (localId: string) => `lunorx.shareLink.${localId}`;
function readLink(localId: string): { shareId: string; url: string } | null {
  try {
    const v = JSON.parse(localStorage.getItem(linkKey(localId)) ?? "null") as unknown;
    return v && typeof v === "object" && typeof (v as { url?: unknown }).url === "string" ? (v as { shareId: string; url: string }) : null;
  } catch {
    return null;
  }
}
function writeLink(localId: string, value: { shareId: string; url: string } | null) {
  try {
    if (value) localStorage.setItem(linkKey(localId), JSON.stringify(value));
    else localStorage.removeItem(linkKey(localId));
  } catch {
    /* storage unavailable: the link shows until this view closes */
  }
}

export function ShareSection({
  localId,
  signedIn,
  onSignIn,
}: {
  localId: string;
  signedIn: boolean;
  onSignIn: () => void;
}) {
  const [shares, setShares] = useState<ShareDto[] | null>(null);
  const [saved, setSaved] = useState<{ shareId: string; url: string } | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setSaved(readLink(localId));
    setCopied(false);
    setError(null);
    if (!signedIn) {
      setShares(null);
      return;
    }
    syncListShares(localId)
      .then(setShares)
      .catch((e) => setError(CATEGORY_MESSAGES[toCategory(e)]));
  }, [localId, signedIn]);

  const active = (shares ?? []).find((s) => s.active) ?? null;
  // The server sends the live link's URL. A link from an older server, or made before the server kept tokens, falls
  // back to the URL this device saved when it made the link.
  const link = active ? active.url ?? (saved && saved.shareId === active.id ? saved.url : null) : null;

  async function create() {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const created = await syncShare(localId);
      const next = { shareId: created.id, url: created.url };
      writeLink(localId, next);
      setSaved(next);
      setShares(await syncListShares(localId));
    } catch (e) {
      setError(CATEGORY_MESSAGES[toCategory(e)]);
    }
    setBusy(false);
  }

  async function stop() {
    if (!active) return;
    setBusy(true);
    setError(null);
    try {
      await syncRevokeShare(localId, active.id);
      writeLink(localId, null);
      setSaved(null);
      setShares(await syncListShares(localId));
    } catch (e) {
      setError(CATEGORY_MESSAGES[toCategory(e)]);
    }
    setBusy(false);
  }

  async function copy() {
    if (!link) return;
    if (await copyText(link)) setCopied(true);
    else setError("Could not copy. Select the link and copy it yourself.");
  }

  if (!signedIn) {
    return (
      <StatusNote tone="blue">
        Sign in to share this playlist.
        <Button variant="ghost" onClick={onSignIn} className="ml-1 h-8 px-2">
          Sign in
        </Button>
      </StatusNote>
    );
  }

  return (
    <section aria-labelledby={`share-${localId}`} className="flex flex-col gap-2">
      <h3 id={`share-${localId}`} className="text-[13px] font-semibold">
        Share
      </h3>
      {!active && (
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="secondary" disabled={busy || shares === null} onClick={() => void create()}>
            {busy ? "Working…" : "Create share link"}
          </Button>
          <span className="text-[12px] text-secondary-text">One link per playlist. Anyone with it can view, not edit.</span>
        </div>
      )}
      {active && link && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            readOnly
            aria-label="Share link"
            value={link}
            onFocus={(e) => e.currentTarget.select()}
            className="min-w-0 flex-1 rounded-[8px] bg-frost-100 px-3 py-2 text-[12px]"
          />
          <Button onClick={() => void copy()}>{copied ? "Copied" : "Copy link"}</Button>
        </div>
      )}
      {active && !link && (
        <p className="text-[12px] text-secondary-text">
          This link was made before links could be shown again. Recreate it once to see it here; the old link stops
          working.
        </p>
      )}
      {active && (
        <div className="flex flex-wrap items-center gap-2">
          <span className="text-[12px] text-secondary-text">Created {new Date(active.createdAt).toLocaleDateString()}</span>
          <Button variant="ghost" disabled={busy} onClick={() => void create()} title="Makes a new link; the current one stops working">
            Recreate link
          </Button>
          <Button variant="ghost" disabled={busy} onClick={() => void stop()}>
            Stop sharing
          </Button>
        </div>
      )}
      {error && (
        <p role="alert" className="text-[12px] text-critical-coral">
          {error}
        </p>
      )}
    </section>
  );
}
