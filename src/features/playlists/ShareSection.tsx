"use client";

import { useEffect, useState } from "react";
import { Button, StatusNote } from "../../components/ui";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import { syncListShares, syncRevokeShare, syncShare, type ErrorCategory, type ShareDto } from "../../services/tauri/sync";
import { copyText } from "../../lib/clipboard";

// Read-only links to an uploaded playlist. Anyone with a link can read the playlist; revoking stops that link at once.
// The link contains its token, so it is shown here and never logged.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e) ? (e as ErrorCategory) : "internal";
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
  const [link, setLink] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setLink(null);
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

  async function create() {
    setBusy(true);
    setError(null);
    setCopied(false);
    try {
      const created = await syncShare(localId);
      setLink(created.url);
      setShares(await syncListShares(localId));
    } catch (e) {
      setError(CATEGORY_MESSAGES[toCategory(e)]);
    }
    setBusy(false);
  }

  async function revoke(id: string) {
    setBusy(true);
    setError(null);
    try {
      await syncRevokeShare(localId, id);
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

  const active = (shares ?? []).filter((s) => s.active);

  return (
    <section aria-labelledby={`share-${localId}`} className="flex flex-col gap-2">
      <h3 id={`share-${localId}`} className="text-[13px] font-semibold">
        Share
      </h3>
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="secondary" disabled={busy} onClick={() => void create()}>
          {busy ? "Working…" : "Create share link"}
        </Button>
        <span className="text-[12px] text-secondary-text">
          {active.length === 0 ? "No active links." : `${active.length} active ${active.length === 1 ? "link" : "links"}.`}
        </span>
      </div>
      {link && (
        <div className="flex flex-wrap items-center gap-2">
          <input
            readOnly
            aria-label="Share link"
            value={link}
            onFocus={(e) => e.currentTarget.select()}
            className="min-w-0 flex-1 rounded-[8px] bg-frost-100 px-3 py-2 text-[12px]"
          />
          <Button onClick={() => void copy()}>{copied ? "Copied" : "Copy link"}</Button>
          <span className="w-full text-[12px] text-secondary-text">Shown once. Copy it now.</span>
        </div>
      )}
      {active.length > 0 && (
        <ul className="flex flex-col gap-1">
          {active.map((s) => (
            <li key={s.id} className="flex items-center justify-between gap-2 text-[12px] text-secondary-text">
              <span>Created {new Date(s.createdAt).toLocaleDateString()}</span>
              <Button variant="ghost" disabled={busy} onClick={() => void revoke(s.id)} aria-label={`Stop sharing link created ${new Date(s.createdAt).toLocaleDateString()}`}>
                Stop sharing
              </Button>
            </li>
          ))}
        </ul>
      )}
      {error && (
        <p role="alert" className="text-[12px] text-critical-coral">
          {error}
        </p>
      )}
    </section>
  );
}
