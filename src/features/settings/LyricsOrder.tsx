"use client";

import { ArrowDown, ArrowUp, RefreshCw } from "lucide-react";
import { useEffect, useState, type KeyboardEvent } from "react";
import { Switch } from "../../components/Lyrics";
import { Button, IconButton } from "../../components/ui";
import { moveId, normalizeOrder } from "../../lib/lyricsSource";
import { loadLyricsSettings, saveLyricsSettings, useLyricsStore } from "../../lib/lyricsStore";
import { Inset, NoteRow, SettingRow, SkeletonRows } from "./rows";

/**
 * Settings > Playback & audio > Lyrics: the Auto order. Each source can be turned off (nothing is sent to it) and moved with the
 * buttons or Alt+Up and Alt+Down. Changes show at once and are saved in core; a failed save goes back and offers Retry.
 */
export function LyricsOrder() {
  const { settings, loadState, saveState, wanted } = useLyricsStore();
  const [said, setSaid] = useState("");
  useEffect(() => { void loadLyricsSettings(); }, []);

  const order = settings ? normalizeOrder(settings.order, settings.sources) : [];
  const labelOf = (id: string) => settings?.sources.find((s) => s.id === id)?.label ?? id;
  const move = (id: string, delta: number) => {
    if (!settings) return;
    const next = moveId(order, id, delta);
    if (next === order) return;
    setSaid(`${labelOf(id)} moved to position ${next.indexOf(id) + 1} of ${next.length}.`);
    void saveLyricsSettings({ order: next.slice(), disabled: settings.disabled });
  };
  const toggle = (id: string, on: boolean) => {
    if (!settings) return;
    const disabled = on ? settings.disabled.filter((d) => d !== id) : [...settings.disabled, id];
    setSaid(`${labelOf(id)} turned ${on ? "on" : "off"}.`);
    void saveLyricsSettings({ order, disabled });
  };
  const onKey = (id: string) => (event: KeyboardEvent) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    move(id, event.key === "ArrowUp" ? -1 : 1);
  };

  return (
    <Inset id="settings-lyrics" title="Lyrics">
      <SettingRow label="Auto order" hint="Automatic prefers word-timed lyrics, then timed, then plain text. Ties go to the source higher in this list. Your own My LRC file always comes first." />
      {loadState === "loading" && !settings && <SkeletonRows rows={4} />}
      {loadState === "error" && !settings && (
        <NoteRow tone="bad" role="alert">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span>Could not read the lyrics sources.</span>
            <Button onClick={() => void loadLyricsSettings(true)}><RefreshCw size={14} strokeWidth={1.75} aria-hidden="true" />Retry</Button>
          </div>
        </NoteRow>
      )}
      {settings && (
        <ul aria-label="Auto order" className="m-0 list-none p-0">
          {order.map((id, i) => {
            const on = !settings.disabled.includes(id);
            return (
              <li key={id} onKeyDown={onKey(id)} className="flex min-h-11 items-center justify-between gap-4 border-t border-hairline px-4 py-1 text-[13px] text-primary-text">
                <span className={on ? "" : "text-secondary-text"}>{labelOf(id)}</span>
                <span className="flex items-center gap-2">
                  <Switch on={on} onChange={(v) => toggle(id, v)} name={`Use this source: ${labelOf(id)}`}>Use this source</Switch>
                  <IconButton label={`Move ${labelOf(id)} up`} aria-disabled={i === 0} onClick={() => move(id, -1)} className={i === 0 ? "opacity-40" : ""}><ArrowUp size={16} aria-hidden="true" /></IconButton>
                  <IconButton label={`Move ${labelOf(id)} down`} aria-disabled={i === order.length - 1} onClick={() => move(id, 1)} className={i === order.length - 1 ? "opacity-40" : ""}><ArrowDown size={16} aria-hidden="true" /></IconButton>
                </span>
              </li>
            );
          })}
        </ul>
      )}
      {saveState === "error" && (
        <NoteRow tone="bad" role="alert">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <span>Could not save the change. The list is back as it was.</span>
            {wanted && <Button onClick={() => void saveLyricsSettings(wanted)}><RefreshCw size={14} strokeWidth={1.75} aria-hidden="true" />Retry</Button>}
          </div>
        </NoteRow>
      )}
      <p role="status" className="sr-only">{said}</p>
      <NoteRow tone="off">Other sources are unofficial services. Turning one off stops sending the song title and artist to it.</NoteRow>
    </Inset>
  );
}
