"use client";

import { RefreshCw } from "lucide-react";
import { useEffect, useState, type FormEvent } from "react";
import { SyncAllButton } from "../../components/SyncAllButton";
import { useSyncAll } from "../../lib/syncAll";
import { Button, Screen } from "../../components/ui";
import {
  syncAddLocalEntry,
  syncCreateLocalPlaylist,
  syncDiscardPending,
  syncFlushPending,
  syncListLocalPlaylists,
  syncResolveConflict,
  syncSignIn,
  syncSignOut,
  syncStatus,
  syncUploadPlaylist,
  type ErrorCategory,
  type LocalEntryDto,
  type LocalPlaylistDto,
  type Provider,
  type SyncStatusDto,
} from "../../services/tauri/sync";
import { SignInForm } from "../accounts/SignInForm";
import {
  Banner,
  FIELD,
  Inset,
  NoteRow,
  PageRoot,
  SettingRow,
  SkeletonRows,
  Status,
  StatusRow,
  syncSummary,
} from "../settings/rows";

// Fixed copy per category. Backend text never reaches the UI.
export const CATEGORY_MESSAGES: Record<ErrorCategory, string> = {
  validation: "That can't be done with the current data. Check it and try again.",
  unauthenticated: "Your session has ended or the email or password is wrong. Sign in again.",
  conflict: "This playlist changed on the server.",
  rateLimited: "Too many requests. Wait a moment and try again.",
  offline: "No connection to the server. Your local changes are kept.",
  storage: "Local sync data could not be read or saved. Restart the app.",
  queueFull: "This playlist has too many unsynced changes. Upload or discard them before adding more.",
  internal: "Something went wrong. Try again later.",
};

const SIGN_OUT_REFUSED =
  "You have changes that have not been uploaded. Upload them or discard them before signing out.";

// Rejections are bare category strings; anything else falls back to the generic category.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e)
    ? (e as ErrorCategory)
    : "internal";
}

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

function SyncAllResult() {
  const hasResult = useSyncAll((s) => s.parts.length > 0);
  return hasResult ? <SyncAllButton showResult compact /> : null;
}

export default function SyncPanel() {
  const [status, setStatus] = useState<SyncStatusDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorCategory | null>(null);
  const [refused, setRefused] = useState(false);
  const [notice, setNotice] = useState<{ warn: boolean; text: string } | null>(null);
  const [playlists, setPlaylists] = useState<LocalPlaylistDto[]>([]);
  const [newName, setNewName] = useState("");
  const [entryPlaylist, setEntryPlaylist] = useState("");
  const [entryProvider, setEntryProvider] = useState<Provider>("Spotify");
  const [entryTrack, setEntryTrack] = useState("");
  const [entryUrl, setEntryUrl] = useState("");
  const [entryTitle, setEntryTitle] = useState("");
  // Playlist whose Keep server choice is waiting for the user's confirmation.
  const [confirmKeepServer, setConfirmKeepServer] = useState<string | null>(null);
  const [confirmDiscard, setConfirmDiscard] = useState(false);

  async function load() {
    try {
      setStatus(await syncStatus());
      setPlaylists(await syncListLocalPlaylists());
    } catch (e) {
      setError(toCategory(e));
    }
  }

  useEffect(() => {
    void load();
  }, []);

  // Runs one action, then re-reads status so the view matches the engine. Returns the failure category, if any.
  async function run(action: () => Promise<unknown>): Promise<ErrorCategory | null> {
    setBusy(true);
    setError(null);
    setRefused(false);
    setNotice(null);
    let failed: ErrorCategory | null = null;
    try {
      await action();
    } catch (e) {
      failed = toCategory(e);
    }
    setBusy(false);
    setError(failed);
    await load();
    return failed;
  }

  async function signIn(email: string, secret: string, antibot: string[]) {
    await run(() => syncSignIn(email, secret, antibot[0]));
  }

  async function signOut() {
    const failed = await run(async () => {
      const { warning } = await syncSignOut();
      if (warning) setNotice({ warn: true, text: "Signed out on this device, but the server did not confirm the sign-out." });
    });
    if (failed === "validation") {
      setError(null);
      setRefused(true);
    }
  }

  // Clears the field only when the action succeeded, so a rejected entry can be corrected.
  async function createPlaylist(e: FormEvent) {
    e.preventDefault();
    const failed = await run(() => syncCreateLocalPlaylist(newName.trim(), null));
    if (!failed) setNewName("");
  }

  async function addEntry(e: FormEvent) {
    e.preventDefault();
    const entry: LocalEntryDto = {
      provider: entryProvider,
      providerTrackId: entryTrack.trim(),
      // Empty for YouTube is filled in by the engine from the track id.
      providerUrl: entryUrl.trim(),
      // Spotify entries carry ids only; the backend rejects a title on them.
      title: entryProvider === "YouTube" ? entryTitle.trim() || null : null,
      artists: null,
      artworkUrl: null,
      durationMs: null,
    };
    const failed = await run(() => syncAddLocalEntry(entryPlaylist, entry));
    if (!failed) {
      setEntryTrack("");
      setEntryUrl("");
      setEntryTitle("");
    }
  }

  async function discardPending() {
    setConfirmDiscard(false);
    await run(async () => {
      const count = await syncDiscardPending();
      setNotice({ warn: false, text: `Discarded ${plural(count, "pending write", "pending writes")}.` });
    });
  }

  async function flushPending() {
    await run(async () => {
      const count = await syncFlushPending();
      setNotice({ warn: false, text: `Sent ${plural(count, "pending write", "pending writes")} to the server.` });
    });
  }

  const header = (
    <div className="flex items-center gap-2">
      <SyncAllButton />
      <Button onClick={() => void load()} disabled={busy} aria-label="Refresh sync status">
        <RefreshCw size={14} aria-hidden="true" />
        Refresh
      </Button>
    </div>
  );

  if (!status) {
    return (
      <PageRoot>
        <Screen id="sync-title" title="Sync" caption="Keep local playlists in step with your LUNOR-X account.">
          {error ? (
            <Inset id="sync-failed-title" title="Status">
              <NoteRow tone="bad" role="alert">
                {CATEGORY_MESSAGES[error]}
              </NoteRow>
              <SettingRow label="Try again">
                <Button
                  onClick={() => {
                    setError(null);
                    void load();
                  }}
                >
                  Retry
                </Button>
              </SettingRow>
            </Inset>
          ) : (
            <div role="status" aria-label="Loading sync">
              <Inset id="sync-loading-title" title="Status">
                <SkeletonRows rows={3} />
              </Inset>
            </div>
          )}
        </Screen>
      </PageRoot>
    );
  }

  const s = status.state;
  const pending = status.pendingWrites;
  const signedOut = s.state === "signedOut";
  const summary = syncSummary(s);
  const conflictCount = Math.max(status.conflicts.length, s.state === "conflict" ? 1 : 0);
  const stateHint: string = (() => {
    switch (s.state) {
      case "signedOut":
        return "Sign in to sync playlists.";
      case "syncing":
        return "Sending and receiving changes.";
      case "synced":
        return "All changes are synced.";
      case "offline":
        return "Your local edits are kept on this device and will sync when the connection returns.";
      case "rateLimited":
        return s.retryAfterSecs === null ? "Too many requests. Try again later." : `Too many requests. Try again in ${s.retryAfterSecs} seconds.`;
      case "conflict":
        return "A playlist changed on the server while you had unsent changes.";
      case "error":
        return CATEGORY_MESSAGES[s.kind];
    }
  })();

  // Counts for the conflicted playlist only: that is what Keep server discards.
  const target = s.state === "conflict" ? playlists.find((p) => p.remoteId === s.playlistId) : undefined;
  const writes = target?.queuedWrites ?? 0;
  const tail = target?.unsentEntries ?? 0;

  return (
    <PageRoot>
      <Screen id="sync-title" title="Sync" caption="Keep local playlists in step with your LUNOR-X account." actions={header}>
        <div className="flex flex-col gap-8">
          <SyncAllResult />
          {notice && (
            <Banner tone={notice.warn ? "warn" : "ok"} role="status">
              {notice.text}
            </Banner>
          )}
          {error && (
            <Banner tone="bad" role="alert">
              {CATEGORY_MESSAGES[error]}
            </Banner>
          )}

          <Inset id="sync-status-title" title="Status">
            <StatusRow label="Sync" tone={summary.tone} status={summary.text} hint={stateHint} />
            <SettingRow label="Pending writes">
              <Status tone={pending > 0 ? "warn" : "ok"}>{pending === 0 ? "None" : pending}</Status>
            </SettingRow>
            <SettingRow label="Conflicts">
              <Status tone={conflictCount > 0 ? "warn" : "ok"}>{conflictCount === 0 ? "None" : conflictCount}</Status>
            </SettingRow>
          </Inset>

          {s.state === "conflict" && (
            <Inset id="sync-conflict-title" title="Resolve conflict">
              <NoteRow tone="warn">
                {target ? `“${target.name}”` : `Playlist ${s.playlistId}`} changed on the server while you had unsent
                changes. Choose which version to keep.
              </NoteRow>
              <SettingRow label="Keep mine" hint="Applies your unsent changes on top of the server’s current version.">
                <Button
                  disabled={busy}
                  onClick={() => void run(() => syncResolveConflict(s.playlistId, "keepMine"))}
                >
                  Keep mine
                </Button>
              </SettingRow>
              {confirmKeepServer === s.playlistId ? (
                <>
                  <NoteRow tone="warn" id="sync-keep-server-warning" role="alert">
                    Keep server replaces this playlist with the server copy. For this playlist only,{" "}
                    {plural(writes, "unsent change", "unsent changes")} and{" "}
                    {plural(tail, "not-yet-uploaded entry", "not-yet-uploaded entries")} will be discarded. Other
                    playlists are not affected.
                  </NoteRow>
                  <SettingRow label="Confirm keep server">
                    <Button onClick={() => setConfirmKeepServer(null)} disabled={busy}>
                      Cancel
                    </Button>
                    <Button
                      variant="primary"
                      disabled={busy}
                      aria-describedby="sync-keep-server-warning"
                      onClick={() => {
                        setConfirmKeepServer(null);
                        void run(() => syncResolveConflict(s.playlistId, "keepServer"));
                      }}
                    >
                      Discard unsent changes and keep server
                    </Button>
                  </SettingRow>
                </>
              ) : (
                <SettingRow label="Keep server" hint="Replaces this playlist with the server copy. Your unsent changes for it are discarded.">
                  <Button disabled={busy} onClick={() => setConfirmKeepServer(s.playlistId)}>
                    Keep server…
                  </Button>
                </SettingRow>
              )}
            </Inset>
          )}

          {signedOut && (
            <SignInForm
              idPrefix="sync"
              busy={busy}
              error={null}
              blockedReason={pending > 0 ? "Sign in is blocked until you discard the unsent changes below." : undefined}
              onSubmit={signIn}
            />
          )}

          {(!signedOut || pending > 0) && (
            <Inset id="sync-actions-title" title="Actions">
              {!signedOut && (
                <SettingRow
                  label="Flush pending writes"
                  hint={pending === 0 ? "Nothing is waiting to upload." : `Send ${plural(pending, "queued change", "queued changes")} to the server now.`}
                >
                  <Button disabled={busy || pending === 0} onClick={() => void flushPending()}>
                    Flush pending
                  </Button>
                </SettingRow>
              )}
              {confirmDiscard && pending > 0 ? (
                <>
                  <NoteRow tone="warn" id="sync-discard-warning" role="alert">
                    Discard {plural(pending, "unsent change", "unsent changes")}? They are removed from this device
                    and cannot be recovered.
                  </NoteRow>
                  <SettingRow label="Confirm discard">
                    <Button onClick={() => setConfirmDiscard(false)} disabled={busy}>
                      Cancel
                    </Button>
                    <Button variant="primary" disabled={busy} aria-describedby="sync-discard-warning" onClick={() => void discardPending()}>
                      Discard {pending}
                    </Button>
                  </SettingRow>
                </>
              ) : (
                <SettingRow
                  label="Discard pending writes"
                  hint={pending === 0 ? "Nothing to discard." : `Removes ${plural(pending, "unsent change", "unsent changes")} from this device.`}
                >
                  <Button disabled={busy || pending === 0} onClick={() => setConfirmDiscard(true)}>
                    Discard…
                  </Button>
                </SettingRow>
              )}
              {!signedOut && refused && (
                <NoteRow tone="warn" role="alert">
                  {SIGN_OUT_REFUSED}
                </NoteRow>
              )}
              {!signedOut && (
                <SettingRow label="Sign out" hint="Signs out of the LUNOR-X backend on this device.">
                  <Button disabled={busy} onClick={() => void signOut()}>
                    Sign out
                  </Button>
                </SettingRow>
              )}
            </Inset>
          )}

          <Inset id="sync-playlists-title" title="Local playlists">
            {playlists.length === 0 ? (
              <SettingRow label="No local playlists yet." hint="Create one below, then upload it to the server." />
            ) : (
              playlists.map((p) => (
                <SettingRow key={p.id} label={p.name} hint={plural(p.entryCount, "entry", "entries")}>
                  {p.remoteId && p.unsentEntries > 0 ? (
                    <>
                      <Status tone="warn">
                        Partly uploaded ({p.uploaded} of {p.entryCount})
                      </Status>
                      <Button disabled={busy} aria-label={`Resume upload of ${p.name}`} onClick={() => void run(() => syncUploadPlaylist(p.id))}>
                        Resume upload
                      </Button>
                    </>
                  ) : p.remoteId && p.queuedWrites > 0 ? (
                    // Queued adds are not on the server until the queue replays.
                    <Status tone="warn">
                      Pending: {plural(p.queuedWrites, "change", "changes")} not yet on the server
                    </Status>
                  ) : p.remoteId ? (
                    <Status tone="ok">On the server</Status>
                  ) : (
                    <>
                      <Status tone="off">Local only</Status>
                      <Button disabled={busy} aria-label={`Upload ${p.name} to server`} onClick={() => void run(() => syncUploadPlaylist(p.id))}>
                        Upload to server
                      </Button>
                    </>
                  )}
                </SettingRow>
              ))
            )}
          </Inset>

          <form onSubmit={createPlaylist}>
            <Inset id="sync-new-title" title="New playlist">
              <SettingRow label="Name" htmlFor="sync-new-playlist">
                <input
                  id="sync-new-playlist"
                  value={newName}
                  onChange={(e) => setNewName(e.currentTarget.value)}
                  className={`${FIELD} w-[240px]`}
                />
                <Button type="submit" variant="primary" disabled={busy || newName.trim() === ""}>
                  Create playlist
                </Button>
              </SettingRow>
            </Inset>
          </form>

          {playlists.length > 0 && (
            <details className="group">
              <summary className="flex min-h-8 w-fit cursor-pointer list-none items-center rounded-[8px] px-4 text-[13px] font-semibold text-secondary-text select-none hover:bg-frost-100 hover:text-primary-text [&::-webkit-details-marker]:hidden">
                Advanced: add an entry by track ID
              </summary>
              <form onSubmit={addEntry} className="mt-3">
                <Inset id="sync-entry-title" title="Add entry">
                  <SettingRow label="Playlist" htmlFor="sync-entry-playlist">
                    <select
                      id="sync-entry-playlist"
                      value={entryPlaylist}
                      onChange={(e) => setEntryPlaylist(e.currentTarget.value)}
                      className={`${FIELD} w-[240px]`}
                    >
                      <option value="">Choose a playlist</option>
                      {playlists.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                  </SettingRow>
                  <SettingRow label="Provider" htmlFor="sync-entry-provider">
                    <select
                      id="sync-entry-provider"
                      value={entryProvider}
                      onChange={(e) => setEntryProvider(e.currentTarget.value as Provider)}
                      className={`${FIELD} w-[240px]`}
                    >
                      <option value="Spotify">Spotify</option>
                      <option value="YouTube">YouTube</option>
                    </select>
                  </SettingRow>
                  <SettingRow label="Track ID" htmlFor="sync-entry-track">
                    <input id="sync-entry-track" value={entryTrack} onChange={(e) => setEntryTrack(e.currentTarget.value)} className={`${FIELD} w-[240px]`} />
                  </SettingRow>
                  <SettingRow
                    label="Track URL"
                    htmlFor="sync-entry-url"
                    hint={entryProvider === "Spotify" ? "Required for Spotify." : "Optional for YouTube."}
                  >
                    <input id="sync-entry-url" value={entryUrl} onChange={(e) => setEntryUrl(e.currentTarget.value)} className={`${FIELD} w-[240px]`} />
                  </SettingRow>
                  {entryProvider === "YouTube" && (
                    <SettingRow label="Title" htmlFor="sync-entry-title-input">
                      <input
                        id="sync-entry-title-input"
                        value={entryTitle}
                        onChange={(e) => setEntryTitle(e.currentTarget.value)}
                        className={`${FIELD} w-[240px]`}
                      />
                    </SettingRow>
                  )}
                  <SettingRow label="Add to playlist">
                    <Button
                      type="submit"
                      variant="primary"
                      disabled={
                        busy ||
                        entryPlaylist === "" ||
                        entryTrack.trim() === "" ||
                        (entryProvider === "Spotify" && entryUrl.trim() === "")
                      }
                    >
                      Add entry
                    </Button>
                  </SettingRow>
                </Inset>
              </form>
            </details>
          )}
        </div>
      </Screen>
    </PageRoot>
  );
}
