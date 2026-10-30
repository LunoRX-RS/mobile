"use client";
import { useEffect, useState, type ReactNode } from "react";
import { Moon } from "lucide-react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { AMBIENT_BG, Art } from "../../components/Art";
import { Button } from "../../components/ui";
import type { Tone } from "../../lib/media";
import { syncRegister, syncSignIn, syncStatus, type ErrorCategory } from "../../services/tauri/sync";
import { CATEGORY_MESSAGES } from "../sync/SyncPanel";
import { SignInForm } from "./SignInForm";

// After a create, the sign-in step is what refuses: an existing email gets the same 201, then a wrong-password answer.
const REGISTER_REFUSED_SIGNIN =
  "This email could not be signed in with that password. If you already have an account, use Sign in.";

// Remembered on this device: after "Continue without an account" the gate stays hidden on later launches. Signing in
// from the Accounts page still works at any time.
const SKIP_KEY = "lunorx.loginSkipped";

// The cover wall: generated artwork, one composition per seed, so it reads as a library and no two tiles match.
const TONES: Tone[] = ["violet", "slate", "rose", "amber", "moss", "ink"];
const COVERS = Array.from({ length: 30 }, (_, i) => ({ seed: `lunor-gate-${i}`, tone: TONES[(i * 7) % TONES.length] }));

// Legal pages live on the website. Build-time overridable; the opener capability allows https://www.lunorx.xyz/* and https://lunorx.xyz/* only.
const TERMS_URL = process.env.NEXT_PUBLIC_TERMS_URL?.trim() || "https://www.lunorx.xyz/legal/terms/";
const PRIVACY_URL = process.env.NEXT_PUBLIC_PRIVACY_URL?.trim() || "https://www.lunorx.xyz/legal/privacy/";

// Opens in the system browser, never inside the app's webview.
function LegalLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <a
      href={href}
      target="_blank"
      rel="noreferrer"
      onClick={(e) => {
        e.preventDefault();
        openUrl(href).catch(() => window.open(href, "_blank", "noopener"));
      }}
      className="rounded-[4px] underline decoration-hairline-strong underline-offset-[3px] transition-colors hover:text-primary-text hover:decoration-current"
    >
      {children}
    </a>
  );
}

const MODES = [
  { key: "signIn", label: "Sign in" },
  { key: "register", label: "Create account" },
] as const;

function readSkipped(): boolean {
  try {
    return window.localStorage.getItem(SKIP_KEY) === "1";
  } catch {
    return false;
  }
}

function writeSkipped(value: boolean) {
  try {
    if (value) window.localStorage.setItem(SKIP_KEY, "1");
    else window.localStorage.removeItem(SKIP_KEY);
  } catch {
    /* storage unavailable: the gate simply shows again next launch */
  }
}

// Rejections are bare category strings; anything else falls back to the generic category.
function toCategory(e: unknown): ErrorCategory {
  return typeof e === "string" && Object.hasOwn(CATEGORY_MESSAGES, e) ? (e as ErrorCategory) : "internal";
}

/**
 * Full-screen sign-in shown at launch while the LunoRX account is signed out. Skippable.
 * Artwork leads: a band of covers fades into the canvas above the form; everything the user acts on sits in one
 * column. Same flow and copy as the desktop gate.
 */
export function LoginGate() {
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [mode, setMode] = useState<"signIn" | "register">("signIn");

  useEffect(() => {
    let active = true;
    if (readSkipped()) return;
    syncStatus().then(
      (s) => { if (active && s.state.state === "signedOut") setOpen(true); },
      () => { /* sync unavailable: nothing to sign in to, so no gate */ },
    );
    return () => { active = false; };
  }, []);

  if (!open) return null;

  async function submit(email: string, secret: string, antibot: string[]) {
    setBusy(true);
    setError(null);
    try {
      if (mode === "register") await syncRegister(email, secret, antibot);
      else await syncSignIn(email, secret, antibot[0]);
      setOpen(false);
    } catch (err) {
      const category = toCategory(err);
      setError(
        mode === "register" && category === "unauthenticated" ? REGISTER_REFUSED_SIGNIN : CATEGORY_MESSAGES[category],
      );
    }
    setBusy(false);
  }

  function continueWithoutAccount() {
    writeSkipped(true);
    setOpen(false);
  }

  const registering = mode === "register";

  return (
    // ln-safe-x sits here, not on the padded column below: it is unlayered CSS, so on the same element it would
    // silently replace Tailwind's px-6 and flush the whole gate against the screen edge.
    <div className="ln-safe-x fixed inset-0 z-[90] overflow-y-auto bg-night-canvas">
      <div aria-hidden="true" className="pointer-events-none fixed inset-0 overflow-hidden">
        <div className={`absolute inset-0 ${AMBIENT_BG.violet}`} />
        <div className="ln-login-wall-mask">
          <div className="ln-login-wall lx-stagger">
            {COVERS.map((c) => (
              <Art key={c.seed} seed={c.seed} tone={c.tone} grain={false} className="aspect-square w-full" />
            ))}
          </div>
        </div>
        <div className="ln-login-shade absolute inset-0" />
      </div>

      <div className="ln-safe-top ln-safe-bottom relative flex min-h-full flex-col px-6 pt-10 pb-8">
        <div className="flex items-center gap-2 text-primary-text">
          <Moon size={18} aria-hidden="true" fill="currentColor" fillOpacity={0.9} className="shrink-0" />
          <span className="text-[14px] leading-5 font-semibold tracking-[-0.01em]">LunoRX</span>
        </div>

        <main
          aria-labelledby="login-gate-title"
          className="lx-enter mt-[22vh] mb-auto flex w-full max-w-[420px] flex-col"
        >
          <h1 id="login-gate-title" className="text-[34px] leading-[38px] font-bold tracking-[-0.025em] text-balance text-primary-text">
            {registering ? "Create your LunoRX account" : "Sign in to LunoRX"}
          </h1>
          <p className="mt-3 max-w-[36ch] text-[16px] leading-[23px] text-primary-text/80">
            Sync your playlists, likes and listening stats across your devices.
          </p>

          {/* Mode switch: two equal segments on a Frost 200 track, the chosen one raised on Frost 300. */}
          <div role="radiogroup" aria-label="Account action" className="mt-8 grid grid-cols-2 gap-1 rounded-[12px] bg-frost-200 p-1">
            {MODES.map((m) => (
              <button
                key={m.key}
                type="button"
                role="radio"
                aria-checked={mode === m.key}
                disabled={busy}
                onClick={() => { setError(null); setMode(m.key); }}
                className={`ln-touch lx-press h-11 rounded-[9px] text-[14px] font-semibold transition-colors disabled:opacity-40 ${
                  mode === m.key ? "bg-frost-300 text-primary-text" : "text-secondary-text hover:text-primary-text"
                }`}
              >
                {m.label}
              </button>
            ))}
          </div>

          <div className="mt-5">
            <SignInForm idPrefix="login-gate" mode={mode} busy={busy} error={error} hideTitle fullWidthSubmit onSubmit={submit} />
          </div>

          {/* The way out, kept in the same column so it is found without scrolling around the screen. */}
          <div className="mt-8 flex flex-col gap-3 border-t border-hairline pt-6">
            <Button variant="secondary" disabled={busy} className="ln-touch h-11 w-full rounded-[12px] text-[14px]" onClick={continueWithoutAccount}>
              Continue without an account
            </Button>
            <p className="px-4 text-[13px] leading-[18px] text-secondary-text">
              Playlists stay on this device without an account. An account adds sync and sharing.
            </p>
          </div>

          <nav aria-label="Legal" className="mt-6 flex flex-wrap items-center gap-x-1.5 px-4 text-[13px] leading-[18px] text-secondary-text">
            <LegalLink href={TERMS_URL}>Terms of Service</LegalLink>
            <span aria-hidden="true">·</span>
            <LegalLink href={PRIVACY_URL}>Privacy Policy</LegalLink>
          </nav>
        </main>
      </div>
    </div>
  );
}
