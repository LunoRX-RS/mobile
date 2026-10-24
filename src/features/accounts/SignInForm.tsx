"use client";

import { useRef, useState, type FormEvent, type ReactNode } from "react";
import { Button } from "../../components/ui";
import { FIELD, Inset, NoteRow, SettingRow } from "../settings/rows";
import { AntibotError, antibotEnabled, useAntibot } from "./antibot";

// Full-width mode (`bare`): the field fills the row and the placeholder names it; the label stays for screen readers.
// Defined at module scope so the inputs keep their identity (and focus) across renders.
function Row({ bare, label, htmlFor, children }: { bare: boolean; label: string; htmlFor: string; children: ReactNode }) {
  return bare ? (
    <div className="px-3 py-1.5 first:pt-3 last:pb-3">
      <label htmlFor={htmlFor} className="sr-only">{label}</label>
      {children}
    </div>
  ) : (
    <SettingRow label={label} htmlFor={htmlFor}>{children}</SettingRow>
  );
}

// Shared by Accounts and Sync. The password lives in state only until submit, then the field is cleared.
// Sign in to an existing account, or create one. The backend limits passwords to 12 to 128 bytes.
export function SignInForm({
  idPrefix,
  mode = "signIn",
  busy,
  blockedReason,
  error,
  hideTitle = false,
  fullWidthSubmit = false,
  onSubmit,
}: {
  idPrefix: string;
  mode?: "signIn" | "register";
  /** The page heading already names the action, so the group title is read by screen readers only. */
  hideTitle?: boolean;
  /** A full-width large submit button under the fields, instead of the "LunoRX account" row inside the group. */
  fullWidthSubmit?: boolean;
  busy: boolean;
  blockedReason?: string;
  error: string | null;
  /** `antibot` holds the Turnstile tokens: one for sign in, two for register (it creates the account, then signs in). */
  onSubmit: (email: string, password: string, antibot: string[]) => void | Promise<void>;
}) {
  const registering = mode === "register";
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [checking, setChecking] = useState(false);
  const [checkFailed, setCheckFailed] = useState(false);
  const antibot = useAntibot();
  const [problems, setProblems] = useState<{ email?: string; password?: string }>({});
  const emailRef = useRef<HTMLInputElement>(null);
  const passwordRef = useRef<HTMLInputElement>(null);
  const eId = `${idPrefix}-email`;
  const pId = `${idPrefix}-password`;
  const errId = `${idPrefix}-error`;

  async function submit(e: FormEvent) {
    e.preventDefault();
    if (busy || checking || blockedReason) return;
    const next: { email?: string; password?: string } = {};
    if (email.trim() === "") next.email = "Enter your email address.";
    else if (!/^\S+@\S+$/.test(email.trim())) next.email = "Enter an email address like name@example.com.";
    const bytes = new TextEncoder().encode(password).length;
    if (password === "") next.password = "Enter your password.";
    else if (registering && (bytes < 12 || bytes > 128)) next.password = "Use a password of 12 to 128 characters.";
    setProblems(next);
    if (next.email) return void emailRef.current?.focus();
    if (next.password) return void passwordRef.current?.focus();
    let tokens: string[] = [];
    if (antibotEnabled) {
      setChecking(true);
      setCheckFailed(false);
      try {
        tokens = await antibot.getTokens(registering ? 2 : 1);
      } catch (e) {
        // The check page could not load (backend unreachable or not set up): send without a token and let the server
        // decide. A server that requires the check refuses, and that error is shown. A check that loaded but failed
        // or timed out stays a client-side stop.
        if (!(e instanceof AntibotError && e.message === "unavailable")) {
          setCheckFailed(true);
          return;
        }
      } finally {
        setChecking(false);
      }
    }
    const secret = password;
    setPassword(""); // not kept in state past submit
    void onSubmit(email.trim(), secret, tokens);
  }

  const submitLabel = registering
    ? busy || checking ? "Creating account…" : "Create account"
    : busy || checking ? "Signing in…" : "Sign in";

  const describe = (own: string | undefined, ownId: string) =>
    [own ? ownId : "", error ? errId : ""].filter(Boolean).join(" ") || undefined;

  return (
    <form onSubmit={submit} noValidate aria-busy={busy || checking}>
      <Inset id={`${idPrefix}-title`} title={registering ? "Create account" : "Sign in"} hideTitle={hideTitle}>
        <Row bare={fullWidthSubmit} label="Email" htmlFor={eId}>
          <input
            ref={emailRef}
            id={eId}
            type="email"
            placeholder={fullWidthSubmit ? "Email" : "name@example.com"}
            autoComplete="username"
            value={email}
            disabled={busy}
            aria-invalid={problems.email ? true : undefined}
            aria-describedby={describe(problems.email, `${eId}-problem`)}
            onChange={(e) => setEmail(e.currentTarget.value)}
            className={fullWidthSubmit ? `${FIELD} h-11 w-full text-[15px]` : `${FIELD} w-full sm:w-[320px]`}
          />
        </Row>
        {problems.email && (
          <NoteRow tone="bad" id={`${eId}-problem`} role="alert">
            {problems.email}
          </NoteRow>
        )}
        <Row bare={fullWidthSubmit} label="Password" htmlFor={pId}>
          <input
            ref={passwordRef}
            id={pId}
            type="password"
            placeholder={fullWidthSubmit ? (registering ? "Password (12 to 128 characters)" : "Password") : registering ? "12 to 128 characters" : "Your password"}
            autoComplete={registering ? "new-password" : "current-password"}
            value={password}
            disabled={busy}
            aria-invalid={problems.password ? true : undefined}
            aria-describedby={describe(problems.password, `${pId}-problem`)}
            onChange={(e) => setPassword(e.currentTarget.value)}
            className={fullWidthSubmit ? `${FIELD} h-11 w-full text-[15px]` : `${FIELD} w-full sm:w-[320px]`}
          />
        </Row>
        {problems.password && (
          <NoteRow tone="bad" id={`${pId}-problem`} role="alert">
            {problems.password}
          </NoteRow>
        )}
        {error && (
          <NoteRow tone="bad" id={errId} role="alert">
            {error}
          </NoteRow>
        )}
        {checkFailed && (
          <NoteRow tone="bad" role="alert">
            The security check could not finish. Check your connection and try again.
          </NoteRow>
        )}
        {antibot.frame && <div className="flex justify-end px-4 pb-2">{antibot.frame}</div>}
        {!fullWidthSubmit && (
          <SettingRow
            label="LunoRX account"
            hint={blockedReason ?? (registering ? "Creates a LunoRX account and signs you in." : "Press Enter to sign in.")}
            hintId={`${idPrefix}-hint`}
          >
            <Button type="submit" variant="primary" disabled={busy || checking || !!blockedReason} aria-describedby={`${idPrefix}-hint`}>
              {submitLabel}
            </Button>
          </SettingRow>
        )}
      </Inset>
      {fullWidthSubmit && (
        <div className="mt-4 flex flex-col gap-2">
          <Button
            type="submit"
            variant="primary"
            className="h-11 w-full rounded-[12px] text-[14px]"
            disabled={busy || checking || !!blockedReason}
            aria-describedby={blockedReason ? `${idPrefix}-hint` : undefined}
          >
            {submitLabel}
          </Button>
          {blockedReason && (
            <p id={`${idPrefix}-hint`} className="px-4 text-[12px] leading-4 text-secondary-text">
              {blockedReason}
            </p>
          )}
        </div>
      )}
    </form>
  );
}
