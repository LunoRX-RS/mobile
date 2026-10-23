"use client";
import { YouTubeConnection } from "../connections/YouTubeConnection";
import { Inset, SettingRow, StatusRow } from "./rows";

// Provider sign-in rows for Settings. YouTube search needs no API key; the only credential is the Google session,
// which stays in the OS key store. Spotify has no sign-in command yet.
export function ProviderKeys() {
  return (
    <Inset id="settings-providers" title="Providers">
      <SettingRow label="YouTube Music" stack>
        <YouTubeConnection />
      </SettingRow>
      <StatusRow
        label="Spotify"
        tone="off"
        status="Not available"
        hint="No Spotify sign-in command is registered yet."
      />
    </Inset>
  );
}
