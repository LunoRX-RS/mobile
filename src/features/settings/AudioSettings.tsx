"use client";
import { useEffect, useState } from "react";
import { Button } from "../../components/ui";
import { playerAudioSettings, playerSetAudioSettings, type AudioSettings, type AudioSettingsDto } from "../../services/tauri/player";
import { setSleepTimer, useSleepTimer } from "../../lib/sleepTimer";
import { FIELD, Inset, NoteRow, SettingRow } from "./rows";

const LOUDNESS_HINT: Record<AudioSettings["loudness"], string> = {
  off: "Plays the track as it comes from YouTube. Quiet tracks stay quiet.",
  balanced: "Evens out quiet and loud parts so tracks sound closer in volume.",
  loud: "Balanced, plus 4 dB more gain with a limiter to avoid clipping. May sound compressed on quiet tracks.",
};

export function AudioSettingsPanel() {
  const [value, setValue] = useState<AudioSettingsDto | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState(false);
  useEffect(() => {
    let active = true;
    playerAudioSettings().then(v => { if (active) setValue(v); }).catch(() => { if (active) setError(true); });
    return () => { active = false; };
  }, []);
  async function update(settings: AudioSettings) {
    setBusy(true); setError(false);
    try { setValue(await playerSetAudioSettings(settings)); } catch { setError(true); }
    finally { setBusy(false); }
  }
  async function refresh() {
    setBusy(true); setError(false);
    try { setValue(await playerAudioSettings()); } catch { setError(true); }
    finally { setBusy(false); }
  }
  const endsAt = useSleepTimer((t) => t.endsAt);
  const disabled = busy || !value?.available;
  const output = value?.output;
  return <Inset id="settings-audio" title="Audio quality and output">
    <SettingRow label="Stream quality" htmlFor="audio-quality" hint="YouTube target bitrate. Uses the lowest available stream if none meets the limit.">
      <select id="audio-quality" className={FIELD} disabled={disabled} value={value?.settings.quality ?? "high"} onChange={e => value && update({ ...value.settings, quality: e.target.value as AudioSettings["quality"] })}>
        <option value="low">Low · up to 64 kbps</option><option value="normal">Normal · up to 128 kbps</option><option value="high">High · best available</option>
      </select>
    </SettingRow>
    <SettingRow label="PCM output" htmlFor="audio-pcm" hint="24-bit sources fit in 32-bit PCM. Output format does not add source detail; the device may negotiate another format.">
      <select id="audio-pcm" className={FIELD} disabled={disabled} value={value?.settings.pcmFormat ?? "float32"} onChange={e => value && update({ ...value.settings, pcmFormat: e.target.value as AudioSettings["pcmFormat"] })}>
        <option value="int16">16-bit integer</option><option value="int32">32-bit integer (includes 24-bit)</option><option value="float32">32-bit float</option>
      </select>
    </SettingRow>
    <SettingRow label="Sample rate" htmlFor="audio-rate" hint="Automatic follows the source where supported. Other rates resample audio.">
      <select id="audio-rate" className={FIELD} disabled={disabled} value={value?.settings.sampleRate ?? "auto"} onChange={e => value && update({ ...value.settings, sampleRate: e.target.value as AudioSettings["sampleRate"] })}>
        <option value="auto">Automatic</option><option value="hz44100">44.1 kHz</option><option value="hz48000">48 kHz</option><option value="hz96000">96 kHz</option><option value="hz192000">192 kHz</option>
      </select>
    </SettingRow>
    <SettingRow label="Loudness" htmlFor="audio-loudness" hint={LOUDNESS_HINT[value?.settings.loudness ?? "balanced"]}>
      <select id="audio-loudness" className={FIELD} disabled={disabled} value={value?.settings.loudness ?? "balanced"} onChange={e => value && update({ ...value.settings, loudness: e.target.value as AudioSettings["loudness"] })}>
        <option value="off">Off</option><option value="balanced">Balanced</option><option value="loud">Loud</option>
      </select>
    </SettingRow>
    <SettingRow label="Sleep timer" htmlFor="sleep-timer" hint={endsAt ? `Pauses at ${new Date(endsAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}. Closing the app cancels it.` : "Pauses playback once after the chosen time."}>
      <select id="sleep-timer" className={FIELD} value="" onChange={e => setSleepTimer(Number(e.target.value) || null)}>
        <option value="">{endsAt ? "Running…" : "Off"}</option>{[15, 30, 45, 60, 90].map(m => <option key={m} value={m}>{m} minutes</option>)}
      </select>
      {endsAt && <Button onClick={() => setSleepTimer(null)}>Cancel</Button>}
    </SettingRow>
    <SettingRow label="Negotiated audio output" hint="macOS uses shared CoreAudio. Device hardware format remains controlled by Audio MIDI Setup.">
      <span>{output?.format ? `${output.driver ?? "Audio"} · ${output.format} · ${output.sampleRate ?? "?"} Hz` : "No active output"}</span>
      <Button disabled={busy} onClick={refresh}>Refresh</Button>
    </SettingRow>
    <NoteRow tone="off">Loudness changes apply right away. Other settings apply when you load the next track. Saved for this app session. Compressed sources remain compressed quality.</NoteRow>
    {error && <NoteRow tone="bad" role="alert">Audio settings unavailable. Try Refresh.</NoteRow>}
  </Inset>;
}
