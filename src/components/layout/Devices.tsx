"use client";

import { useEffect, useRef, useState } from "react";
import { Laptop, MonitorSmartphone, Pause, Play, Smartphone, SkipBack, SkipForward, Volume2 } from "lucide-react";
import { formatDuration } from "../../lib/media";
import { remotePosition, sendRemoteCommand, useRemote, type RemoteDevice } from "../../lib/remote";
import { IconButton, Scrubber, cx } from "../ui";
import { TrackCover } from "../TrackCover";

const LINK_TEXT = {
  off: "Cross-device control is off.",
  connecting: "Connecting…",
  offline: "Offline. Trying again…",
  signedOut: "Sign in on the Accounts page to see your other devices.",
  online: "",
} as const;

function DeviceIcon({ kind }: { kind: RemoteDevice["kind"] }) {
  return kind === "phone" ? <Smartphone size={16} aria-hidden="true" /> : <Laptop size={16} aria-hidden="true" />;
}

/** Opens the list of the account's devices. Picking one makes this app its remote control. */
export function DevicesButton() {
  const [open, setOpen] = useState(false);
  const box = useRef<HTMLDivElement>(null);
  const { enabled, link, devices, target, notice, setEnabled, setTarget } = useRemote();
  useEffect(() => {
    if (!open) return;
    const away = (e: PointerEvent) => { if (!box.current?.contains(e.target as Node)) setOpen(false); };
    const esc = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("pointerdown", away);
    document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("pointerdown", away); document.removeEventListener("keydown", esc); };
  }, [open]);
  const list = Object.values(devices).sort((a, b) => Number(b.online) - Number(a.online) || a.name.localeCompare(b.name));
  return (
    <div ref={box} className="relative">
      <IconButton label="Devices" active={target !== null} aria-expanded={open} onClick={() => setOpen((o) => !o)}>
        <MonitorSmartphone size={17} />
      </IconButton>
      {open && (
        <div role="dialog" aria-label="Devices" className="glass-solid lx-pop-in absolute right-0 bottom-full z-[80] mb-2 flex w-[300px] flex-col gap-0.5 rounded-[10px] p-1.5 text-[13px] shadow-[0_24px_64px_rgba(0,0,0,0.55)]">
          {!enabled ? (
            <div className="flex flex-col gap-2 p-2">
              <p className="text-secondary-text">Control playback on your other devices, or let them control this one. Your devices talk through your LUNOR-X account; no audio leaves a device.</p>
              <button type="button" onClick={() => setEnabled(true)} className="h-8 rounded-[8px] bg-primary-text text-on-ink font-semibold lx-press">Turn on</button>
            </div>
          ) : (
            <>
              <button type="button" onClick={() => setTarget(null)} aria-pressed={target === null}
                className={cx("flex h-9 items-center gap-2.5 rounded-[6px] px-2.5 text-left hover:bg-white/10", target === null && "text-lumen-lavender")}>
                <Laptop size={16} aria-hidden="true" /><span className="min-w-0 flex-1 truncate">This device</span>
              </button>
              {list.map((d) => (
                <button key={d.deviceId} type="button" disabled={!d.online} aria-pressed={target === d.deviceId} onClick={() => { setTarget(d.deviceId); setOpen(false); }}
                  className={cx("flex min-h-9 items-center gap-2.5 rounded-[6px] px-2.5 py-1 text-left hover:bg-white/10 disabled:opacity-40", target === d.deviceId && "text-lumen-lavender")}>
                  <DeviceIcon kind={d.kind} />
                  <span className="min-w-0 flex-1">
                    <span className="block truncate">{d.name}</span>
                    <span className="block truncate text-[11px] text-secondary-text">{!d.online ? "Offline" : d.state?.track ? `${d.state.status === "playing" ? "Playing" : "Paused"} · ${d.state.track.title}` : "Idle"}</span>
                  </span>
                </button>
              ))}
              {link === "online" && list.length === 0 && <p className="p-2 text-secondary-text">No other devices yet. Open LUNOR-X on another device signed in to this account and turn this on there.</p>}
              {link !== "online" && <p role="status" className="p-2 text-secondary-text">{LINK_TEXT[link]}</p>}
              {notice && <p role="alert" className="px-2 text-[11px] text-critical-coral">{notice}</p>}
              <button type="button" onClick={() => setEnabled(false)} className="mt-1 h-8 rounded-[6px] px-2.5 text-left text-secondary-text hover:bg-white/10">Turn off</button>
            </>
          )}
        </div>
      )}
    </div>
  );
}

/** The player bar while this app drives another device. Local playback is not touched. */
export function RemotePlayerBar() {
  const { target, devices, setTarget } = useRemote();
  const device = target ? devices[target] : undefined;
  const state = device?.state ?? null;
  const [, tick] = useState(0);
  useEffect(() => {
    if (state?.status !== "playing") return;
    const id = window.setInterval(() => tick((n) => n + 1), 500);
    return () => window.clearInterval(id);
  }, [state?.status]);
  const track = state?.track ?? null;
  const online = device?.online === true;
  const playing = state?.status === "playing" || state?.status === "buffering";
  const duration = track?.durationMs ?? 0;
  const position = state ? remotePosition(state) : 0;
  return (
    <footer aria-label={`Controlling ${device?.name ?? "another device"}`} className="glass-bar flex min-h-[var(--height-player)] shrink-0 items-center gap-4 border-t border-hairline px-4">
      <div className="flex w-[30%] min-w-0 items-center gap-3">
        {track && <TrackCover key={track.providerTrackId} url={null} videoId={track.providerTrackId} className="lx-fade-in size-12" />}
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13px] font-semibold">{track?.title ?? "Nothing playing"}</p>
          <p className="truncate text-[11px] text-secondary-text">{track?.artists.join(", ")}</p>
          <p role="status" className="truncate text-[11px] text-lumen-lavender">{online ? `Controlling ${device?.name}` : `${device?.name ?? "Device"} is offline`}</p>
        </div>
      </div>
      <div className="flex min-w-0 flex-1 flex-col items-center gap-1">
        <div className="flex items-center gap-2">
          <IconButton label="Previous track" disabled={!online} onClick={() => sendRemoteCommand({ action: "previous" })}><SkipBack size={20} fill="currentColor" /></IconButton>
          <IconButton label={playing ? "Pause" : "Play"} disabled={!online} onClick={() => sendRemoteCommand({ action: playing ? "pause" : "play" })}>
            {playing ? <Pause size={24} /> : <Play size={24} />}
          </IconButton>
          <IconButton label="Next track" disabled={!online} onClick={() => sendRemoteCommand({ action: "next" })}><SkipForward size={20} fill="currentColor" /></IconButton>
        </div>
        <div className="flex w-full max-w-[560px] items-center gap-2 text-[11px] tabular-nums text-secondary-text">
          <span>{formatDuration(position / 1000)}</span>
          <Scrubber label="Seek on the other device" max={Math.max(1, duration / 1000)} value={position / 1000} disabled={!online || !duration}
            onCommit={(value) => sendRemoteCommand({ action: "seek", positionMs: Math.round(value * 1000) })} className="flex-1" />
          <span>{formatDuration(duration / 1000)}</span>
        </div>
      </div>
      <div className="flex w-[20%] items-center justify-end gap-2 text-secondary-text">
        <button type="button" onClick={() => setTarget(null)} className="h-8 rounded-[8px] px-3 text-[13px] font-medium hover:bg-frost-100 hover:text-primary-text lx-press">Play here</button>
        <DevicesButton />
        <Volume2 size={16} aria-hidden="true" />
        <Scrubber label="Volume on the other device" max={100} value={state?.volume ?? 100} disabled={!online}
          onCommit={(value) => sendRemoteCommand({ action: "volume", percent: Math.round(value) })} className="w-24" />
      </div>
    </footer>
  );
}
