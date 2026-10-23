"use client";
import { useState, type FormEvent } from "react";
import { Button, Screen } from "../../components/ui";
import { usePlayback, playTrack } from "../../lib/playback";
import { youtubeVideoId } from "../../lib/youtube";
import type { PlaybackStateName } from "../../services/tauri/player";
import { FIELD, Inset, NoteRow, PageRoot, SettingRow, StatusRow, type Tone } from "../settings/rows";

const STATES: Record<PlaybackStateName, { tone: Tone; text: string }> = {
  idle: { tone: "off", text: "Idle" },
  resolving: { tone: "off", text: "Finding audio" },
  preparing: { tone: "off", text: "Preparing" },
  ready: { tone: "off", text: "Ready" },
  playing: { tone: "ok", text: "Playing" },
  paused: { tone: "off", text: "Paused" },
  buffering: { tone: "off", text: "Buffering" },
  blocked: { tone: "warn", text: "Blocked" },
  ended: { tone: "off", text: "Ended" },
  error: { tone: "bad", text: "Error" },
};

export default function PlayerScreen() {
  const playback = usePlayback();
  const [input, setInput] = useState("");
  const [invalid, setInvalid] = useState(false);
  function submit(event: FormEvent) {
    event.preventDefault();
    const id = youtubeVideoId(input);
    setInvalid(!id);
    if (!id) return;
    void playTrack({ provider: "YouTube", providerTrackId: id,
      canonicalUrl: `https://www.youtube.com/watch?v=${id}`, title: `YouTube · ${id}` });
  }
  const track = playback.requested ?? playback.track;
  const state = STATES[playback.player.state] ?? { tone: "off" as Tone, text: playback.player.state };

  return (
    <PageRoot>
      <Screen
        id="player-title"
        title="Player"
        caption="Status of audio playback in the app. Pause, seek, volume and stop are in the player bar."
      >
        <div className="flex flex-col gap-8">
          <form onSubmit={submit} noValidate>
            <Inset id="youtube-playback" title="Play from YouTube">
              <SettingRow label="YouTube link or video ID" htmlFor="youtube-url" hint="Public YouTube audio plays in the desktop app. Pasted links need no API key." hintId="youtube-help">
                <input
                  id="youtube-url"
                  type="text"
                  value={input}
                  onChange={(event) => setInput(event.target.value)}
                  aria-invalid={invalid ? true : undefined}
                  aria-describedby={invalid ? "youtube-help youtube-invalid" : "youtube-help"}
                  placeholder="https://www.youtube.com/watch?v=…"
                  className={`${FIELD} w-[280px]`}
                />
                <Button type="submit" variant="primary" disabled={playback.busy || !input.trim()}>
                  {playback.busy ? "Preparing…" : "Play"}
                </Button>
              </SettingRow>
              {invalid && (
                <NoteRow tone="bad" id="youtube-invalid" role="alert">
                  Enter a valid YouTube video URL or 11-character video ID.
                </NoteRow>
              )}
            </Inset>
          </form>

          <Inset id="native-now-playing" title="Now playing">
            <SettingRow label="Track">{track?.title ?? "Nothing loaded"}</SettingRow>
            <StatusRow
              label="State"
              tone={state.tone}
              status={playback.busy ? "Preparing audio…" : state.text}
              hint={`Revision ${playback.player.revision}`}
            />
            {playback.error && (
              <NoteRow tone="bad" role="alert">
                {playback.error}
              </NoteRow>
            )}
            <SettingRow label="Controls" hint={track ? "Use the player bar to pause, seek, change volume or stop." : "The player bar controls stay disabled until a track is loaded."} />
          </Inset>

          <Inset id="player-capabilities" title="Playback capabilities">
            <StatusRow
              label="YouTube audio in the app"
              tone={playback.available ? "ok" : "off"}
              status={playback.available ? "Available" : "Not confirmed"}
              hint="Plays through the native player (libmpv). Confirmed once a track has loaded."
            />
            <StatusRow label="YouTube, open externally" tone="ok" status="Available" hint="Opens the video on YouTube in your browser." />
            <StatusRow label="Spotify audio" tone="off" status="Unsupported" hint="No authorized Spotify playback yet." />
            <StatusRow label="Sample tracks and queue" tone="off" status="Demo only" hint="Sample library tracks and the sample queue have no audio." />
          </Inset>
        </div>
      </Screen>
    </PageRoot>
  );
}
