"use client";
import { Infinity as InfinityIcon, Repeat, Repeat1 } from "lucide-react";
import { IconButton } from "./ui";
import { usePlayback } from "../lib/playback";
import { nextRepeat, setPlayMode, type Repeat as RepeatMode } from "../lib/playMode";

/** Repeat: off, the whole list, or this track. */
export function RepeatButton({ className }: { className?: string }) {
  const repeat = usePlayback((s) => (s.autoplay.repeat ?? "off") as RepeatMode);
  const label = repeat === "off" ? "Repeat off" : repeat === "all" ? "Repeat all" : "Repeat this track";
  return (
    <IconButton label={label} active={repeat !== "off"} aria-pressed={repeat !== "off"} title={`${label}. Click to change.`} className={className} onClick={() => setPlayMode({ repeat: nextRepeat(repeat) })}>
      {repeat === "one" ? <Repeat1 size={18} /> : <Repeat size={18} />}
    </IconButton>
  );
}

/** Autoplay: keep playing related tracks once the list is used up. */
export function AutoplayButton({ className }: { className?: string }) {
  const on = usePlayback((s) => s.autoplay.radio ?? true);
  const label = on ? "Autoplay on" : "Autoplay off";
  return (
    <IconButton label={label} active={on} aria-pressed={on} title={`${label}. Plays related music when your list ends.`} className={className} onClick={() => setPlayMode({ autoplay: !on })}>
      <InfinityIcon size={18} />
    </IconButton>
  );
}
