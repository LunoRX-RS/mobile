//! Tauri command surface for the playback engine. Commands are thin: validate typed arguments, run one
//! engine call under the engine lock, and return a typed state DTO or a fixed error category.
//! The lock is released before any event is emitted, so a slow webview never blocks the engine.

use std::str::FromStr;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::Mutex;

use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
use crate::core::domain::{ProviderId, TrackReference};
use crate::core::errors::{CoreError, HostRejectKind, PlaybackErrorKind};
use crate::core::playback::command::{PlaybackCommand, Volume};
use crate::core::playback::engine::PlaybackEngine;
use crate::core::playback::host::PlayerEvent;
#[cfg(mobile)]
use crate::core::playback::mobile::NativeHost;
#[cfg(desktop)]
use crate::core::playback::native::NativeHost;
use crate::core::playback::state::PlaybackState;
use crate::core::registry::ProviderRegistry;
use std::{collections::HashMap, time::Duration};
use tauri::Manager;

/// Event name for every state change, payload `PlayerStateDto`.
pub const STATE_EVENT: &str = "core://player/state";
const PAYLOAD_VERSION: u8 = 1;
const MAX_ID_BYTES: usize = 256;
const MAX_URL_BYTES: usize = 2048;
const MAX_TITLE_BYTES: usize = 512;
// 24 h: longer than any video this app can play.
const MAX_SEEK_MS: i64 = 86_400_000;
// Matches the libmpv initial volume in native.rs.
const DEFAULT_VOLUME: u8 = 100;

#[tauri::command]
pub async fn player_track_info(
    app: AppHandle,
    youtube: State<'_, crate::commands::youtube::YouTubeState>,
    offline: State<'_, crate::commands::offline::OfflineState>,
    provider_track_id: String,
) -> Result<crate::core::playback::track_info::TrackInfo, PlayerError> {
    use crate::core::playback::{extract, track_info};
    if !extract::valid_id(&provider_track_id) {
        return Err(invalid());
    }
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| PlayerError::new(PlayerErrorCategory::HostRejected))?;
    let yt = youtube
        .yt
        .as_ref()
        .ok_or_else(|| PlayerError::new(PlayerErrorCategory::UnsupportedCapability))?;
    let lyric_path = app
        .path()
        .app_cache_dir()
        .ok()
        .map(|root| track_info::cache_file(&root, &provider_track_id));
    let lyrics_ctx = crate::commands::lyric_sources::context(&app, &provider_track_id).await;
    let result = tokio::time::timeout(
        Duration::from_secs(25),
        track_info::fetch_with(yt, &provider_track_id, lyric_path.as_deref(), &lyrics_ctx),
    )
    .await
    .map_err(|_| PlayerError::new(PlayerErrorCategory::Timeout))
    .and_then(|result| result.map_err(|error| from_core(&CoreError::Playback(error))));
    let result = result.or_else(|error| {
        lyric_path
            .as_deref()
            .and_then(|path| track_info::cached_lrclib(&provider_track_id, path))
            .ok_or(error)
    });
    let cached = offline
        .manager
        .as_ref()
        .ok()
        .and_then(|m| m.cache.metadata(&provider_track_id));
    match (result, cached) {
        (Ok(mut info), Some(cached)) => {
            info.artwork_url = cached.artwork_url.or(info.artwork_url);
            Ok(info)
        }
        (Err(_), Some(cached)) => Ok(track_info::TrackInfo {
            provider_track_id: cached.id,
            title: cached.title,
            artists: cached.artists,
            artist_refs: Vec::new(),
            album: None,
            artwork_url: cached.artwork_url,
            lyrics: None,
            lyrics_source: None,
            lyrics_provider: None,
            lyrics_status: "unavailable",
            lyrics_lines: None,
            lyrics_synced: false,
            lyrics_sources: track_info::no_sources(),
        }),
        (result, None) => result,
    }
}

/// The one playback session. The engine is behind a tokio Mutex because `dispatch` awaits the host.
pub struct PlayerState {
    engine: std::sync::Arc<Mutex<PlaybackEngine>>,
    autoplay: Mutex<Option<tauri::async_runtime::JoinHandle<()>>>,
    reports: tokio::sync::watch::Sender<u64>,
    autoplay_status: crate::core::playback::autoplay::Status,
    progress: Mutex<(u64, u64)>,
    /// Last volume core accepted (0-100). The webview and the macOS menu both read it from the progress DTO.
    volume: std::sync::atomic::AtomicU8,
    history_pending: std::sync::atomic::AtomicBool,
    pub available: bool,
    audio: crate::core::playback::audio::AudioControl,
}

impl PlayerState {
    pub(crate) fn download_quality(&self) -> crate::core::playback::audio::AudioQuality {
        self.audio.settings().quality
    }
    pub fn start(app: &AppHandle) {
        let (reports, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let native = app
            .state::<crate::commands::youtube::YouTubeState>()
            .yt
            .clone()
            .ok_or(PlaybackErrorKind::UnsupportedCapability)
            .and_then(|yt| {
                let cache = app
                    .state::<crate::commands::offline::OfflineState>()
                    .manager
                    .as_ref()
                    .map_err(|_| PlaybackErrorKind::UnsupportedCapability)?
                    .cache
                    .clone();
                #[cfg(desktop)]
                let host = NativeHost::with_yt(yt, reports);
                #[cfg(mobile)]
                let host = NativeHost::with_yt(app.clone(), yt, reports);
                host.map(|host| host.with_cache(cache))
            });
        // On a phone the page player takes a load the direct stream refuses (see `core::playback::routing`).
        #[cfg(mobile)]
        let native = native.map(|host| {
            let page = host.page_host();
            let audio = host.audio.clone();
            (
                crate::core::playback::routing::RoutingHost::new(host, page),
                audio,
            )
        });
        #[cfg(desktop)]
        let native = native.map(|host| {
            let audio = host.audio.clone();
            (host, audio)
        });
        let state = match native {
            Ok((host, audio)) => {
                let mut registry = ProviderRegistry::new();
                let caps = ProviderCapabilities {
                    playback: CapabilityStatus::Supported,
                    ..ProviderCapabilities::unverified()
                };
                registry
                    .register(ProviderId::YouTube, caps)
                    .expect("fresh registry");
                let hosts: HashMap<_, Box<dyn crate::core::playback::host::PlayerHost>> =
                    HashMap::from([(ProviderId::YouTube, Box::new(host) as _)]);
                let mut state = Self::new(PlaybackEngine::new(
                    registry,
                    hosts,
                    Duration::from_secs(55),
                ));
                state.audio = audio;
                state.available = true;
                state
            }
            Err(kind) => {
                // Category only, like the other startup diagnostics.
                eprintln!("native player unavailable: {kind:?}");
                Self::new(PlaybackEngine::production())
            }
        };
        app.manage(state);
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            let mut last_history_track = None;
            // The play being timed: its id and when listening time was last added.
            let mut listening: Option<(String, std::time::Instant)> = None;
            let mut last_flush = std::time::Instant::now();
            #[cfg(mobile)]
            let mut recovery = Recovery::default();
            while let Some(report) = receiver.recv().await {
                let state = app.state::<PlayerState>();
                let mut engine = state.engine.lock().await;
                if report.revision != engine.revision() {
                    continue;
                }
                // A stream that dies mid-track (an expired link, a cut connection, a short file) is loaded again from
                // where it stopped before the user ever sees an error.
                #[cfg(mobile)]
                if let Some(track) = engine.active_track().cloned() {
                    if recovery.id != track.provider_track_id
                        || (engine.state() == PlaybackState::Playing
                            && report.position_ms > recovery.at_ms + RECOVERY_RESET_MS)
                    {
                        recovery = Recovery {
                            id: track.provider_track_id.clone(),
                            ..Recovery::default()
                        };
                    }
                    if matches!(report.event, Some(PlayerEvent::Failed))
                        && recovery.attempts < MAX_RECOVERIES
                    {
                        recovery.attempts += 1;
                        recovery.at_ms = report.position_ms;
                        drop(engine);
                        let (app, position_ms) = (app.clone(), report.position_ms);
                        tauri::async_runtime::spawn(async move {
                            recover(&app, track, position_ms).await;
                        });
                        continue;
                    }
                }
                if let Some(event) = report.event {
                    let before = snapshot(&engine);
                    let result = engine.observe(report.revision, event).map(|_| ());
                    let _ = finish(&app, settle(&engine, before, result));
                }
                *state.progress.lock().await = (report.position_ms, report.duration_ms);
                state.reports.send_replace(report.revision);
                let history_pending = engine.state() == PlaybackState::Playing
                    && state
                        .history_pending
                        .swap(false, std::sync::atomic::Ordering::Relaxed);
                let history_track = if engine.state() == PlaybackState::Playing {
                    engine
                        .active_track()
                        .filter(|t| {
                            last_history_track.as_ref() != Some(&t.provider_track_id)
                                || history_pending
                        })
                        .map(|t| crate::core::library::SavedTrack {
                            provider: t.provider.clone(),
                            provider_track_id: t.provider_track_id.clone(),
                            canonical_url: t.canonical_url.clone(),
                            title: t.title.clone(),
                            artists: t.artists.clone(),
                            artist_refs: vec![],
                            artwork_url: t.artwork_url.clone(),
                            duration_ms: (report.duration_ms > 0)
                                .then_some(report.duration_ms)
                                .or(t.duration_ms),
                            source: None,
                        })
                } else {
                    None
                };
                let playing_id = (engine.state() == PlaybackState::Playing)
                    .then(|| engine.active_track().map(|t| t.provider_track_id.clone()))
                    .flatten();
                drop(engine);
                let dto = progress(&state).await;
                crate::platform::discord::offer(&dto);
                let _ = app.emit_to("main", "core://player/progress", dto);
                if let Some(track) = history_track {
                    let log = app.state::<crate::commands::stats::AnalyticsState>();
                    let now_ms = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(0, |d| d.as_millis() as u64);
                    log.with(|l| {
                        l.begin(
                            format!("{:?}", track.provider),
                            track.provider_track_id.clone(),
                            now_ms,
                            track.duration_ms.unwrap_or(0),
                        );
                        Ok(())
                    })
                    .ok();
                    listening = Some((track.provider_track_id.clone(), std::time::Instant::now()));
                    app.state::<crate::commands::telemetry::TelemetryState>()
                        .record("feature.play");
                    let counts = app.state::<crate::commands::stats::StatsState>();
                    if let Err(kind) = counts.with(|s| s.record(track.clone())) {
                        eprintln!("listening stats unavailable: {kind:?}");
                    }
                    last_history_track = Some(track.provider_track_id.clone());
                    let history = app.state::<crate::commands::library::HistoryState>();
                    match history.0.with(|store| store.record_play(track)) {
                        Ok(tracks) => {
                            let _ = app.emit_to("main", "core://history/changed", tracks);
                        }
                        Err(kind) => {
                            eprintln!("play history unavailable: {kind:?}");
                            let _ = app.emit_to("main", "core://history/error", ());
                        }
                    }
                }
                // Listening time only grows while the track is actually playing; a pause or seek gap is never added.
                if let Some((id, last)) = &mut listening {
                    let now = std::time::Instant::now();
                    if playing_id.as_deref() == Some(id.as_str()) {
                        let ms = now.duration_since(*last).as_millis().min(5_000) as u64;
                        let log = app.state::<crate::commands::stats::AnalyticsState>();
                        log.with(|l| {
                            l.add_listened(id, ms);
                            Ok(())
                        })
                        .ok();
                    }
                    *last = now;
                }
                if last_flush.elapsed() >= Duration::from_secs(20) {
                    last_flush = std::time::Instant::now();
                    let log = app.state::<crate::commands::stats::AnalyticsState>();
                    let _ = log.with(|l| l.flush());
                }
                crate::platform::poke();
            }
        });
    }

    fn new(engine: PlaybackEngine) -> Self {
        Self {
            engine: std::sync::Arc::new(Mutex::new(engine)),
            autoplay: Mutex::new(None),
            reports: tokio::sync::watch::channel(0).0,
            autoplay_status: Default::default(),
            progress: Mutex::new((0, 0)),
            volume: std::sync::atomic::AtomicU8::new(DEFAULT_VOLUME),
            history_pending: std::sync::atomic::AtomicBool::new(true),
            available: false,
            audio: Default::default(),
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LoadedTrackDto {
    provider: String,
    pub(crate) provider_track_id: String,
    canonical_url: String,
    pub(crate) title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) artists: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) artwork_url: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerProgressDto {
    pub(crate) player: PlayerStateDto,
    pub(crate) track: Option<LoadedTrackDto>,
    pub(crate) position_ms: u64,
    pub(crate) duration_ms: u64,
    available: bool,
    /// Last volume core accepted, so the slider follows menu and media-key changes.
    pub(crate) volume: u8,
    autoplay: crate::core::playback::autoplay::AutoplayStatus,
}

// Only the macOS media tests build one of these; elsewhere it would be dead code.
#[cfg(all(test, target_os = "macos"))]
impl PlayerProgressDto {
    pub(crate) fn for_test(
        player: PlayerStateDto,
        title: Option<String>,
        position_ms: u64,
        duration_ms: u64,
    ) -> Self {
        Self {
            player,
            track: title.map(|title| LoadedTrackDto {
                provider: "youtube".into(),
                provider_track_id: "aqz-KE-bpKQ".into(),
                canonical_url: String::new(),
                title,
                artists: Vec::new(),
                artwork_url: None,
            }),
            position_ms,
            duration_ms,
            available: true,
            volume: DEFAULT_VOLUME,
            autoplay: Default::default(),
        }
    }
}

pub(crate) async fn progress(state: &PlayerState) -> PlayerProgressDto {
    let engine = state.engine.lock().await;
    let (position_ms, duration_ms) = *state.progress.lock().await;
    PlayerProgressDto {
        player: snapshot(&engine),
        available: state.available,
        position_ms,
        duration_ms,
        volume: state.volume.load(std::sync::atomic::Ordering::Relaxed),
        autoplay: state
            .autoplay_status
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default(),
        track: engine.active_track().map(|t| LoadedTrackDto {
            provider: t.provider.as_str().into(),
            provider_track_id: t.provider_track_id.clone(),
            canonical_url: t.canonical_url.clone(),
            title: t.title.clone(),
            artists: t.artists.clone(),
            artwork_url: t.artwork_url.clone(),
        }),
    }
}

#[tauri::command]
pub async fn player_progress(
    state: State<'_, PlayerState>,
) -> Result<PlayerProgressDto, PlayerError> {
    Ok(progress(&state).await)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlaybackStateDto {
    Idle,
    Resolving,
    Preparing,
    Ready,
    Playing,
    Paused,
    Buffering,
    Blocked,
    Ended,
    Error,
}

impl From<PlaybackState> for PlaybackStateDto {
    fn from(s: PlaybackState) -> Self {
        match s {
            PlaybackState::Idle => Self::Idle,
            PlaybackState::Resolving => Self::Resolving,
            PlaybackState::Preparing => Self::Preparing,
            PlaybackState::Ready => Self::Ready,
            PlaybackState::Playing => Self::Playing,
            PlaybackState::Paused => Self::Paused,
            PlaybackState::Buffering => Self::Buffering,
            PlaybackState::Blocked => Self::Blocked,
            PlaybackState::Ended => Self::Ended,
            PlaybackState::Error => Self::Error,
        }
    }
}

/// Command return value and `core://player/state` payload. `revision` is the engine revision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerStateDto {
    pub version: u8,
    pub state: PlaybackStateDto,
    pub revision: u64,
}

fn snapshot(engine: &PlaybackEngine) -> PlayerStateDto {
    PlayerStateDto {
        version: PAYLOAD_VERSION,
        state: engine.state().into(),
        revision: engine.revision(),
    }
}

/// Fixed categories only. `message` is fixed text per category, never backend text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayerErrorCategory {
    Validation,
    InvalidTransition,
    UnsupportedCapability,
    StorageInUse,
    StorageUnavailable,
    NoActiveProvider,
    CrossProviderUnsupported,
    QueueBoundary,
    HostRejected,
    Timeout,
    StaleOutcome,
    Internal,
    VerificationRequired,
    MediaRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerError {
    pub category: PlayerErrorCategory,
    pub message: &'static str,
}

impl PlayerError {
    fn new(category: PlayerErrorCategory) -> Self {
        use PlayerErrorCategory::*;
        let message = match category {
            Validation => "invalid player argument",
            InvalidTransition => "command not valid in the current playback state",
            UnsupportedCapability => "playback is not available for this provider",
            StorageInUse => "media storage is open in another app instance",
            StorageUnavailable => "media storage could not be opened",
            NoActiveProvider => "no track is loaded",
            CrossProviderUnsupported => "switching providers is not supported",
            QueueBoundary => "no track in that direction",
            HostRejected => "the player rejected the request",
            Timeout => "the player did not respond in time",
            StaleOutcome => "the player report is out of date",
            Internal => "playback failed",
            VerificationRequired => {
                "YouTube asked for verification; sign in with Google in Connections"
            }
            MediaRefused => {
                "the media host refused the audio bytes; YouTube serves a stream in full only to a signed-in session"
            }
        };
        Self { category, message }
    }
}

fn cache_startup_error(error: crate::core::playback::cache::CacheError) -> PlayerError {
    use crate::core::playback::cache::CacheError;
    PlayerError::new(if matches!(error, CacheError::InUse) {
        PlayerErrorCategory::StorageInUse
    } else {
        PlayerErrorCategory::StorageUnavailable
    })
}

fn invalid() -> PlayerError {
    PlayerError::new(PlayerErrorCategory::Validation)
}

fn from_core(e: &CoreError) -> PlayerError {
    use PlayerErrorCategory as C;
    let category = match e {
        CoreError::Playback(kind) => match kind {
            PlaybackErrorKind::InvalidTransition { .. } => C::InvalidTransition,
            PlaybackErrorKind::UnsupportedCapability => C::UnsupportedCapability,
            PlaybackErrorKind::NoActiveProvider => C::NoActiveProvider,
            PlaybackErrorKind::CrossProviderUnsupported => C::CrossProviderUnsupported,
            PlaybackErrorKind::QueueBoundary => C::QueueBoundary,
            PlaybackErrorKind::HostRejected(HostRejectKind::Timeout) => C::Timeout,
            PlaybackErrorKind::HostRejected(HostRejectKind::Failed) => C::HostRejected,
            PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired) => {
                C::VerificationRequired
            }
            PlaybackErrorKind::HostRejected(HostRejectKind::MediaRefused) => C::MediaRefused,
            PlaybackErrorKind::StaleOutcome => C::StaleOutcome,
            PlaybackErrorKind::Validation(_) => C::Validation,
        },
        CoreError::Validation(_) => C::Validation,
        CoreError::UnsupportedCapability => C::UnsupportedCapability,
        _ => C::Internal,
    };
    PlayerError::new(category)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerLoadArgs {
    #[serde(default)]
    pub queue_selection: bool,
    #[serde(default)]
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<u64>,
    pub provider: String,
    pub provider_track_id: String,
    pub canonical_url: String,
    pub title: String,
    /// Tracks to play after this one (an album or playlist), in order. Ignored for queue selections.
    #[serde(default)]
    pub following: Vec<FollowingTrack>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FollowingTrack {
    pub provider_track_id: String,
    pub title: String,
    #[serde(default)]
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
}

const MAX_FOLLOWING: usize = 100;

fn validate_following(
    tracks: Vec<FollowingTrack>,
) -> Result<Vec<crate::core::playback::cache::OfflineTrack>, PlayerError> {
    if tracks.len() > MAX_FOLLOWING {
        return Err(invalid());
    }
    tracks
        .into_iter()
        .map(|t| {
            if !crate::core::playback::extract::valid_id(&t.provider_track_id)
                || t.title.trim().is_empty()
                || t.title.len() > MAX_TITLE_BYTES
                || t.artists.len() > 30
                || t.artists.iter().any(|a| a.len() > 512)
            {
                return Err(invalid());
            }
            Ok(crate::core::playback::cache::OfflineTrack {
                provider: "YouTube".into(),
                id: t.provider_track_id,
                title: t.title,
                artists: t.artists,
                artwork_url: t
                    .artwork_url
                    .as_deref()
                    .and_then(crate::core::playback::track_info::artwork_url),
                duration_ms: t.duration_ms,
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlayerEventArg {
    Buffering,
    Playing,
    Ended,
    Blocked,
}

impl From<PlayerEventArg> for PlayerEvent {
    fn from(e: PlayerEventArg) -> Self {
        match e {
            PlayerEventArg::Buffering => Self::Buffering,
            PlayerEventArg::Playing => Self::Playing,
            PlayerEventArg::Ended => Self::Ended,
            PlayerEventArg::Blocked => Self::Blocked,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerObserveArgs {
    pub revision: i64,
    pub event: PlayerEventArg,
}

// Numbers arrive as i64 so out-of-range values reach these checks, not a serde message.
fn validate_text(value: &str, max_bytes: usize) -> Result<String, PlayerError> {
    let t = value.trim();
    if t.is_empty() || t.len() > max_bytes {
        return Err(invalid());
    }
    Ok(t.to_owned())
}

fn validate_load(args: PlayerLoadArgs) -> Result<TrackReference, PlayerError> {
    let provider = ProviderId::from_str(&args.provider).map_err(|_| invalid())?;
    let id = validate_text(&args.provider_track_id, MAX_ID_BYTES)?;
    let url = validate_text(&args.canonical_url, MAX_URL_BYTES)?;
    if args.title.len() > MAX_TITLE_BYTES {
        return Err(invalid());
    }
    if args.artists.len() > 30
        || args.artists.iter().any(|a| a.len() > 512)
        || args.artwork_url.as_ref().is_some_and(|a| a.len() > 2048)
    {
        return Err(invalid());
    }
    let mut track =
        TrackReference::new(provider, id, url, args.title).map_err(|e| from_core(&e))?;
    track.artists = args.artists;
    track.artwork_url = args
        .artwork_url
        .as_deref()
        .and_then(crate::core::playback::track_info::artwork_url);
    track.duration_ms = args.duration_ms;
    Ok(track)
}

fn validate_seek(position_ms: i64) -> Result<u64, PlayerError> {
    if (0..=MAX_SEEK_MS).contains(&position_ms) {
        Ok(position_ms as u64)
    } else {
        Err(invalid())
    }
}

fn validate_volume(volume: i64) -> Result<Volume, PlayerError> {
    u8::try_from(volume)
        .ok()
        .and_then(|v| Volume::new(v).ok())
        .ok_or_else(invalid)
}

fn validate_revision(revision: i64) -> Result<u64, PlayerError> {
    u64::try_from(revision).map_err(|_| invalid())
}

/// Result of one command. `emit` is set only when the observable state changed; the caller emits it
/// after the engine lock is released.
pub(crate) struct Reply {
    pub(crate) result: Result<PlayerStateDto, PlayerError>,
    emit: Option<PlayerStateDto>,
}

impl Reply {
    fn rejected(e: PlayerError) -> Self {
        Self {
            result: Err(e),
            emit: None,
        }
    }
}

fn settle(engine: &PlaybackEngine, before: PlayerStateDto, result: Result<(), CoreError>) -> Reply {
    let now = snapshot(engine);
    let emit = (now != before).then(|| now.clone());
    let result = match result {
        Ok(()) => Ok(now),
        Err(e) => Err(from_core(&e)),
    };
    Reply { result, emit }
}

// One engine call under the lock. Errors can change state too (a host rejection moves it to Error), so
// the state comparison runs on both outcomes.
pub(crate) async fn dispatch(state: &PlayerState, cmd: PlaybackCommand) -> Reply {
    // Serialize Stop with manual Load, including Stop from macOS media controls.
    let mut autoplay = if matches!(cmd, PlaybackCommand::Stop) {
        Some(state.autoplay.lock().await)
    } else {
        None
    };
    if let Some(slot) = autoplay.as_mut() {
        if let Some(task) = slot.take() {
            task.abort();
        }
        if let Ok(mut status) = state.autoplay_status.lock() {
            *status = Default::default();
        }
    }

    let volume = match &cmd {
        PlaybackCommand::SetVolume(v) => Some(v.value()),
        _ => None,
    };
    let mut engine = state.engine.lock().await;
    let before = snapshot(&engine);
    let result = engine.dispatch(cmd).await.map(|_| ());
    if let (Some(v), Ok(())) = (volume, &result) {
        state.volume.store(v, std::sync::atomic::Ordering::Relaxed);
    }
    let reply = settle(&engine, before, result);
    drop(autoplay);
    reply
}

async fn observe(state: &PlayerState, revision: u64, event: PlayerEvent) -> Reply {
    let mut engine = state.engine.lock().await;
    let before = snapshot(&engine);
    let result = engine.observe(revision, event).map(|_| ());
    settle(&engine, before, result)
}

// The event is emitted after the lock is released. Two concurrent commands can emit out of order,
// so the webview treats each command's return value as authoritative.
pub(crate) fn finish(app: &AppHandle, reply: Reply) -> Result<PlayerStateDto, PlayerError> {
    crate::platform::poke();
    if let Err(e) = &reply.result {
        use PlayerErrorCategory as C;
        // Caller mistakes and races are not product errors; the rest are counted by type only.
        if !matches!(
            e.category,
            C::Validation | C::InvalidTransition | C::StaleOutcome
        ) {
            app.state::<crate::commands::telemetry::TelemetryState>()
                .record(&format!(
                    "error.playback.{}",
                    format!("{:?}", e.category).to_lowercase()
                ));
        }
    }
    if let Some(dto) = reply.emit {
        if matches!(dto.state, PlaybackStateDto::Ready | PlaybackStateDto::Idle) {
            let _ = app.emit_to("main", "core://offline/changed", ());
        }
        if app.emit_to("main", STATE_EVENT, dto).is_err() {
            eprintln!("player state event not delivered");
        }
    }
    reply.result
}

#[cfg(test)]
async fn run_load(state: &PlayerState, args: PlayerLoadArgs) -> Reply {
    match validate_load(args) {
        Ok(track) => dispatch(state, PlaybackCommand::Load(track)).await,
        Err(e) => Reply::rejected(e),
    }
}

pub(crate) async fn run_seek(state: &PlayerState, position_ms: i64) -> Reply {
    match validate_seek(position_ms) {
        Ok(p) => dispatch(state, PlaybackCommand::Seek { position_ms: p }).await,
        Err(e) => Reply::rejected(e),
    }
}

pub(crate) async fn run_set_volume(state: &PlayerState, volume: i64) -> Reply {
    match validate_volume(volume) {
        Ok(v) => dispatch(state, PlaybackCommand::SetVolume(v)).await,
        Err(e) => Reply::rejected(e),
    }
}

async fn run_observe(state: &PlayerState, args: PlayerObserveArgs) -> Reply {
    match validate_revision(args.revision) {
        Ok(rev) => observe(state, rev, args.event.into()).await,
        Err(e) => Reply::rejected(e),
    }
}

#[tauri::command]
pub async fn player_state(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, dispatch(&state, PlaybackCommand::GetState).await)
}

#[tauri::command]
pub async fn player_load(
    app: AppHandle,
    state: State<'_, PlayerState>,
    mut args: PlayerLoadArgs,
) -> Result<PlayerStateDto, PlayerError> {
    if let Err(error) = &app
        .state::<crate::commands::offline::OfflineState>()
        .manager
    {
        return Err(cache_startup_error(*error));
    }
    let queue_selection = args.queue_selection;
    let following = if queue_selection {
        Vec::new()
    } else {
        validate_following(std::mem::take(&mut args.following))?
    };
    let mut track = validate_load(args)?;
    if queue_selection {
        track = crate::core::playback::autoplay::select(
            &state.autoplay_status,
            &track.provider_track_id,
        )
        .ok_or_else(invalid)?;
    }
    load_track(&app, &state, track, queue_selection, true, following).await
}

/// Next/Previous from the OS widget and media keys. Nothing to skip to is not an error.
pub(crate) async fn skip(app: &AppHandle, forward: bool) {
    use crate::core::playback::autoplay::{pop_back, select_first};
    let state = app.state::<PlayerState>();
    let (track, queue_selection) = if forward {
        // With repeat-all and nothing left in the queue, Next starts the list again.
        select_first(&state.autoplay_status)
            .map(|t| (Some(t), true))
            .unwrap_or_else(|| {
                (
                    crate::core::playback::autoplay::restart_context(&state.autoplay_status),
                    true,
                )
            })
    } else {
        (pop_back(&state.autoplay_status), false)
    };
    if let Some(track) = track {
        // Skipping starts the track: a paused or ended player must not leave the new track sitting at 0:00.
        if load_track(app, &state, track, queue_selection, forward, Vec::new())
            .await
            .is_ok()
        {
            let _ = finish(app, dispatch(&state, PlaybackCommand::Play).await);
        }
    }
}

/// How often one track is loaded again after the player reports a failure, before the failure is shown.
#[cfg(mobile)]
const MAX_RECOVERIES: u8 = 2;
/// Playing this much further than the last failure counts as healthy again, so the next failure gets fresh attempts.
#[cfg(mobile)]
const RECOVERY_RESET_MS: u64 = 20_000;

#[cfg(mobile)]
#[derive(Default)]
struct Recovery {
    id: String,
    attempts: u8,
    at_ms: u64,
}

/// Loads the track again (which resolves a fresh link and may use the page player) and resumes from `position_ms`.
/// If the load is refused, its error is published like any other.
#[cfg(mobile)]
async fn recover(app: &AppHandle, track: TrackReference, position_ms: u64) {
    let state = app.state::<PlayerState>();
    tokio::time::sleep(Duration::from_millis(400)).await;
    let ready = load_track(app, &state, track, true, false, Vec::new())
        .await
        .is_ok()
        && finish(app, dispatch(&state, PlaybackCommand::Play).await).is_ok();
    if ready && position_ms > 2_000 {
        let _ = finish(
            app,
            dispatch(&state, PlaybackCommand::Seek { position_ms }).await,
        );
    }
}

async fn load_track(
    app: &AppHandle,
    state: &PlayerState,
    track: TrackReference,
    queue_selection: bool,
    record_back: bool,
    following: Vec<crate::core::playback::cache::OfflineTrack>,
) -> Result<PlayerStateDto, PlayerError> {
    let mut task = state.autoplay.lock().await;
    if record_back {
        let current = state.engine.lock().await.active_track().cloned();
        if let Some(current) = current
            && current.provider_track_id != track.provider_track_id
        {
            crate::core::playback::autoplay::push_back(&state.autoplay_status, current);
        }
    }
    if let Some(previous) = task.take() {
        previous.abort();
    }
    if !queue_selection {
        if let Ok(mut status) = state.autoplay_status.lock() {
            status.reset();
            status.queue = following.clone();
        }
        crate::core::playback::autoplay::set_context(&state.autoplay_status, &track, &following);
    }
    let result = finish(app, dispatch(state, PlaybackCommand::Load(track)).await);
    if result.is_ok() {
        state
            .history_pending
            .store(true, std::sync::atomic::Ordering::Relaxed);
        let yt = app
            .state::<crate::commands::youtube::YouTubeState>()
            .yt
            .clone();
        let cache = app
            .state::<crate::commands::offline::OfflineState>()
            .manager
            .as_ref()
            .ok()
            .map(|m| m.cache.clone());
        if let (Some(yt), Some(cache)) = (yt, cache) {
            // Autoplay prefetches the next track's lyrics into the same cache the UI reads.
            if let Ok(root) = app.path().app_cache_dir() {
                crate::core::playback::track_info::set_cache_root(root);
            }
            let session = crate::core::playback::autoplay::run(
                state.engine.clone(),
                yt,
                cache,
                state.audio.clone(),
                state.reports.subscribe(),
                state.autoplay_status.clone(),
            );
            let app = app.clone();
            let job = tauri::async_runtime::spawn(async move {
                session.await;
                // A failed preparation may produce no native report; publish its final state too.
                let state = app.state::<PlayerState>();
                let dto = progress(&state).await;
                let _ = app.emit_to("main", STATE_EVENT, &dto.player);
                crate::platform::discord::offer(&dto);
                let _ = app.emit_to("main", "core://player/progress", dto);
            });
            *task = Some(job);
        }
    }
    result
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerModeArgs {
    /// "off", "all" or "one".
    pub repeat: String,
    pub autoplay: bool,
}

/// Repeat and autoplay choices. They apply to the current track's ending and to every later track.
#[tauri::command]
pub async fn player_set_mode(
    app: AppHandle,
    state: State<'_, PlayerState>,
    args: PlayerModeArgs,
) -> Result<(), PlayerError> {
    if !crate::core::playback::autoplay::set_mode(
        &state.autoplay_status,
        &args.repeat,
        args.autoplay,
    ) {
        return Err(invalid());
    }
    // Wake a running autoplay session so it plans the current track's ending again.
    state.reports.send_modify(|v| *v = v.wrapping_add(0));
    let dto = progress(&state).await;
    crate::platform::discord::offer(&dto);
    let _ = app.emit_to("main", "core://player/progress", dto);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerQueueEditArgs {
    /// "next", "remove" or "move".
    pub op: String,
    /// For "next": the track to play after the current one.
    pub track: Option<crate::core::playback::cache::OfflineTrack>,
    /// For "remove": the queued track's video id.
    pub id: Option<String>,
    pub from: Option<usize>,
    pub to: Option<usize>,
}

/// Plays a track next, removes a queued track, or moves one inside the up-next list.
#[tauri::command]
pub async fn player_queue_edit(
    app: AppHandle,
    state: State<'_, PlayerState>,
    args: PlayerQueueEditArgs,
) -> Result<(), PlayerError> {
    use crate::core::playback::autoplay::{QueueEdit, edit_queue};
    let edit = match (args.op.as_str(), args.track, args.id, args.from, args.to) {
        ("next", Some(mut t), ..) => {
            if t.provider != "YouTube"
                || !crate::core::playback::extract::valid_id(&t.id)
                || t.title.trim().is_empty()
                || t.title.len() > 2048
                || t.artists.len() > 30
            {
                return Err(invalid());
            }
            t.artwork_url = t
                .artwork_url
                .as_deref()
                .and_then(crate::core::playback::track_info::artwork_url);
            QueueEdit::Next(t)
        }
        ("remove", _, Some(id), ..) if crate::core::playback::extract::valid_id(&id) => {
            QueueEdit::Remove(id)
        }
        ("move", _, _, Some(from), Some(to)) => QueueEdit::Move { from, to },
        ("shuffle", ..) => QueueEdit::Shuffle,
        _ => return Err(invalid()),
    };
    if !edit_queue(&state.autoplay_status, edit) {
        return Err(invalid());
    }
    // Wake a running autoplay session so it prepares the new head.
    state.reports.send_modify(|v| *v = v.wrapping_add(0));
    let dto = progress(&state).await;
    let _ = app.emit_to("main", "core://player/progress", dto);
    Ok(())
}

/// Adds more related songs to the end of the up-next list. The webview asks for them as the user scrolls down.
#[tauri::command]
pub async fn player_queue_more(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<usize, PlayerError> {
    let Some(yt) = app
        .state::<crate::commands::youtube::YouTubeState>()
        .yt
        .clone()
    else {
        return Err(PlayerError::new(PlayerErrorCategory::UnsupportedCapability));
    };
    let added = crate::core::playback::autoplay::load_more(&state.autoplay_status, &yt).await;
    if added > 0 {
        let dto = progress(&state).await;
        let _ = app.emit_to("main", "core://player/progress", dto);
    }
    Ok(added)
}

#[tauri::command]
pub async fn player_play(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, dispatch(&state, PlaybackCommand::Play).await)
}

#[tauri::command]
pub async fn player_pause(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, dispatch(&state, PlaybackCommand::Pause).await)
}

#[tauri::command]
pub async fn player_resume(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, dispatch(&state, PlaybackCommand::Resume).await)
}

#[tauri::command]
pub async fn player_stop(
    app: AppHandle,
    state: State<'_, PlayerState>,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, dispatch(&state, PlaybackCommand::Stop).await)
}

/// Next or Previous from the player bar. Previous restarts the track when it is more than 3 s in, like the
/// media keys. Nothing to skip to is not an error.
#[tauri::command]
pub async fn player_skip(
    app: AppHandle,
    state: State<'_, PlayerState>,
    forward: bool,
) -> Result<(), PlayerError> {
    let now = progress(&state).await;
    if !forward && now.position_ms > 3_000 {
        let _ = run_seek(&state, 0).await;
        // Restarting from a paused or ended player resumes it, like Next and Previous do.
        if now.player.state != PlaybackStateDto::Playing {
            let _ = finish(&app, dispatch(&state, PlaybackCommand::Play).await);
        }
    } else {
        skip(&app, forward).await;
    }
    let dto = progress(&state).await;
    crate::platform::discord::offer(&dto);
    let _ = app.emit_to("main", "core://player/progress", dto);
    Ok(())
}

#[tauri::command]
pub async fn player_seek(
    app: AppHandle,
    state: State<'_, PlayerState>,
    position_ms: i64,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, run_seek(&state, position_ms).await)
}

#[tauri::command]
pub async fn player_set_volume(
    app: AppHandle,
    state: State<'_, PlayerState>,
    volume: i64,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, run_set_volume(&state, volume).await)
}

#[tauri::command]
pub async fn player_observe(
    app: AppHandle,
    state: State<'_, PlayerState>,
    args: PlayerObserveArgs,
) -> Result<PlayerStateDto, PlayerError> {
    finish(&app, run_observe(&state, args).await)
}

const YOUTUBE_ID_LEN: usize = 11;
const YOUTUBE_WATCH_PREFIX: &str = "https://www.youtube.com/watch?v=";

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerOpenExternalArgs {
    pub provider: String,
    pub provider_track_id: String,
    pub canonical_url: String,
}

/// Fixed categories only. The webview shows its own copy for each.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum OpenExternalError {
    Validation,
    UnsupportedProvider,
    OpenFailed,
}

// The only URL this command may open. Built here from a validated id; canonicalUrl is only checked.
fn build_youtube_watch_url(args: &PlayerOpenExternalArgs) -> Result<String, OpenExternalError> {
    let provider =
        ProviderId::from_str(&args.provider).map_err(|_| OpenExternalError::Validation)?;
    if provider != ProviderId::YouTube {
        return Err(OpenExternalError::UnsupportedProvider);
    }
    let id = &args.provider_track_id;
    let id_ok = id.len() == YOUTUBE_ID_LEN
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !id_ok {
        return Err(OpenExternalError::Validation);
    }
    let url = format!("{YOUTUBE_WATCH_PREFIX}{id}");
    if !args.canonical_url.is_empty() && args.canonical_url != url {
        return Err(OpenExternalError::Validation);
    }
    Ok(url)
}

/// Opens the YouTube watch page in the default browser through tauri-plugin-opener. Allowed for YouTube only.
#[tauri::command]
pub async fn player_open_external(
    app: AppHandle,
    args: PlayerOpenExternalArgs,
) -> Result<(), OpenExternalError> {
    let url = build_youtube_watch_url(&args)?;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|_| OpenExternalError::OpenFailed)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::time::Duration;

    use super::*;
    use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
    use crate::core::playback::host::{HostFuture, HostOutcome, HostRequest, PlayerHost};
    use crate::core::registry::ProviderRegistry;

    fn args(provider: &str, id: &str, url: &str) -> PlayerLoadArgs {
        PlayerLoadArgs {
            queue_selection: false,
            artists: vec![],
            artwork_url: None,
            duration_ms: None,
            provider: provider.into(),
            provider_track_id: id.into(),
            canonical_url: url.into(),
            title: "t".into(),
            following: vec![],
        }
    }

    #[test]
    fn following_tracks_are_validated_and_bounded() {
        let ok = |id: &str| FollowingTrack {
            provider_track_id: id.into(),
            title: "t".into(),
            artists: vec![],
            artwork_url: None,
            duration_ms: None,
        };
        assert_eq!(
            validate_following(vec![ok("r-G2pKr2_n8")]).unwrap().len(),
            1
        );
        assert!(validate_following(vec![ok("bad id")]).is_err());
        let many = (0..=MAX_FOLLOWING).map(|_| ok("r-G2pKr2_n8")).collect();
        assert!(validate_following(many).is_err());
    }

    fn valid_args() -> PlayerLoadArgs {
        args("YouTube", "vid1", "https://youtu.be/vid1")
    }

    fn category_of(r: Result<TrackReference, PlayerError>) -> PlayerErrorCategory {
        r.unwrap_err().category
    }

    /// Answers every command with a rejection, the way a host that refuses playback would.
    struct RefusingHost;

    impl PlayerHost for RefusingHost {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }

        fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
            Box::pin(async move {
                HostOutcome::Rejected {
                    request_id: req.request_id,
                    revision: req.revision,
                    kind: PlaybackErrorKind::HostRejected(HostRejectKind::Failed),
                }
            })
        }
    }

    // Playback Supported so the engine reaches the host, unlike production() where it is refused first.
    fn engine_with_host(host: Box<dyn PlayerHost>) -> PlaybackEngine {
        let mut registry = ProviderRegistry::new();
        let caps = ProviderCapabilities {
            playback: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        registry.register(ProviderId::YouTube, caps).unwrap();
        PlaybackEngine::new(
            registry,
            HashMap::from([(ProviderId::YouTube, host)]),
            Duration::from_secs(8),
        )
    }

    #[test]
    fn load_rejects_unknown_and_lowercase_provider() {
        for p in ["Spotify2", "youtube", "", "Deezer"] {
            let r = validate_load(args(p, "vid1", "https://youtu.be/vid1"));
            assert_eq!(category_of(r), PlayerErrorCategory::Validation, "{p:?}");
        }
    }

    #[test]
    fn load_rejects_blank_or_oversized_ids() {
        let blank = validate_load(args("YouTube", "   ", "https://youtu.be/x"));
        assert_eq!(category_of(blank), PlayerErrorCategory::Validation);
        let empty_url = validate_load(args("YouTube", "vid1", ""));
        assert_eq!(category_of(empty_url), PlayerErrorCategory::Validation);
        let long_id = "a".repeat(MAX_ID_BYTES + 1);
        let r = validate_load(args("YouTube", &long_id, "https://youtu.be/x"));
        assert_eq!(category_of(r), PlayerErrorCategory::Validation);
    }

    #[test]
    fn load_trims_ids_and_builds_track() {
        let t = validate_load(args("YouTube", "  vid1 ", " https://youtu.be/vid1 ")).unwrap();
        assert_eq!(t.provider, ProviderId::YouTube);
        assert_eq!(t.provider_track_id, "vid1");
        assert_eq!(t.canonical_url, "https://youtu.be/vid1");
    }

    #[test]
    fn seek_accepts_bounds_and_rejects_outside() {
        assert_eq!(validate_seek(0), Ok(0));
        assert_eq!(validate_seek(MAX_SEEK_MS), Ok(MAX_SEEK_MS as u64));
        assert_eq!(
            validate_seek(-1).unwrap_err().category,
            PlayerErrorCategory::Validation
        );
        assert_eq!(
            validate_seek(MAX_SEEK_MS + 1).unwrap_err().category,
            PlayerErrorCategory::Validation
        );
    }

    #[test]
    fn volume_accepts_zero_to_hundred_only() {
        assert_eq!(validate_volume(0).map(|v| v.value()), Ok(0));
        assert_eq!(validate_volume(100).map(|v| v.value()), Ok(100));
        for bad in [-1, 101, 256, i64::MAX] {
            assert_eq!(
                validate_volume(bad).unwrap_err().category,
                PlayerErrorCategory::Validation,
                "{bad}"
            );
        }
    }

    #[test]
    fn negative_revision_is_validation() {
        assert_eq!(
            validate_revision(-1).unwrap_err().category,
            PlayerErrorCategory::Validation
        );
        assert_eq!(validate_revision(7), Ok(7));
    }

    #[test]
    fn state_dto_serializes_camel_case_with_version() {
        let dto = PlayerStateDto {
            version: PAYLOAD_VERSION,
            state: PlaybackStateDto::Resolving,
            revision: 3,
        };
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            serde_json::json!({ "version": 1, "state": "resolving", "revision": 3 })
        );
    }

    #[test]
    fn error_serializes_only_category_and_fixed_message() {
        let e = PlayerError::new(PlayerErrorCategory::HostRejected);
        assert_eq!(
            serde_json::to_value(&e).unwrap(),
            serde_json::json!({ "category": "hostRejected", "message": "the player rejected the request" })
        );
    }

    #[test]
    fn cache_startup_errors_distinguish_lock_contention_from_audio_failure() {
        use crate::core::playback::cache::CacheError;
        assert_eq!(
            cache_startup_error(CacheError::InUse).category,
            PlayerErrorCategory::StorageInUse
        );
        assert_eq!(
            cache_startup_error(CacheError::Storage).category,
            PlayerErrorCategory::StorageUnavailable
        );
    }

    #[test]
    fn state_event_name_is_core_player_state() {
        assert_eq!(STATE_EVENT, "core://player/state");
    }

    #[tokio::test]
    async fn stop_cancels_pending_autoplay_and_clears_up_next() {
        struct Dropped(Option<tokio::sync::oneshot::Sender<()>>);
        impl Drop for Dropped {
            fn drop(&mut self) {
                if let Some(send) = self.0.take() {
                    let _ = send.send(());
                }
            }
        }
        let state = PlayerState::new(PlaybackEngine::production());
        let (dropped, cancelled) = tokio::sync::oneshot::channel();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tauri::async_runtime::spawn(async move {
            let _guard = Dropped(Some(dropped));
            let _ = started.send(());
            std::future::pending::<()>().await;
        });
        ready.await.unwrap();
        *state.autoplay.lock().await = Some(task);
        state.autoplay_status.lock().unwrap().next_title = Some("Next".into());
        dispatch(&state, PlaybackCommand::Stop).await;
        tokio::time::timeout(Duration::from_secs(2), cancelled)
            .await
            .unwrap()
            .unwrap();
        assert!(state.autoplay.lock().await.is_none());
        assert!(state.autoplay_status.lock().unwrap().next_title.is_none());
    }

    #[tokio::test]
    async fn production_load_is_refused_before_any_host_and_emits_nothing() {
        let state = PlayerState::new(PlaybackEngine::production());
        let reply = run_load(&state, valid_args()).await;
        assert_eq!(
            reply.result.unwrap_err().category,
            PlayerErrorCategory::UnsupportedCapability
        );
        assert_eq!(reply.emit, None);
        assert_eq!(state.engine.lock().await.state(), PlaybackState::Idle);
    }

    #[tokio::test]
    async fn refusing_host_load_returns_typed_error_and_moves_state_to_error() {
        let state = PlayerState::new(engine_with_host(Box::new(RefusingHost)));
        let reply = run_load(&state, valid_args()).await;
        let err = reply.result.unwrap_err();
        assert_eq!(err.category, PlayerErrorCategory::HostRejected);
        assert_eq!(err.message, "the player rejected the request");
        let emitted = reply
            .emit
            .expect("state changed to Error, so it must be emitted");
        assert_eq!(emitted.state, PlaybackStateDto::Error);
        assert_eq!(emitted.revision, 1);
        assert_eq!(state.engine.lock().await.state(), PlaybackState::Error);
    }

    #[tokio::test]
    async fn play_after_refused_load_is_invalid_and_emits_nothing() {
        let state = PlayerState::new(engine_with_host(Box::new(RefusingHost)));
        run_load(&state, valid_args()).await;
        let reply = dispatch(&state, PlaybackCommand::Play).await;
        assert_eq!(
            reply.result.unwrap_err().category,
            PlayerErrorCategory::InvalidTransition
        );
        assert_eq!(reply.emit, None);
    }

    #[tokio::test]
    async fn observe_with_stale_revision_is_stale_outcome() {
        let state = PlayerState::new(PlaybackEngine::production());
        let reply = run_observe(
            &state,
            PlayerObserveArgs {
                revision: 5,
                event: PlayerEventArg::Playing,
            },
        )
        .await;
        assert_eq!(
            reply.result.unwrap_err().category,
            PlayerErrorCategory::StaleOutcome
        );
        assert_eq!(reply.emit, None);
    }

    #[tokio::test]
    async fn bad_seek_argument_never_reaches_engine() {
        let state = PlayerState::new(PlaybackEngine::production());
        let reply = run_seek(&state, -5).await;
        assert_eq!(
            reply.result.unwrap_err().category,
            PlayerErrorCategory::Validation
        );
        assert_eq!(state.engine.lock().await.revision(), 0);
    }

    fn open_args(provider: &str, id: &str, url: &str) -> PlayerOpenExternalArgs {
        PlayerOpenExternalArgs {
            provider: provider.into(),
            provider_track_id: id.into(),
            canonical_url: url.into(),
        }
    }

    #[test]
    fn open_accepts_valid_id_and_builds_watch_url() {
        let url = build_youtube_watch_url(&open_args("YouTube", "dQw4w9WgXcQ", "")).unwrap();
        assert_eq!(url, "https://www.youtube.com/watch?v=dQw4w9WgXcQ");
        let with_chars = build_youtube_watch_url(&open_args("YouTube", "a_B-9zZ0-_x", "")).unwrap();
        assert_eq!(with_chars, "https://www.youtube.com/watch?v=a_B-9zZ0-_x");
    }

    #[test]
    fn open_accepts_exact_canonical_url_only() {
        let exact = "https://www.youtube.com/watch?v=dQw4w9WgXcQ";
        assert_eq!(
            build_youtube_watch_url(&open_args("YouTube", "dQw4w9WgXcQ", exact)).unwrap(),
            exact
        );
        for other in [
            "https://youtu.be/dQw4w9WgXcQ",
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ&t=1",
            "https://evil.example/watch?v=dQw4w9WgXcQ",
            " https://www.youtube.com/watch?v=dQw4w9WgXcQ",
        ] {
            assert_eq!(
                build_youtube_watch_url(&open_args("YouTube", "dQw4w9WgXcQ", other)),
                Err(OpenExternalError::Validation),
                "{other}"
            );
        }
    }

    #[test]
    fn open_rejects_wrong_id_length() {
        for id in ["", "dQw4w9WgXc", "dQw4w9WgXcQQ", "dQw4w9WgXcQ "] {
            assert_eq!(
                build_youtube_watch_url(&open_args("YouTube", id, "")),
                Err(OpenExternalError::Validation),
                "{id:?}"
            );
        }
    }

    #[test]
    fn open_rejects_bad_id_characters() {
        for id in [
            "dQw4w9WgX?Q",
            "dQw4w9WgX/Q",
            "dQw4w9WgX Q",
            "dQw4w9WgX&Q",
            "dQw4w9WgX\u{e9}",
            "dQw4w9WgX\n",
        ] {
            assert_eq!(
                build_youtube_watch_url(&open_args("YouTube", id, "")),
                Err(OpenExternalError::Validation),
                "{id:?}"
            );
        }
    }

    #[test]
    fn open_rejects_spotify_as_unsupported_provider() {
        assert_eq!(
            build_youtube_watch_url(&open_args("Spotify", "dQw4w9WgXcQ", "")),
            Err(OpenExternalError::UnsupportedProvider)
        );
    }

    #[test]
    fn open_rejects_unknown_provider_as_validation() {
        assert_eq!(
            build_youtube_watch_url(&open_args("youtube", "dQw4w9WgXcQ", "")),
            Err(OpenExternalError::Validation)
        );
    }

    #[test]
    fn open_error_serializes_only_category() {
        assert_eq!(
            serde_json::to_value(OpenExternalError::OpenFailed).unwrap(),
            serde_json::json!({ "category": "openFailed" })
        );
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSettingsDto {
    settings: crate::core::playback::audio::AudioSettings,
    output: crate::core::playback::audio::AudioOutput,
    available: bool,
}
#[tauri::command]
pub fn player_audio_settings(state: State<'_, PlayerState>) -> AudioSettingsDto {
    AudioSettingsDto {
        settings: state.audio.settings(),
        output: state
            .audio
            .output
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone(),
        available: state.available,
    }
}
/// The sound presets, in display order. One list for the settings panel and the macOS Sound menu.
#[tauri::command]
pub fn player_effect_presets() -> Vec<crate::core::playback::presets::Preset> {
    crate::core::playback::presets::all()
}

#[tauri::command]
pub fn player_set_audio_settings(
    app: AppHandle,
    state: State<'_, PlayerState>,
    settings: crate::core::playback::audio::AudioSettings,
) -> Result<AudioSettingsDto, PlayerError> {
    if !state.available {
        return Err(PlayerError::new(PlayerErrorCategory::UnsupportedCapability));
    }
    *state
        .audio
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = settings;
    crate::platform::sound_changed(&app);
    Ok(player_audio_settings(state))
}
