import type { LrcLineDto } from "../services/tauri/lrc";

const STAMP = /\[(\d{1,3}):(\d{2})(?:[.:](\d{1,3}))?\]/g;

/** Reads plain .lrc text: `[mm:ss.xx]line`, several stamps per line allowed. Tags such as [ar:...] and `<mm:ss.xx>` word stamps are ignored. */
export function parseLrc(text: string): LrcLineDto[] {
  const lines: LrcLineDto[] = [];
  for (const raw of text.replace(/^﻿/, "").split(/\r?\n/)) {
    const stamps = [...raw.matchAll(STAMP)];
    if (stamps.length === 0) continue;
    const body = raw.replace(STAMP, "").replace(/<\d{1,3}:\d{2}(?:[.:]\d{1,3})?>/g, "").trim();
    for (const m of stamps) {
      const frac = m[3] ? Number(m[3].padEnd(3, "0").slice(0, 3)) : 0;
      lines.push({ startMs: (Number(m[1]) * 60 + Number(m[2])) * 1000 + frac, text: body });
    }
  }
  return lines.sort((a, b) => a.startMs - b.startMs);
}
