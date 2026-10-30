//! iOS / Android audio host. AVPlayer or Media3 plays the file; this side prepares the source and mirrors the
//! plugin's `poll` facts into engine reports. Only player confirmations settle engine commands, exactly like the
//! desktop `native` host (libmpv has no mobile build). Compiled for mobile targets only.
use super::{
    audio::{AudioControl, AudioSettings},
    command::PlaybackCommand,
    diag::stage_log,
    extract,
    host::{HostFuture, HostOutcome, HostRequest, PlayerEvent, PlayerHost},
    mobile_protocol::{Poll, end_event},
    proxy::AudioProxy,
};
use crate::core::yt::Yt;
use crate::core::{
    domain::ProviderId,
    errors::{HostRejectKind, PlaybackErrorKind},
};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Wry};
use tauri_plugin_lunor_native::LunorNativeExt;
use tokio::sync::mpsc::UnboundedSender;

/// Kept next to the desktop definition's shape so `commands::player` handles both hosts the same way.
pub struct NativeReport {
    pub revision: u64,
    pub event: Option<PlayerEvent>,
    pub position_ms: u64,
    pub duration_ms: u64,
}

const COMMAND_TIMEOUT: Duration = Duration::from_secs(12);

pub struct NativeHost {
    pub audio: AudioControl,
    app: AppHandle<Wry>,
    yt: Yt,
    cache: Option<Arc<super::cache::DiskCache>>,
    file: tokio::sync::Mutex<Option<Arc<super::cache::CachedFile>>>,
    proxy: tokio::sync::Mutex<Option<AudioProxy>>,
    revision: Arc<AtomicU64>,
}

impl NativeHost {
    pub fn with_cache(mut self, cache: Arc<super::cache::DiskCache>) -> Self {
        self.cache = Some(cache);
        self
    }

    pub fn with_yt(
        app: AppHandle<Wry>,
        yt: Yt,
        reports: UnboundedSender<NativeReport>,
    ) -> Result<Self, PlaybackErrorKind> {
        let revision = Arc::new(AtomicU64::new(0));
        tauri::async_runtime::spawn(watch(app.clone(), reports, revision.clone()));
        Ok(Self {
            audio: AudioControl::default(),
            app,
            yt,
            cache: None,
            file: tokio::sync::Mutex::new(None),
            proxy: tokio::sync::Mutex::new(None),
            revision,
        })
    }

    /// The fallback that plays through the provider's own page, sharing this host's report revision.
    pub fn page_host(&self) -> PageHost {
        PageHost {
            app: self.app.clone(),
            revision: self.revision.clone(),
        }
    }

    async fn plugin(
        &self,
        command: &'static str,
        payload: Value,
    ) -> Result<Value, PlaybackErrorKind> {
        call(&self.app, command, payload).await
    }
}

async fn call(
    app: &AppHandle<Wry>,
    command: &'static str,
    payload: Value,
) -> Result<Value, PlaybackErrorKind> {
    call_within(app, command, payload, COMMAND_TIMEOUT).await
}

async fn call_within(
    app: &AppHandle<Wry>,
    command: &'static str,
    payload: Value,
    limit: Duration,
) -> Result<Value, PlaybackErrorKind> {
    let app = app.clone();
    let work = tauri::async_runtime::spawn_blocking(move || {
        app.lunor_native().call::<Value>(command, payload)
    });
    match tokio::time::timeout(limit, work).await {
        Ok(Ok(Ok(value))) => {
            if command != "poll" {
                stage_log!("[debug] plugin {} ok", command);
            }
            Ok(value)
        }
        Ok(Ok(Err(e))) => {
            stage_log!("[debug] plugin {} rejected: {}", command, e);
            Err(extract::failed())
        }
        Ok(Err(e)) => {
            stage_log!("[debug] plugin {} panicked: {}", command, e);
            Err(extract::failed())
        }
        Err(_) => {
            stage_log!("[debug] plugin {} timed out", command);
            Err(PlaybackErrorKind::HostRejected(HostRejectKind::Timeout))
        }
    }
}

impl PlayerHost for NativeHost {
    fn provider(&self) -> ProviderId {
        ProviderId::YouTube
    }

    fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
        Box::pin(async move {
            let mut proxy = self.proxy.lock().await;
            let settings: AudioSettings = self.audio.settings();
            let is_stop = matches!(req.command, PlaybackCommand::Stop);
            self.revision.store(req.revision, Ordering::SeqCst);
            let result = async {
                match &req.command {
                    PlaybackCommand::Load(track) => {
                        stage_log!("[debug] mobile load: stop");
                        match self.plugin("stop", json!({})).await {
                            Ok(_) => stage_log!("[debug] mobile load: stop ok"),
                            Err(e) => {
                                stage_log!("[debug] mobile load: stop failed: {:?}", e);
                                return Err(e);
                            }
                        }
                        proxy.take();
                        self.file.lock().await.take();
                        let url = if let Some(cache) = &self.cache {
                            stage_log!("[debug] mobile load: cache");
                            let file = match cache
                                .ensure(
                                    &self.yt,
                                    super::cache::OfflineTrack {
                                        provider: "YouTube".into(),
                                        id: track.provider_track_id.clone(),
                                        title: track.title.clone(),
                                        ..Default::default()
                                    },
                                    settings.quality,
                                )
                                .await
                            {
                                Ok(f) => f,
                                Err(e) => {
                                    stage_log!("[debug] mobile load: cache ensure failed: {:?}", e);
                                    use super::cache::CacheError;
                                    return Err(match e {
                                        CacheError::VerificationRequired => {
                                            PlaybackErrorKind::HostRejected(
                                                HostRejectKind::VerificationRequired,
                                            )
                                        }
                                        CacheError::MediaRefused => {
                                            PlaybackErrorKind::HostRejected(
                                                HostRejectKind::MediaRefused,
                                            )
                                        }
                                        _ => extract::failed(),
                                    });
                                }
                            };
                            let url = reqwest::Url::from_file_path(&file.path)
                                .map_err(|_| extract::failed())?
                                .to_string();
                            *self.file.lock().await = Some(file);
                            url
                        } else {
                            stage_log!(
                                "[debug] mobile load: resolving {}",
                                track.provider_track_id
                            );
                            let audio = match extract::resolve_with_quality(
                                &self.yt,
                                &track.provider_track_id,
                                settings.quality,
                            )
                            .await
                            {
                                Ok(a) => a,
                                Err(e) => {
                                    stage_log!("[debug] mobile load: resolve failed: {:?}", e);
                                    return Err(e);
                                }
                            };
                            stage_log!(
                                "[debug] mobile load: resolved source={}",
                                extract::media_source_label(&audio.url)
                            );
                            let next = match AudioProxy::start(audio).await {
                                Ok(p) => p,
                                Err(e) => {
                                    stage_log!("[debug] mobile load: proxy failed: {:?}", e);
                                    return Err(e);
                                }
                            };
                            let url = next.url.clone();
                            *proxy = Some(next);
                            url
                        };
                        stage_log!(
                            "[debug] mobile load: plugin load source={}",
                            extract::media_source_label(&url)
                        );
                        match self.plugin("load", json!({ "url": url })).await {
                            Ok(v) => {
                                stage_log!("[debug] mobile load: plugin load ok");
                                Ok(v)
                            }
                            Err(e) => {
                                stage_log!("[debug] mobile load: plugin load failed: {:?}", e);
                                Err(e)
                            }
                        }
                    }
                    PlaybackCommand::Play | PlaybackCommand::Resume => {
                        self.plugin("play", json!({})).await
                    }
                    PlaybackCommand::Pause => self.plugin("pause", json!({})).await,
                    PlaybackCommand::Stop => self.plugin("stop", json!({})).await,
                    PlaybackCommand::Seek { position_ms } => {
                        self.plugin("seek", json!({ "positionMs": position_ms }))
                            .await
                    }
                    PlaybackCommand::SetVolume(v) => {
                        self.plugin("setVolume", json!({ "volume": v.value() }))
                            .await
                    }
                    _ => unreachable!("engine translates navigation to Load"),
                }
            }
            .await;
            if result.is_err() || is_stop {
                proxy.take();
                self.file.lock().await.take();
            }
            match result {
                Ok(_) => HostOutcome::Confirmed {
                    request_id: req.request_id,
                    revision: req.revision,
                },
                Err(kind) => HostOutcome::Rejected {
                    request_id: req.request_id,
                    revision: req.revision,
                    kind,
                },
            }
        })
    }
}

/// Plays a track through the YouTube page the plugin keeps in a hidden web view, for when the direct stream is
/// refused. The page is the provider's own player; only its reports settle state, and `poll` carries them like any
/// other player's.
pub struct PageHost {
    app: AppHandle<Wry>,
    revision: Arc<AtomicU64>,
}

/// The page may need a cold start, a consent screen and an ad before it reports.
const PAGE_LOAD_TIMEOUT: Duration = Duration::from_secs(35);

impl PlayerHost for PageHost {
    fn provider(&self) -> ProviderId {
        ProviderId::YouTube
    }

    fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
        Box::pin(async move {
            self.revision.store(req.revision, Ordering::SeqCst);
            let result = match &req.command {
                PlaybackCommand::Load(track) => {
                    if !extract::valid_id(&track.provider_track_id) {
                        Err(extract::failed())
                    } else {
                        stage_log!("[debug] mobile load: page fallback");
                        call_within(
                            &self.app,
                            "pageLoad",
                            json!({ "videoId": track.provider_track_id }),
                            PAGE_LOAD_TIMEOUT,
                        )
                        .await
                    }
                }
                PlaybackCommand::Play | PlaybackCommand::Resume => {
                    call(&self.app, "play", json!({})).await
                }
                PlaybackCommand::Pause => call(&self.app, "pause", json!({})).await,
                PlaybackCommand::Stop => call(&self.app, "stop", json!({})).await,
                PlaybackCommand::Seek { position_ms } => {
                    call(&self.app, "seek", json!({ "positionMs": position_ms })).await
                }
                PlaybackCommand::SetVolume(v) => {
                    call(&self.app, "setVolume", json!({ "volume": v.value() })).await
                }
                _ => Err(extract::failed()),
            };
            match result {
                Ok(_) => HostOutcome::Confirmed {
                    request_id: req.request_id,
                    revision: req.revision,
                },
                Err(kind) => HostOutcome::Rejected {
                    request_id: req.request_id,
                    revision: req.revision,
                    kind,
                },
            }
        })
    }
}

/// Turns plugin facts into engine events. The platform latches `ended`/`failed`, so a transition is seen exactly once
/// here. System-UI presses (lock screen, headphones, notification) go through the same core path as the app buttons.
async fn watch(
    app: AppHandle<Wry>,
    reports: UnboundedSender<NativeReport>,
    revision: Arc<AtomicU64>,
) {
    let mut last_status = String::from("idle");
    let (mut last_position, mut last_duration) = (0u64, 0u64);
    loop {
        let wait = if last_status == "idle" { 1000 } else { 250 };
        tokio::time::sleep(Duration::from_millis(wait)).await;
        let Ok(raw) = call(&app, "poll", json!({})).await else {
            continue;
        };
        let Ok(poll) = serde_json::from_value::<Poll>(raw) else {
            continue;
        };
        let position = poll.position_ms.max(0.0) as u64;
        let duration = poll.duration_ms.max(0.0) as u64;
        let event = if poll.status == last_status {
            None
        } else {
            match poll.status.as_str() {
                "buffering" => Some(PlayerEvent::Buffering),
                "playing" => Some(PlayerEvent::Playing),
                "failed" => Some(PlayerEvent::Failed),
                "ended" => Some(end_event(last_position, last_duration)),
                _ => None,
            }
        };
        if !matches!(event, Some(PlayerEvent::Ended | PlayerEvent::Failed)) {
            last_position = position;
            last_duration = duration;
        }
        last_status = poll.status;
        for remote in poll.remote {
            tauri::async_runtime::spawn(crate::platform::control::run(
                app.clone(),
                match remote.kind.as_str() {
                    "play" => crate::platform::control::Remote::Play,
                    "pause" => crate::platform::control::Remote::Pause,
                    "toggle" => crate::platform::control::Remote::Toggle,
                    "next" => crate::platform::control::Remote::Next,
                    "previous" => crate::platform::control::Remote::Previous,
                    "seek" => crate::platform::control::Remote::SeekTo(remote.position_ms as i64),
                    _ => continue,
                },
            ));
        }
        if reports
            .send(NativeReport {
                revision: revision.load(Ordering::SeqCst),
                event,
                position_ms: last_position,
                duration_ms: last_duration,
            })
            .is_err()
        {
            break;
        }
    }
}
