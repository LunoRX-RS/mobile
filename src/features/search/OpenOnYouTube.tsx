"use client";

import { useState } from "react";
import { ExternalLink } from "lucide-react";
import { playerOpenExternal } from "../../services/tauri/player";
import type { SearchHitDto } from "../../services/tauri/search";

const FAILED_TEXT = "Could not open YouTube. Try again.";
const INVALID_TEXT = "This track cannot be opened on YouTube.";

type Status = "idle" | "opening" | "opened" | "failed" | "invalid";

export function OpenOnYouTube({ hit }: { hit: SearchHitDto }) {
  const [status, setStatus] = useState<Status>("idle");

  async function open() {
    setStatus("opening");
    try {
      await playerOpenExternal({
        provider: hit.provider,
        providerTrackId: hit.providerTrackId,
        canonicalUrl: hit.canonicalUrl,
      });
      setStatus("opened");
    } catch (e) {
      const category =
        typeof e === "object" && e !== null && "category" in e ? e.category : undefined;
      setStatus(category === "validation" ? "invalid" : "failed");
    }
  }

  return (
    <span className="relative inline-flex">
      <button
        type="button"
        data-open-youtube
        aria-label={`Open ${hit.title} on YouTube`}
        title="Open on YouTube"
        disabled={status === "opening"}
        onClick={open}
        className="inline-flex size-8 items-center justify-center rounded-[8px] text-secondary-text transition-colors duration-150 hover:bg-frost-300 hover:text-primary-text disabled:opacity-40"
      >
        <ExternalLink size={16} aria-hidden="true" />
      </button>
      {status === "opened" && <span role="status" className="sr-only">Opened YouTube in your browser.</span>}
      {(status === "failed" || status === "invalid") && (
        <span role="alert" className="glass-solid lx-note absolute top-full right-0 z-20 mt-1 w-56 rounded-[10px] px-3 py-2 text-[12px] leading-4 text-critical-coral shadow-[0_16px_48px_rgba(0,0,0,0.5)]">
          {status === "failed" ? FAILED_TEXT : INVALID_TEXT}
        </span>
      )}
    </span>
  );
}
