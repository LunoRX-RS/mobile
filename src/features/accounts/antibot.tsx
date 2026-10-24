"use client";

import { useCallback, useEffect, useRef, useState, type ReactNode } from "react";

// Cloudflare Turnstile runs in an iframe served by the LunoRX backend, never in this window: Cloudflare's script
// must not share an origin with the app's native commands. The frame posts a one-use token back.
// Unset at build time: no antibot, and the sign-in form works as before (the server then must not require one).
const FRAME_URL = process.env.NEXT_PUBLIC_ANTIBOT_URL?.trim() || "";
const SOURCE = "antibot-frame";
const SOLVE_TIMEOUT_MS = 120_000;
const READY_TIMEOUT_MS = 15_000;

export const antibotEnabled = FRAME_URL !== "";

export class AntibotError extends Error {}

interface Pending {
  id: string;
  resolve: (token: string) => void;
  reject: (e: AntibotError) => void;
  timer: number;
}

/** The frame element to render, and `getTokens(n)` which resolves n fresh tokens, one solve at a time. */
export function useAntibot(): { frame: ReactNode; getTokens: (n: number) => Promise<string[]> } {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const readyRef = useRef<{ promise: Promise<void>; resolve: () => void; reject: (e: AntibotError) => void } | null>(null);
  const pendingRef = useRef<Pending | null>(null);
  const [interactive, setInteractive] = useState(false);
  // Bumped to build a new frame when the current one never became ready.
  const [generation, setGeneration] = useState(0);

  const freshReady = () => {
    let resolve!: () => void;
    let reject!: (e: AntibotError) => void;
    const promise = new Promise<void>((res, rej) => { resolve = res; reject = rej; });
    promise.catch(() => undefined); // surfaced through getTokens; never an unhandled rejection
    return { promise, resolve, reject };
  };
  if (readyRef.current === null) readyRef.current = freshReady();

  useEffect(() => {
    if (!antibotEnabled) return;
    const origin = new URL(FRAME_URL).origin;
    const failPending = (e: AntibotError) => {
      const p = pendingRef.current;
      if (!p) return;
      window.clearTimeout(p.timer);
      pendingRef.current = null;
      p.reject(e);
    };
    const onMessage = (event: MessageEvent) => {
      // Only the frame we created, from the backend's origin.
      if (event.source !== frameRef.current?.contentWindow || event.origin !== origin) return;
      const d: unknown = event.data;
      if (typeof d !== "object" || d === null || (d as { source?: unknown }).source !== SOURCE) return;
      const m = d as { type?: unknown; token?: unknown; id?: unknown };
      const ready = readyRef.current;
      if (m.type === "ready") ready?.resolve();
      else if (m.type === "interactive") setInteractive(true);
      else if (m.type === "settled") setInteractive(false);
      else if (m.type === "error") {
        // Without an id the frame failed to set up (unconfigured or script load): nothing can be solved from now on.
        // With an id only that one solve failed; the frame stays usable so the user can try again.
        if (m.id === undefined) {
          ready?.reject(new AntibotError("unavailable"));
          failPending(new AntibotError("unavailable"));
        } else if (pendingRef.current?.id === m.id) {
          failPending(new AntibotError("failed"));
        }
      } else if (m.type === "token") {
        const p = pendingRef.current;
        if (p && m.id === p.id && typeof m.token === "string" && m.token !== "") {
          window.clearTimeout(p.timer);
          pendingRef.current = null;
          setInteractive(false);
          p.resolve(m.token);
        }
      }
    };
    window.addEventListener("message", onMessage);
    return () => {
      window.removeEventListener("message", onMessage);
      failPending(new AntibotError("closed"));
    };
  }, []);

  const solveOne = useCallback(async (): Promise<string> => {
    const ready = readyRef.current;
    if (!ready) throw new AntibotError("unavailable");
    await Promise.race([
      ready.promise,
      new Promise<never>((_, rej) => window.setTimeout(() => rej(new AntibotError("unavailable")), READY_TIMEOUT_MS)),
    ]);
    const target = frameRef.current?.contentWindow;
    if (!target) throw new AntibotError("unavailable");
    return new Promise<string>((resolve, reject) => {
      const id = crypto.randomUUID();
      const timer = window.setTimeout(() => {
        pendingRef.current = null;
        reject(new AntibotError("timeout"));
      }, SOLVE_TIMEOUT_MS);
      pendingRef.current = { id, resolve, reject, timer };
      target.postMessage({ source: SOURCE, type: "run", id }, new URL(FRAME_URL).origin);
    });
  }, []);

  const getTokens = useCallback(
    async (n: number) => {
      const out: string[] = [];
      for (let i = 0; i < n; i += 1) {
        try {
          out.push(await solveOne());
        } catch (e) {
          // A frame that never became ready (its "ready" message can be missed) gets one new frame, then one more try.
          if (!(e instanceof AntibotError && e.message === "unavailable") || i > 0) throw e;
          readyRef.current = freshReady();
          // The old frame must not answer for the new one: its messages no longer match `frameRef`.
          frameRef.current = null;
          setGeneration((g) => g + 1);
          out.push(await solveOne());
        }
      }
      return out;
    },
    [solveOne],
  );

  const frame = antibotEnabled ? (
    <iframe
      key={generation}
      ref={frameRef}
      src={FRAME_URL}
      title="Security check"
      // The frame only needs its own scripts. No same-origin access to this app, no forms, no navigation of us.
      sandbox="allow-scripts allow-same-origin"
      referrerPolicy="no-referrer"
      tabIndex={interactive ? 0 : -1}
      aria-hidden={interactive ? undefined : true}
      className={interactive ? "block h-[70px] w-[310px] border-0" : "pointer-events-none absolute size-0 border-0"}
    />
  ) : null;

  return { frame, getTokens };
}
