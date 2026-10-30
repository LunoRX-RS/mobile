"use client";

import { useNotice } from "../../lib/notice";

export function NoticeBar() {
  const text = useNotice((s) => s.text);
  if (!text) return null;
  return (
    <div role="status" aria-live="polite" className="glass-solid lx-rise fixed bottom-[104px] left-4 z-[70] max-w-[min(380px,calc(100vw-32px))] rounded-[12px] px-4 py-3 text-[13px] leading-[18px] text-primary-text shadow-[0_18px_48px_rgba(0,0,0,0.5)]">
      {text}
    </div>
  );
}
