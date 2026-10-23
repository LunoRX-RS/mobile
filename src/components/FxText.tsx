"use client";

import { useEffect, useMemo, useState } from "react";
import { EFFECT_META, type Effect } from "../lib/textStyle";

const words = (text: string): string[] => {
  try { return Array.from(new Intl.Segmenter(undefined, { granularity: "word" }).segment(text), (s) => s.segment); }
  catch { return text.split(/(\s+)/).filter(Boolean); }
};
const graphemes = (text: string): string[] => {
  try { return Array.from(new Intl.Segmenter(undefined, { granularity: "grapheme" }).segment(text), (s) => s.segment); }
  catch { return Array.from(text); }
};
const reduced = () => typeof window !== "undefined" && window.matchMedia("(prefers-reduced-motion: reduce)").matches;

const GLYPHS = "!<>-_/[]{}=+*^?#";
function Scramble({ text }: { text: string }) {
  const [shown, setShown] = useState(text);
  useEffect(() => {
    if (reduced()) { setShown(text); return; }
    const chars = graphemes(text);
    let tick = 0;
    const id = window.setInterval(() => {
      tick++;
      // Each character settles after a staggered number of ticks; spaces never scramble.
      setShown(chars.map((c, i) => (/\s/.test(c) || tick > 6 + i * 1.5 ? c : GLYPHS[Math.floor(Math.random() * GLYPHS.length)])).join(""));
      if (tick > 6 + chars.length * 1.5) window.clearInterval(id);
    }, 36);
    return () => window.clearInterval(id);
  }, [text]);
  return <><span className="sr-only">{text}</span><span aria-hidden="true">{shown}</span></>;
}

/**
 * Text with a motion effect from Settings > Text style. The animation starts when this mounts, so key it on the text.
 * Each effect splits into words, characters (words stay unbroken so lines still wrap) or runs on the whole text.
 * Looping effects pause while not `playing`; they follow play state, not the audio level (the audio never reaches the page).
 */
export function FxText({ text, effect, playing = true, base = 0 }: { text: string; effect: Effect; playing?: boolean; base?: number }) {
  const meta = EFFECT_META.get(effect);
  const unit = meta?.unit ?? "none";
  const parts = useMemo(() => (unit === "word" || unit === "char" ? words(text) : []), [unit, text]);
  if (unit === "none") return <>{text}</>;
  if (effect === "scramble") return <Scramble text={text} />;
  if (effect === "shiny") return <span className="lx-fx-shiny">{text}</span>;
  if (unit === "whole") return <span className="lx-fx-whole" style={{ animationDelay: `${Math.min(base, 40) * 70}ms` }} data-fx={effect} data-loop={meta?.loop} data-playing={playing}>{text}</span>;
  // `base` offsets the stagger when the text is one word of a longer line.
  let n = base;
  return (
    <>
      <span className="sr-only">{text}</span>
      <span aria-hidden="true" className="lx-fx" data-fx={effect} data-loop={meta?.loop} data-playing={playing}>
        {parts.map((w, wi) => unit === "word"
          ? <span key={wi} className="lx-fx-w" style={{ "--i": Math.min(n++, 40) } as React.CSSProperties}>{w}</span>
          : <span key={wi} className="lx-fx-w">{graphemes(w).map((g, gi) => <span key={gi} className="lx-fx-c" style={{ "--i": Math.min(n++, 90) } as React.CSSProperties}>{g}</span>)}</span>)}
      </span>
    </>
  );
}
