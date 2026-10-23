"use client";

import { Button, Screen } from "../../components/ui";
import { usePlayback } from "../../lib/playback";
import { Inset, PageRoot, SettingRow, SkeletonRows, StatusRow } from "../settings/rows";
import { searchLabelFor, searchTone, useProviderStatuses } from "./providerStatus";
import { YouTubeConnection } from "./YouTubeConnection";

// No Spotify connect command is registered. sync_sign_in and sync_sign_out are LUNOR-X backend sign-in, not provider connect.
const SPOTIFY_REASON = "Not available: no Spotify sign-in command is registered yet.";

export default function ConnectionsScreen() {
  const providers = useProviderStatuses();
  const nativeAvailable = usePlayback((s) => s.available);
  const loading = providers.state === "loading";

  return (
    <PageRoot>
      <Screen
        id="connections-title"
        title="Connections"
        caption="Music provider accounts and what each one can do. Sign-in to LUNOR-X sync is on Accounts."
      >
        <div className="flex flex-col gap-8">
          <Inset id="connections-youtube-title" title="YouTube Music">
            <SettingRow label="Google sign-in" stack>
              <YouTubeConnection />
            </SettingRow>
            {loading ? (
              <SkeletonRows rows={1} />
            ) : (
              <StatusRow
                label="Search"
                tone={searchTone(providers, "YouTube")}
                status={searchLabelFor(providers, "YouTube")}
                hint="Direct YouTube Music search. No API key is required."
              />
            )}
            <StatusRow
              label="Playback in app"
              tone={nativeAvailable ? "ok" : "off"}
              status={nativeAvailable ? "Available" : "Not confirmed"}
              hint={
                nativeAvailable
                  ? "Audio plays through the native player."
                  : "Audio needs libmpv on this device. It is confirmed once a track has loaded in the Player."
              }
            />
            <StatusRow label="Open externally" tone="ok" status="Available" hint="Opens the video on YouTube in your browser." />
          </Inset>

          <Inset id="connections-spotify-title" title="Spotify">
            <StatusRow label="Connection" tone="off" status="Not connected" hint={SPOTIFY_REASON} hintId="spotify-connect-reason">
              <Button disabled aria-describedby="spotify-connect-reason">
                Connect
              </Button>
              <Button disabled aria-describedby="spotify-connect-reason">
                Disconnect
              </Button>
            </StatusRow>
            {loading ? (
              <SkeletonRows rows={1} />
            ) : (
              <StatusRow
                label="Search"
                tone={searchTone(providers, "Spotify")}
                status={searchLabelFor(providers, "Spotify")}
                hint={
                  providers.state === "ready" && providers.searchByProvider.Spotify === "Unsupported"
                    ? "The core does not report a reason."
                    : undefined
                }
              />
            )}
            <StatusRow
              label="Playback in app"
              tone="off"
              status="Unsupported"
              hint="Spotify audio must come from an authorized Spotify player. None is built in yet."
            />
            <StatusRow label="Open externally" tone="off" status="Not available" hint="No command opens Spotify items yet." />
          </Inset>
        </div>
      </Screen>
    </PageRoot>
  );
}
