"use client";

import { useCallback, useEffect, useRef, useState } from "react";
import { Minus, Plus } from "lucide-react";
import { usePlayback, togglePlayback } from "../lib/playback";
import {
  CAL_BEEPS, CAL_GOOD, CAL_MAX_DELTA, CAL_SKIP, DEFAULT_OFFSET_MS, OFFSET_MAX, OFFSET_MIN, beepPerfTime, beepSchedule, calibrationStats, clampOffset,
  matchTaps, parseCalibration, type CalibrationResult, type StoredCalibration,
} from "../lib/lyricMaker";

export const CAL_KEY = "lunorx.lyricMakerCalibration";

export function readCalibration(): StoredCalibration | null {
  try { return parseCalibration(window.localStorage.getItem(CAL_KEY)); } catch { return null; }
}
export function writeCalibration(c: StoredCalibration): void {
  try { window.localStorage.setItem(CAL_KEY, JSON.stringify(c)); } catch { /* storage unavailable: the numbers last for this window */ }
}

/** The delay the maker subtracts from every stamp, with the calibration it came from. */
export function useCalibration(): { cal: StoredCalibration | null; offsetMs: number; apply: (c: StoredCalibration) => void; setManual: (ms: number) => void } {
  const [cal, setCal] = useState<StoredCalibration | null>(readCalibration);
  const apply = useCallback((c: StoredCalibration) => { setCal(c); writeCalibration(c); }, []);
  const setManual = useCallback((ms: number) => {
    const next: StoredCalibration = { offsetMs: clampOffset(ms), consistency: null, jitterMs: null, date: new Date().toISOString().slice(0, 10), manual: true };
    setCal(next); writeCalibration(next);
  }, []);
  return { cal, offsetMs: cal ? cal.offsetMs : DEFAULT_OFFSET_MS, apply, setManual };
}

/** "Delay 238 ms · Consistency 91%" or the honest default. */
export function calibrationLabel(cal: StoredCalibration | null): string {
  if (!cal) return `Not calibrated: using ${DEFAULT_OFFSET_MS} ms`;
  if (cal.manual || cal.consistency === null) return `Delay ${cal.offsetMs} ms · set by hand`;
  return `Delay ${cal.offsetMs} ms · Consistency ${Math.round(cal.consistency * 100)}%`;
}

const percent = (v: number) => `${Math.round(v * 100)}%`;

type Phase = "intro" | "starting" | "running" | "result" | "unsupported";

/** One 60 ms 880 Hz beep at audio-context time `when` (s), with a short ramp so it does not click. */
function beep(ctx: AudioContext, when: number): void {
  const osc = ctx.createOscillator();
  const gain = ctx.createGain();
  osc.type = "sine";
  osc.frequency.value = 880;
  gain.gain.setValueAtTime(0, when);
  gain.gain.linearRampToValueAtTime(0.35, when + 0.008);
  gain.gain.linearRampToValueAtTime(0, when + 0.06);
  osc.connect(gain).connect(ctx.destination);
  osc.start(when);
  osc.stop(when + 0.08);
}

function HoldStep({ label, delta, onStep, children }: { label: string; delta: number; onStep: (d: number) => void; children: React.ReactNode }) {
  const timer = useRef(0);
  const stop = () => { window.clearTimeout(timer.current); window.clearInterval(timer.current); };
  useEffect(() => stop, []);
  const start = () => {
    stop();
    onStep(delta);
    timer.current = window.setTimeout(() => { timer.current = window.setInterval(() => onStep(delta), 70); }, 400);
  };
  return (
    <button type="button" aria-label={label} title={label} onPointerDown={(e) => { if (e.button === 0) start(); }} onPointerUp={stop} onPointerLeave={stop} onPointerCancel={stop} onBlur={stop}
      onClick={(e) => { if (e.detail === 0) onStep(delta); }} className="ln-mk-step">
      {children}
    </button>
  );
}

/**
 * 12 beeps, one Space press each. The beeps run through the browser audio path, the song through the native player,
 * so the result is an estimate of the user's own reaction delay, not a measurement of the whole chain.
 */
export function CalibrateSheet({ cal, offsetMs, onApply, onManual, onClose }: {
  cal: StoredCalibration | null; offsetMs: number; onApply: (c: StoredCalibration) => void; onManual: (ms: number) => void; onClose: () => void;
}) {
  const [phase, setPhase] = useState<Phase>("intro");
  const [taps, setTaps] = useState(0);
  const [result, setResult] = useState<CalibrationResult | null>(null);
  const root = useRef<HTMLDivElement>(null);
  const ctxRef = useRef<AudioContext | null>(null);
  const beeps = useRef<number[]>([]);
  const tapTimes = useRef<number[]>([]);
  const timer = useRef(0);
  const pausedByUs = useRef(false);

  const cleanup = useCallback(() => {
    window.clearTimeout(timer.current);
    const ctx = ctxRef.current;
    ctxRef.current = null;
    if (ctx) void ctx.close().catch(() => undefined);
    if (pausedByUs.current) { pausedByUs.current = false; void togglePlayback(); }
  }, []);
  useEffect(() => { root.current?.focus(); return cleanup; }, [cleanup]);

  const finish = useCallback(() => {
    const r = calibrationStats(matchTaps(beeps.current, tapTimes.current));
    cleanup();
    setResult(r);
    setPhase("result");
    if (r.ok) onApply({ offsetMs: r.offsetMs, consistency: r.consistency, jitterMs: r.jitterMs, date: new Date().toISOString().slice(0, 10), manual: false });
    root.current?.focus();
  }, [cleanup, onApply]);

  const start = async () => {
    const AC = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AC) { setPhase("unsupported"); return; }
    setPhase("starting");
    setTaps(0);
    tapTimes.current = [];
    // The beeps play alone: pause the song and bring it back afterwards.
    if (usePlayback.getState().player.state === "playing") { pausedByUs.current = true; void togglePlayback(); }
    try {
      const ctx = new AC();
      ctxRef.current = ctx;
      await ctx.resume();
      const out = typeof ctx.getOutputTimestamp === "function" ? ctx.getOutputTimestamp() : undefined;
      const clock = { ctxNow: ctx.currentTime, perfNow: performance.now(), baseLatency: ctx.baseLatency, outputLatency: (ctx as unknown as { outputLatency?: number }).outputLatency, out };
      const base = ctx.currentTime + 0.4;
      const schedule = beepSchedule(Date.now() & 0xffff);
      beeps.current = schedule.map((ms) => { beep(ctx, base + ms / 1000); return beepPerfTime(base + ms / 1000, clock); });
      setPhase("running");
      root.current?.focus();
      timer.current = window.setTimeout(finish, (base - ctx.currentTime) * 1000 + schedule[schedule.length - 1] + CAL_MAX_DELTA + 400);
    } catch { cleanup(); setPhase("unsupported"); }
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === " " && phase === "running") {
      e.preventDefault();
      if (!e.repeat) { tapTimes.current.push(e.timeStamp); setTaps((n) => Math.min(CAL_BEEPS, n + 1)); }
    }
  };

  const R = result;
  return (
    <div className="ln-mk-scrim">
      <div ref={root} tabIndex={-1} role="dialog" aria-modal="true" aria-labelledby="mk-cal-title" onKeyDown={onKeyDown} className="ln-mk-sheet">
        <h2 id="mk-cal-title" className="font-display text-[22px] font-semibold tracking-[-0.01em]">Calibrate your delay</h2>
        {phase === "intro" && (
          <>
            <p className="mt-2 text-[14px] leading-5 text-secondary-text">
              You will hear {CAL_BEEPS} short beeps, about 20 seconds in all. Press Space once for each beep. The first {CAL_SKIP} are practice. The song pauses while this runs.
            </p>
            <div className="mt-5 flex gap-2">
              <button type="button" onClick={() => void start()} className="ln-mk-btn ln-mk-btn-solid">Start the beeps</button>
              <button type="button" onClick={onClose} className="ln-mk-btn">Cancel</button>
            </div>
          </>
        )}
        {(phase === "starting" || phase === "running") && (
          <>
            <p role="status" className="mt-2 text-[14px] leading-5 text-secondary-text">
              {phase === "starting" ? "Starting the audio…" : "Press Space on each beep. If you miss one, carry on with the next."}
            </p>
            <p className="mt-4 text-[34px] font-semibold tabular-nums">{taps} <span className="text-[16px] font-normal text-secondary-text">of {CAL_BEEPS} taps</span></p>
            <div className="mt-5"><button type="button" onClick={onClose} className="ln-mk-btn">Stop</button></div>
          </>
        )}
        {phase === "unsupported" && (
          <>
            <p role="alert" className="mt-2 text-[14px] leading-5 text-critical-coral">This window cannot play the test beeps. Set the delay by hand below.</p>
            <div className="mt-5"><button type="button" onClick={onClose} className="ln-mk-btn">Close</button></div>
          </>
        )}
        {phase === "result" && R && (
          <>
            {R.ok ? (
              <div role="status" className="mt-3 space-y-1.5">
                <p className="text-[17px] font-semibold">Your typical delay: <span className="tabular-nums">{R.offsetMs} ms</span> <span className="text-[14px] font-normal text-secondary-text">(applied automatically)</span></p>
                <p className="text-[15px]">Consistency: <span className="tabular-nums">{percent(R.consistency)}</span> of your taps land within 300 ms of the typical delay.</p>
                <p className="text-[15px] font-semibold">{R.consistency >= CAL_GOOD ? "Good enough for a reliable file." : "Taps are inconsistent. Try again, or use Tap along on a calmer song. Slowing the song down is not available yet."}</p>
                <p className="text-[12px] leading-4 text-secondary-text">Based on {R.used} usable taps out of {R.counted} counted beeps. Inconsistent taps mean some lines will land more than 300 ms off.</p>
              </div>
            ) : (
              <p role="alert" className="mt-3 text-[15px]">Only {R.used} of {R.counted} counted beeps had a usable tap, so nothing was changed. Make sure the sound is on, then try again.</p>
            )}
            <div className="mt-5 flex gap-2">
              <button type="button" onClick={() => { setResult(null); setPhase("intro"); }} className="ln-mk-btn">Try again</button>
              <button type="button" onClick={onClose} className="ln-mk-btn ln-mk-btn-solid">Done</button>
            </div>
          </>
        )}
        {(phase === "intro" || phase === "result" || phase === "unsupported") && (
          <div className="mt-6 border-t border-hairline pt-4">
            <div role="group" aria-label="Delay" className="flex items-center justify-between gap-4">
              <div>
                <p className="text-[13px] font-semibold">Delay</p>
                <p className="text-[12px] leading-4 text-secondary-text">Taken off every stamp. {cal ? "" : `Using ${DEFAULT_OFFSET_MS} ms until you calibrate.`}</p>
              </div>
              <div className="flex items-center gap-1">
                <HoldStep label="Decrease delay by 10 ms" delta={-10} onStep={(d) => onManual(offsetMs + d)}><Minus size={16} aria-hidden="true" /></HoldStep>
                <output aria-label="Delay in milliseconds" className="w-[72px] text-center text-[17px] font-semibold tabular-nums">{offsetMs} ms</output>
                <HoldStep label="Increase delay by 10 ms" delta={10} onStep={(d) => onManual(offsetMs + d)}><Plus size={16} aria-hidden="true" /></HoldStep>
              </div>
            </div>
            <p className="mt-2 text-[12px] leading-4 text-secondary-text">
              Range {OFFSET_MIN} to +{OFFSET_MAX} ms. The beeps go through the browser audio path and the song through the native player, so output delay can differ a little; the stepper fixes any remainder. For a single line, use Back 5 s and stamp it again.
            </p>
          </div>
        )}
      </div>
    </div>
  );
}
