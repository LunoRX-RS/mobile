"use client";
import { useEffect, useState } from "react";
import { Switch } from "../../components/Lyrics";
import { discordSettingsGet, discordSettingsSet, type DiscordSettings as Dto, type DiscordStatus } from "../../services/tauri/discord";
import { FIELD, Inset, NoteRow, SettingRow } from "./rows";

const STATUS: Record<DiscordStatus, string> = {
  off: "Off",
  connecting: "Connecting to Discord…",
  connected: "Connected. Your track shows on your Discord profile.",
  unavailable: "Discord is not reachable. Open the Discord app (not the website) and check the Application ID.",
};

/** Settings > General > Discord. Off by default: turning it on shares what you play with your Discord profile. */
export function DiscordSettings() {
  const [settings, setSettings] = useState<Dto | null>(null);
  const [id, setId] = useState("");
  const [error, setError] = useState("");
  const read = () => discordSettingsGet().then((s) => { setSettings(s); return s; }, () => null);
  useEffect(() => {
    let live = true;
    void read().then((s) => { if (live && s) setId(s.clientId); });
    // The connection comes up a moment after the switch, and Discord can start later; read the status again.
    const t = window.setInterval(() => void read(), 4000);
    return () => { live = false; window.clearInterval(t); };
  }, []);
  async function save(enabled: boolean, clientId: string) {
    setError("");
    try { await discordSettingsSet(enabled, clientId); } catch { setError("That is not a valid Application ID. It is 17 to 20 digits."); }
    await read();
  }
  if (!settings) return null;
  const custom = id.trim() !== "";
  const valid = !custom || /^\d{17,20}$/.test(id.trim());
  return (
    <Inset id="settings-discord" title="Discord">
      <SettingRow label="Show what I'm listening to" hint="Sends the track title, artists, cover and a YouTube link to your Discord app, which shows them to people who can see your profile.">
        <Switch on={settings.enabled} onChange={(on) => void save(on, id)} name="Show what I'm listening to on Discord">{settings.enabled ? "On" : "Off"}</Switch>
      </SettingRow>
      <SettingRow label="Own Application ID (optional)" htmlFor="discord-app-id" hint="Leave empty to use LunoRX's. To show your own app name and artwork, create an application at discord.com/developers and paste its Application ID.">
        <input id="discord-app-id" inputMode="numeric" autoComplete="off" spellCheck={false} placeholder={settings.defaultClientId} value={id} aria-invalid={!valid}
          onChange={(e) => setId(e.target.value)} onBlur={() => { if (id.trim() !== settings.clientId) void save(settings.enabled, id.trim()); }} className={`${FIELD} w-[220px] tabular-nums`} />
      </SettingRow>
      <NoteRow tone={settings.status === "connected" ? "ok" : settings.status === "unavailable" ? "warn" : "off"} role="status">
        {STATUS[settings.status]}
      </NoteRow>
      {error && <NoteRow tone="bad" role="alert">{error}</NoteRow>}
    </Inset>
  );
}
