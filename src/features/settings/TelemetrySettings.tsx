"use client";
import { useEffect, useState } from "react";
import { Switch } from "../../components/Lyrics";
import { Button } from "../../components/ui";
import { telemetryDelete, telemetryFlush, telemetryGet, telemetrySet, type TelemetryViewDto } from "../../services/tauri/telemetry";
import { Inset, NoteRow, SettingRow } from "./rows";

/** Settings > General > Usage data. Off by default. Shows exactly what would be sent. */
export function TelemetrySettings() {
  const [view, setView] = useState<TelemetryViewDto | null>(null);
  const [note, setNote] = useState<{ tone: "ok" | "bad"; text: string } | null>(null);
  const [busy, setBusy] = useState(false);
  const read = () => telemetryGet().then(setView, () => setNote({ tone: "bad", text: "Could not read the usage data setting." }));
  useEffect(() => { void read(); }, []);
  async function toggle(on: boolean) {
    setBusy(true);
    try { setView(await telemetrySet(on)); setNote(null); } catch { setNote({ tone: "bad", text: "Could not change the setting." }); }
    finally { setBusy(false); }
  }
  async function sendNow() {
    setBusy(true);
    try { await telemetryFlush(); setNote({ tone: "ok", text: "Sent." }); await read(); }
    catch { setNote({ tone: "bad", text: "Could not reach the server. The counts are kept and sent later." }); }
    finally { setBusy(false); }
  }
  async function remove() {
    setBusy(true);
    try { setView(await telemetryDelete()); setNote({ tone: "ok", text: "Deleted from the server. Usage data is off and this app has a new anonymous ID." }); }
    catch { setNote({ tone: "bad", text: "Could not reach the server, so nothing was deleted. Try again." }); }
    finally { setBusy(false); }
  }
  if (!view) return null;
  const pending = Object.entries(view.pending);
  return (
    <Inset id="settings-usage" title="Usage data">
      <SettingRow label="Share anonymous usage counts" hint="Helps decide what to improve. Sends how often features are used and which error types happen, with the app version and your OS. Never song titles, artists, searches, accounts or sign-ins.">
        <Switch on={view.enabled} onChange={(on) => void toggle(on)} name="Share anonymous usage counts">{view.enabled ? "On" : "Off"}</Switch>
      </SettingRow>
      {view.enabled && (
        <>
          <NoteRow tone="off">
            Next upload: {pending.length === 0 ? "nothing yet" : pending.map(([k, v]) => `${k} ×${v}`).join(", ")}. Anonymous ID: {view.installId}
          </NoteRow>
          <SettingRow label="Upload and delete">
            <Button disabled={busy || pending.length === 0} onClick={() => void sendNow()}>Send now</Button>
            <Button disabled={busy} onClick={() => void remove()}>Delete my usage data</Button>
          </SettingRow>
        </>
      )}
      {note && <NoteRow tone={note.tone} role={note.tone === "bad" ? "alert" : "status"}>{note.text}</NoteRow>}
    </Inset>
  );
}
