"use client";
import { useState } from "react";
import { Music2 } from "lucide-react";
import { cx } from "./ui";

export function TrackCover({ url, videoId, title = "", className }: {
  url?: string | null; videoId: string; title?: string; className?: string;
}) {
  const [failed, setFailed] = useState<string[]>([]);
  const fallback = /^[A-Za-z0-9_-]{11}$/.test(videoId) ? `https://i.ytimg.com/vi/${videoId}/hqdefault.jpg` : null;
  const source = url && !failed.includes(url) ? url : fallback && !failed.includes(fallback) ? fallback : null;
  return (
    <div className={cx("relative flex shrink-0 items-center justify-center overflow-hidden rounded-[6px] bg-frost-200 text-secondary-text", className)}>
      <Music2 className="size-1/3" aria-hidden="true" />
      {source && <img src={source} alt={title ? `${title} artwork` : ""} loading="lazy" referrerPolicy="no-referrer"
        onError={() => setFailed((previous) => [...previous, source].slice(-2))} className="absolute inset-0 size-full object-cover" />}
    </div>
  );
}
