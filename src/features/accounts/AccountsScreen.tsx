"use client";

import { useEffect, useState } from "react";
import { Button, Screen } from "../../components/ui";
import { useUi } from "../../lib/store";
import {
  syncSignOut,
  syncStatus,
  type ErrorCategory,
  type SyncStatusDto,
} from "../../services/tauri/sync";
import { Banner, Inset, NoteRow, PageRoot, SettingRow, SkeletonRows, Status, StatusRow, syncSummary } from "../settings/rows";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";

const SIGN_OUT_REFUSED =
  "You have changes that have not been uploaded. Upload them or discard them in Sync before signing out.";

// Rejections are bare category strings; anything else falls back to the generic category.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e) ? (e as ErrorCategory) : "internal";
}

export default function AccountsScreen() {
  const navigate = useUi((s) => s.navigate);
  const [status, setStatus] = useState<SyncStatusDto | null>(null);
  const [loadFailed, setLoadFailed] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<ErrorCategory | null>(null);
  const [notice, setNotice] = useState<{ warn: boolean; text: string } | null>(null);

  async function reload() {
    try {
      setStatus(await syncStatus());
      setLoadFailed(false);
    } catch {
      setLoadFailed(true);
    }
  }

  useEffect(() => {
    void reload();
  }, []);

  async function signOut() {
    setBusy(true);
    setError(null);
    setNotice(null);
    try {
      const { warning } = await syncSignOut();
      setNotice(
        warning
          ? { warn: true, text: "Signed out on this device. The server did not confirm the sign-out." }
          : { warn: false, text: "Signed out." },
      );
    } catch (err) {
      setError(toCategory(err));
    }
    setBusy(false);
    await reload();
  }

  const signedIn = status !== null && status.state.state !== "signedOut";
  const summary = status ? syncSummary(status.state) : null;
  const errorText = error === null ? null : error === "validation" ? SIGN_OUT_REFUSED : CATEGORY_MESSAGES[error];

  return (
    <PageRoot>
      <Screen
        id="accounts-title"
        title="Accounts"
        caption="Sign in to the LUNOR-X backend to sync playlists. This is not a music provider connection."
      >
        <div className="flex flex-col gap-8">
          {notice && (
            <Banner tone={notice.warn ? "warn" : "ok"} role="status">
              {notice.text}
            </Banner>
          )}

          {status === null && !loadFailed && (
            <Inset id="accounts-loading-title" title="Account">
              <SkeletonRows rows={2} />
            </Inset>
          )}

          {loadFailed && status === null && (
            <Inset id="accounts-failed-title" title="Account">
              <NoteRow tone="bad" role="alert">
                Could not read account status.
              </NoteRow>
              <SettingRow label="Try again">
                <Button onClick={() => void reload()}>Retry</Button>
              </SettingRow>
            </Inset>
          )}

          {status !== null && !signedIn && (
            <>
              {status.pendingWrites > 0 && (
                <Inset id="accounts-pending-title" title="This device">
                  <NoteRow tone="warn">
                    {status.pendingWrites} unsent {status.pendingWrites === 1 ? "change" : "changes"} on this device.
                    Open Sync to upload or discard them before signing in.
                  </NoteRow>
                  <SettingRow label="Unsent changes">
                    <Button onClick={() => navigate("sync")}>Open Sync</Button>
                  </SettingRow>
                </Inset>
              )}
              <Inset id="accounts-login-title" title="Account">
                <SettingRow label="Sign in or create an account" hint="Needed to upload and share playlists. Playlists still work without one.">
                  <Button onClick={() => navigate("login")}>Continue</Button>
                </SettingRow>
              </Inset>
            </>
          )}

          {status !== null && signedIn && summary && (
            <Inset id="accounts-signed-in-title" title="Account">
              <StatusRow label="Account" tone="ok" status="Signed in" hint="The status command does not report a user ID." />
              <StatusRow label="Sync state" tone={summary.tone} status={summary.text} />
              <SettingRow label="Unsent changes">
                <Status tone={status.pendingWrites > 0 ? "warn" : "ok"}>
                  {status.pendingWrites === 0 ? "None" : status.pendingWrites}
                </Status>
              </SettingRow>
              {errorText && (
                <NoteRow tone="bad" id="accounts-signout-error" role="alert">
                  {errorText}
                </NoteRow>
              )}
              <SettingRow label="Sign out" hint={status.pendingWrites > 0 ? "Blocked while changes are unsent. Upload or discard them in Sync first." : "Signs out of the LUNOR-X backend on this device."}>
                <Button onClick={() => navigate("sync")}>Open Sync</Button>
                <Button
                  disabled={busy}
                  onClick={() => void signOut()}
                  aria-describedby={errorText ? "accounts-signout-error" : undefined}
                >
                  {busy ? "Signing out…" : "Sign out"}
                </Button>
              </SettingRow>
            </Inset>
          )}
        </div>
      </Screen>
    </PageRoot>
  );
}
