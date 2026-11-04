import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

// Mirrors commands/player.rs. Tauri maps the Rust `args` struct fields to these camelCase keys.

export interface PlayerOpenExternalArgs {
  provider: string; // only "YouTube" is accepted
  providerTrackId: string; // exactly 11 chars, [A-Za-z0-9_-]
  canonicalUrl: string; // "" or the exact watch URL; the core builds the URL itself
}

// Thrown (rejected) by invoke. Only the category is used; never show it raw.
export type OpenExternalError =
  | { category: "validation" }
  | { category: "unsupportedProvider" }
  | { category: "openFailed" };

export function playerOpenExternal(args: PlayerOpenExternalArgs): Promise<void> {
  return invoke<void>("player_open_external", { args });
}

// Mirrors PlaybackStateDto (camelCase unit variants) and PlayerStateDto in commands/player.rs.
export type PlaybackStateName =
  | "idle"
  | "resolving"
  | "preparing"
  | "ready"
  | "playing"
  | "paused"
  | "buffering"
  | "blocked"
  | "ended"
  | "error";

export interface PlayerStateDto {
  version: number;
  state: PlaybackStateName;
  revision: number;
}

export const PLAYER_STATE_EVENT = "core://player/state";

// Rejected by invoke with this shape. Only the category is used; the message is never shown.
export interface PlayerError {
  category: string;
  message: string;
}

export function playerState(): Promise<PlayerStateDto> {
  return invoke<PlayerStateDto>("player_state");
}

export interface LoadedTrack {
  source?: "YouTube" | "YouTube Music";
  provider: string;
  providerTrackId: string;
  canonicalUrl: string;
  title: string;
  artists?: string[];
  artistRefs?: import("./search").ArtistRef[];
  artworkUrl?: string | null;
}

/** One word with real timing from a source that delivers it. */
export interface LyricWordDto {
  startMs: number;
  endMs: number;
  text: string;
}

/** A line with a real start time. `words` is non-null only when the source delivers real per-word timing; null or absent means whole-line timing. */
export interface LyricLineDto {
  startMs: number;
  text: string;
  words?: LyricWordDto[] | null;
}

export type LyricsSourceId = "youtubeMusic" | "lrclib" | "lyricsplus" | "boidu" | "simpmusic" | "netease" | "qq" | "kugou" | "mine";
export type LyricsSourceStatus = "available" | "noMatch" | "timingMismatch" | "failed" | "unreachable" | "disabled" | "instrumental" | "unavailable";

/**
 * One lyrics candidate. Core lists the sources it knows, in its own order; "mine" is added by the app from the user's own file.
 * Ids can be missing or unknown (a newer core): render what arrives. Lines are real timestamps or null.
 */
export interface LyricsSourceDto {
  id: LyricsSourceId | (string & {});
  label: string;
  available: boolean;
  text: string | null;
  lines: LyricLineDto[] | null;
  synced: boolean;
  /** True when the lines carry real word timings. */
  words?: boolean;
  attribution: string | null;
  /** noMatch: searched, nothing matched. timingMismatch: matched text, but the length differs too much for timestamps (plain only). failed: the source answered with an error. unreachable: no answer (network, timeout). disabled: turned off in Settings. */
  status: LyricsSourceStatus;
  /** The user picked this match by hand (Search by title). */
  bound?: boolean;
}

export interface TrackInfo {
  providerTrackId: string;
  title: string;
  artists: string[];
  artistRefs?: import("./search").ArtistRef[];
  artworkUrl: string | null;
  lyrics: string | null;
  lyricsSource: string | null;
  lyricsProvider: "YouTube Music" | "LRCLIB" | null;
  lyricsStatus: "available" | "unavailable" | "failed" | "instrumental";
  /** Timed lines, or null. Older payloads may omit it; treat absence as null. */
  lyricsLines: LyricLineDto[] | null;
  /** True only when lyricsLines carries real timestamps. */
  lyricsSynced: boolean;
  /** Every source core knows. Older payloads may omit it; see sourcesOf in lib/lyricsSource. */
  lyricsSources?: LyricsSourceDto[];
  /** The album the song is on; `id` is absent when there is no album page. */
  album?: { id?: string | null; name: string };
}

export function playerTrackInfo(providerTrackId: string): Promise<TrackInfo> {
  return invoke("player_track_info", { providerTrackId });
}

export interface PlayerProgressDto {
  autoplay: { state: string; nextTitle: string | null; queue: import("./offline").OfflineTrack[]; repeat?: "off" | "all" | "one"; radio?: boolean };
  player: PlayerStateDto;
  track: LoadedTrack | null;
  positionMs: number;
  durationMs: number;
  available: boolean;
  /** Last volume core accepted (0-100); follows macOS menu changes. */
  volume: number;
}

/** `following` are the tracks to play after this one, in order (an album or playlist). */
export function playerLoad(track: LoadedTrack, queueSelection = false, following: LoadedTrack[] = []): Promise<PlayerStateDto> {
  const next = following.map((t) => ({
    provider: t.provider,
    providerTrackId: t.providerTrackId, title: t.title, artists: t.artists ?? [], artworkUrl: t.artworkUrl ?? null,
    durationMs: (t as { durationMs?: number | null }).durationMs ?? null,
  }));
  return invoke("player_load", { args: { ...track, queueSelection, following: next } });
}

export function playerCommand(command: "play" | "pause" | "resume" | "stop"): Promise<PlayerStateDto> {
  return invoke(`player_${command}`);
}

export function playerSkip(forward: boolean): Promise<void> {
  return invoke("player_skip", { forward });
}

/** Repeat ("off" | "all" | "one") and autoplay (related tracks after the list). Core keeps them until changed. */
export function playerSetMode(repeat: "off" | "all" | "one", autoplay: boolean): Promise<void> {
  return invoke("player_set_mode", { args: { repeat, autoplay } });
}

export function playerSeek(positionMs: number): Promise<PlayerStateDto> {
  return invoke("player_seek", { positionMs: Math.round(positionMs) });
}

export function playerSetVolume(volume: number): Promise<PlayerStateDto> {
  return invoke("player_set_volume", { volume: Math.round(volume) });
}

export function playerProgress(): Promise<PlayerProgressDto> {
  return invoke("player_progress");
}

export function subscribePlayerProgress(onProgress: (dto: PlayerProgressDto) => void): Promise<UnlistenFn> {
  return listen<PlayerProgressDto>("core://player/progress", (event) => onProgress(event.payload));
}

// Resolves to the unlisten function. The caller must call it on teardown, even if teardown happens
// before this promise resolves.
export function subscribePlayerState(
  onState: (dto: PlayerStateDto) => void,
): Promise<UnlistenFn> {
  return listen<PlayerStateDto>(PLAYER_STATE_EVENT, (event) => onState(event.payload));
}

export interface AudioSettings {
  quality: "low" | "normal" | "high";
  pcmFormat: "int16" | "int32" | "float32";
  sampleRate: "auto" | "hz44100" | "hz48000" | "hz96000" | "hz192000";
  loudness: "off" | "balanced" | "loud";
  effects: AudioEffects;
}
/** Sound shaping done by mpv's filters. Gains are whole dB, -12 to 12. */
export interface AudioEffects {
  enabled: boolean;
  preamp: number;
  /** 31, 62, 125, 250, 500 Hz, then 1, 2, 4, 8, 16 kHz. */
  bands: number[];
  bass: number;
  treble: number;
  /** 0 mono, 100 unchanged, 200 doubled. */
  stereoWidth: number;
  crossfeed: number;
  reverb: "off" | "room" | "hall";
  compressor: "off" | "light" | "medium" | "heavy";
  /** Percent, 50 to 200. */
  speed: number;
  /** True keeps the pitch when the speed changes. False lets pitch follow speed (nightcore: faster and higher). */
  keepPitch: boolean;
  /** Moving-sound effects for headphones: off, "8d" (slow circle) or "4d" (faster, wider). */
  spatial: "off" | "8d" | "4d";
}
export const FLAT_EFFECTS: AudioEffects = { enabled: true, preamp: 0, bands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0], bass: 0, treble: 0, stereoWidth: 100, crossfeed: 0, reverb: "off", compressor: "off", speed: 100, keepPitch: true, spatial: "off" };
export interface AudioSettingsDto {
  settings: AudioSettings;
  available: boolean;
  output: { driver: string | null; format: string | null; sampleRate: number | null; sourceFormat: string | null; sourceSampleRate: number | null };
}
/** A sound preset: a full set of effects. Choosing one replaces what was set before. */
export interface EffectPreset { id: string; label: string; group: string; effects: AudioEffects }
export function playerEffectPresets(): Promise<EffectPreset[]> {
  return invoke("player_effect_presets");
}
/** Fired when the sound settings change outside the settings panel (the macOS Sound menu). */
export function subscribeAudioSettings(on: (dto: AudioSettingsDto) => void): Promise<UnlistenFn> {
  return listen<AudioSettingsDto>("core://audio/settings", (event) => on(event.payload));
}
export function playerAudioSettings(): Promise<AudioSettingsDto> {
  return invoke("player_audio_settings");
}
export function playerSetAudioSettings(settings: AudioSettings): Promise<AudioSettingsDto> {
  return invoke("player_set_audio_settings", { settings });
}

/** Edits the up-next list: play a track after the current one, drop a queued track, or move one. */
export type QueueEditArgs =
  | { op: "next"; track: { provider: "YouTube"; id: string; title: string; artists: string[]; artworkUrl: string | null; durationMs: number | null } }
  | { op: "remove"; id: string }
  | { op: "move"; from: number; to: number }
  | { op: "shuffle" };

export function playerQueueEdit(args: QueueEditArgs): Promise<void> {
  return invoke<void>("player_queue_edit", { args });
}

/** Appends more related songs to the end of the up-next list; resolves with how many were added (0 at the end). */
export function playerQueueMore(): Promise<number> {
  return invoke<number>("player_queue_more");
}
