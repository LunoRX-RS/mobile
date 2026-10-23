"use client";

import { useEffect, useState } from "react";
import { Button, Screen } from "../../components/ui";
import { useUi } from "../../lib/store";
import { syncRegister, syncSignIn, syncStatus, type ErrorCategory } from "../../services/tauri/sync";
import { Banner, PageRoot, SettingRow } from "../settings/rows";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import { SignInForm } from "./SignInForm";

// Its own page, so signing in is a step the user chooses and not a form inside Settings. Playlists work without it;
// uploading and sharing need it. A successful sign-in returns to the screen the user came from.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e) ? (e as ErrorCategory) : "internal";
}

export default function LoginPage() {
  const navigate = useUi((s) => s.navigate);
  const goBack = useUi((s) => s.goBack);
  const hasBack = useUi((s) => s.back.length > 0);
  const [mode, setMode] = useState<"signIn" | "register">("signIn");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [pending, setPending] = useState(0);

  useEffect(() => {
    syncStatus()
      .then((s) => setPending(s.pendingWrites))
      .catch(() => setPending(0));
  }, []);

  function leave() {
    if (hasBack) goBack();
    else navigate("accounts");
  }

  async function submit(email: string, secret: string, antibot: string[]) {
    setBusy(true);
    setError(null);
    try {
      if (mode === "register") await syncRegister(email, secret, antibot);
      else await syncSignIn(email, secret, antibot[0]);
      leave();
    } catch (err) {
      setError(CATEGORY_MESSAGES[toCategory(err)]);
    }
    setBusy(false);
  }

  const blocked =
    pending > 0 ? `Upload or discard the ${pending} unsent ${pending === 1 ? "change" : "changes"} in Sync first.` : undefined;

  return (
    <PageRoot>
      <Screen
        id="login-title"
        title={mode === "register" ? "Create account" : "Sign in"}
        caption="Needed to upload and share playlists. Playlists still work without an account."
      >
        <div className="flex max-w-[640px] flex-col gap-6">
          <div role="group" aria-label="Account action" className="flex gap-2">
            <Button variant={mode === "signIn" ? "primary" : "secondary"} aria-pressed={mode === "signIn"} onClick={() => { setMode("signIn"); setError(null); }}>
              I have an account
            </Button>
            <Button variant={mode === "register" ? "primary" : "secondary"} aria-pressed={mode === "register"} onClick={() => { setMode("register"); setError(null); }}>
              Create an account
            </Button>
          </div>

          {error && <Banner tone="warn" role="alert">{error}</Banner>}

          <SignInForm
            key={mode}
            idPrefix="login"
            mode={mode}
            busy={busy}
            blockedReason={blocked}
            error={null}
            onSubmit={submit}
          />

          <SettingRow label="Not now" hint="Go back. Playlists stay on this device.">
            <Button onClick={leave}>Continue without an account</Button>
          </SettingRow>
        </div>
      </Screen>
    </PageRoot>
  );
}
