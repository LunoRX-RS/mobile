import { invoke } from "@tauri-apps/api/core";

export type DiscordStatus = "off" | "connecting" | "connected" | "unavailable";
export interface DiscordSettings { enabled: boolean; clientId: string; defaultClientId: string; status: DiscordStatus }
export const discordSettingsGet = () => invoke<DiscordSettings>("discord_settings_get");
/** Rejects with "invalid application id" for a malformed Application ID. */
export const discordSettingsSet = (enabled: boolean, clientId: string) => invoke<void>("discord_settings_set", { args: { enabled, clientId } });
