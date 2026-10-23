//! One libmpv owner thread. Only player confirmations settle engine commands.
use super::{
    audio::{AudioControl, AudioSettings},
    command::PlaybackCommand,
    extract,
    host::{HostFuture, HostOutcome, HostRequest, PlayerEvent, PlayerHost},
    proxy::AudioProxy,
};
use crate::core::{
    domain::ProviderId,
    errors::{HostRejectKind, PlaybackErrorKind},
    yt::Yt,
};
use libmpv2::{Mpv, events::Event, mpv_end_file_reason};
use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};
use tokio::sync::{mpsc::UnboundedSender, oneshot};

pub struct NativeReport {
    pub revision: u64,
    pub event: Option<PlayerEvent>,
    pub position_ms: u64,
    pub duration_ms: u64,
}
struct Request {
    revision: u64,
    command: PlaybackCommand,
    url: Option<String>,
    audio: AudioSettings,
    reply: oneshot::Sender<Result<(), PlaybackErrorKind>>,
}
pub struct NativeHost {
    pub audio: AudioControl,
    yt: Yt,
    cache: Option<std::sync::Arc<super::cache::DiskCache>>,
    file: tokio::sync::Mutex<Option<std::sync::Arc<super::cache::CachedFile>>>,
    sender: mpsc::SyncSender<Request>,
    proxy: tokio::sync::Mutex<Option<AudioProxy>>,
}
impl NativeHost {
    pub fn with_cache(mut self, cache: std::sync::Arc<super::cache::DiskCache>) -> Self {
        self.cache = Some(cache);
        self
    }
    pub fn new(reports: UnboundedSender<NativeReport>) -> Result<Self, PlaybackErrorKind> {
        let yt = extract::resolver()?;
        Self::with_yt(yt, reports)
    }

    pub fn with_yt(
        yt: Yt,
        reports: UnboundedSender<NativeReport>,
    ) -> Result<Self, PlaybackErrorKind> {
        let (sender, receiver) = mpsc::sync_channel(1);
        let (started, startup) = mpsc::sync_channel(1);
        let audio = AudioControl::default();
        let worker_audio = audio.clone();
        thread::Builder::new()
            .name("lunor-audio".into())
            .spawn(move || {
                super::locale::numeric_c();
                let mpv = Mpv::with_initializer(|init| {
                    init.set_option("config", "no")?;
                    #[cfg(target_os = "macos")]
                    init.set_option("ao", "coreaudio")?;
                    init.set_option("ytdl", "no")?;
                    init.set_option("vid", "no")?;
                    init.set_option("terminal", "no")?;
                    init.set_option("network-timeout", 30i64)?;
                    init.set_option("cache-on-disk", "no")?;
                    init.set_option("cache", "yes")?;
                    init.set_option("cache-secs", 30i64)?;
                    init.set_option("demuxer-max-bytes", 8 * 1024 * 1024i64)?;
                    init.set_option("demuxer-max-back-bytes", 2 * 1024 * 1024i64)?;
                    init.set_option("cache-pause", "yes")?;
                    init.set_option("cache-pause-initial", "yes")?;
                    init.set_option("cache-pause-wait", 1f64)?;
                    init.set_option("volume", 100f64)
                });
                match mpv {
                    Ok(mpv) => {
                        let _ = started.send(Ok(()));
                        run(mpv, receiver, reports, worker_audio);
                    }
                    Err(error) => {
                        eprintln!("libmpv failed to start: {error}");
                        let _ = started.send(Err(extract::failed()));
                    }
                }
            })
            .map_err(|_| extract::failed())?;
        startup
            .recv_timeout(Duration::from_secs(8))
            .map_err(|_| extract::failed())??;
        Ok(Self {
            audio,
            yt,
            cache: None,
            file: tokio::sync::Mutex::new(None),
            sender,
            proxy: tokio::sync::Mutex::new(None),
        })
    }
}
impl PlayerHost for NativeHost {
    fn provider(&self) -> ProviderId {
        ProviderId::YouTube
    }
    fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
        Box::pin(async move {
            let mut proxy = self.proxy.lock().await;
            let settings = self.audio.settings();
            let is_stop = matches!(req.command, PlaybackCommand::Stop);
            let result = async {
                let url = if let PlaybackCommand::Load(track) = &req.command {
                    let (reply, stopped) = oneshot::channel();
                    self.sender
                        .try_send(Request {
                            revision: req.revision,
                            command: PlaybackCommand::Stop,
                            url: None,
                            audio: settings,
                            reply,
                        })
                        .map_err(|_| extract::failed())?;
                    stopped.await.map_err(|_| extract::failed())??;
                    proxy.take();
                    self.file.lock().await.take();
                    if let Some(cache) = &self.cache {
                        let file = cache
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
                            .map_err(|e| match e {
                                super::cache::CacheError::VerificationRequired => {
                                    PlaybackErrorKind::HostRejected(
                                        HostRejectKind::VerificationRequired,
                                    )
                                }
                                _ => extract::failed(),
                            })?;
                        let url = reqwest::Url::from_file_path(&file.path)
                            .map_err(|_| extract::failed())?
                            .to_string();
                        *self.file.lock().await = Some(file);
                        Some(url)
                    } else {
                        let audio = extract::resolve_with_quality(
                            &self.yt,
                            &track.provider_track_id,
                            settings.quality,
                        )
                        .await?;
                        let next = AudioProxy::start(audio).await?;
                        let url = next.url.clone();
                        *proxy = Some(next);
                        Some(url)
                    }
                } else {
                    None
                };
                let (reply, outcome) = oneshot::channel();
                self.sender
                    .try_send(Request {
                        revision: req.revision,
                        command: req.command,
                        url,
                        audio: settings,
                        reply,
                    })
                    .map_err(|_| extract::failed())?;
                outcome.await.map_err(|_| extract::failed())?
            }
            .await;
            if result.is_err() || is_stop {
                proxy.take();
                self.file.lock().await.take();
            }
            match result {
                Ok(()) => HostOutcome::Confirmed {
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
fn apply(mpv: &Mpv, request: &Request) -> libmpv2::Result<()> {
    match &request.command {
        PlaybackCommand::Load(_) => {
            request.audio.apply(mpv)?;
            mpv.set_property("pause", true)?;
            // URL was parsed and restricted before reaching this command-string API.
            let quoted = format!("\"{}\"", request.url.as_deref().unwrap_or(""));
            mpv.command("loadfile", &[&quoted, "replace"])
        }
        PlaybackCommand::Play | PlaybackCommand::Resume => mpv.set_property("pause", false),
        PlaybackCommand::Pause => mpv.set_property("pause", true),
        PlaybackCommand::Stop => mpv.command("stop", &[]),
        PlaybackCommand::Seek { position_ms } => mpv.command(
            "seek",
            &[
                &(*position_ms as f64 / 1000.0).to_string(),
                "absolute+exact",
            ],
        ),
        PlaybackCommand::SetVolume(v) => mpv.set_property("volume", f64::from(v.value())),
        _ => unreachable!("engine translates navigation to Load"),
    }
}
fn milliseconds(mpv: &Mpv, property: &str) -> u64 {
    (mpv.get_property::<f64>(property).unwrap_or(0.0).max(0.0) * 1000.0) as u64
}
fn run(
    mut mpv: Mpv,
    commands: mpsc::Receiver<Request>,
    reports: UnboundedSender<NativeReport>,
    audio: AudioControl,
) {
    let mut revision = 0;
    let mut applied_audio = None;
    let mut pending: Option<(Request, Instant)> = None;
    let mut last_report = Instant::now();
    let mut buffering = false;
    let (mut last_position, mut last_duration) = (0, 0);
    loop {
        // Settings change from another thread; apply at once and again after any reset.
        let shaping = audio.settings();
        let key = (shaping.loudness, shaping.effects);
        if applied_audio != Some(key) && shaping.apply_effects(&mpv).is_ok() {
            applied_audio = Some(key);
        }
        match commands.try_recv() {
            Ok(request) => {
                if request.reply.is_closed() {
                    continue;
                }
                if let Some((old, _)) = pending.take() {
                    let _ = old.reply.send(Err(extract::failed()));
                }
                while mpv.event_context_mut().wait_event(0.0).is_some() {}
                revision = request.revision;
                match apply(&mpv, &request) {
                    Err(_) => {
                        let _ = request.reply.send(Err(extract::failed()));
                    }
                    Ok(())
                        if matches!(
                            request.command,
                            PlaybackCommand::Load(_)
                                | PlaybackCommand::Play
                                | PlaybackCommand::Resume
                        ) =>
                    {
                        pending = Some((request, Instant::now()));
                    }
                    Ok(()) => {
                        let _ = request.reply.send(Ok(()));
                    }
                }
            }
            Err(mpsc::TryRecvError::Disconnected) => break,
            Err(mpsc::TryRecvError::Empty) => {}
        }
        let mut notice = None;
        match mpv.event_context_mut().wait_event(0.05) {
            Some(Ok(Event::FileLoaded))
                if pending
                    .as_ref()
                    .is_some_and(|(r, _)| matches!(r.command, PlaybackCommand::Load(_))) =>
            {
                if let Some((request, _)) = pending.take() {
                    let _ = request.reply.send(Ok(()));
                }
            }
            Some(Ok(Event::PlaybackRestart)) => {
                if pending.as_ref().is_some_and(|(r, _)| {
                    matches!(r.command, PlaybackCommand::Play | PlaybackCommand::Resume)
                }) && let Some((request, _)) = pending.take()
                {
                    let _ = request.reply.send(Ok(()));
                }
            }
            Some(Ok(Event::EndFile(reason))) if reason == mpv_end_file_reason::Eof => {
                notice = Some(end_event(last_position, last_duration))
            }
            Some(Err(_)) | Some(Ok(Event::EndFile(mpv_end_file_reason::Error))) => {
                if let Some((request, _)) = pending.take() {
                    let _ = request.reply.send(Err(extract::failed()));
                }
                notice = Some(PlayerEvent::Failed);
            }
            Some(Ok(Event::Shutdown)) => break,
            _ => {}
        }
        // Unpausing an already decoded file does not always emit PlaybackRestart.
        if pending.as_ref().is_some_and(|(r, _)| {
            matches!(r.command, PlaybackCommand::Play | PlaybackCommand::Resume)
        }) && !mpv.get_property::<bool>("pause").unwrap_or(true)
            && !mpv.get_property::<bool>("core-idle").unwrap_or(true)
            && !mpv.get_property::<bool>("idle-active").unwrap_or(true)
            && let Some((request, _)) = pending.take()
        {
            let _ = request.reply.send(Ok(()));
        }
        if pending.as_ref().is_some_and(|(r, start)| {
            r.reply.is_closed() || start.elapsed() > Duration::from_secs(12)
        }) {
            let _ = mpv.command("stop", &[]);
            if let Some((request, _)) = pending.take() {
                let _ = request.reply.send(Err(PlaybackErrorKind::HostRejected(
                    HostRejectKind::Timeout,
                )));
            }
        }
        if last_report.elapsed() >= Duration::from_millis(250) || notice.is_some() {
            let now_buffering = mpv
                .get_property::<bool>("paused-for-cache")
                .unwrap_or(false);
            if now_buffering != buffering && notice.is_none() {
                notice = if now_buffering {
                    Some(PlayerEvent::Buffering)
                } else if !mpv.get_property::<bool>("pause").unwrap_or(true) {
                    Some(PlayerEvent::Playing)
                } else {
                    None
                };
            }
            buffering = now_buffering;
            if !matches!(notice, Some(PlayerEvent::Ended | PlayerEvent::Failed)) {
                last_position = milliseconds(&mpv, "time-pos");
                last_duration = milliseconds(&mpv, "duration");
            }
            audio.observe(&mpv);
            if reports
                .send(NativeReport {
                    revision,
                    event: notice,
                    position_ms: last_position,
                    duration_ms: last_duration,
                })
                .is_err()
            {
                break;
            }
            last_report = Instant::now();
        }
    }
}

fn end_event(position_ms: u64, duration_ms: u64) -> PlayerEvent {
    if duration_ms > 0 && duration_ms.saturating_sub(position_ms) > 2000 {
        PlayerEvent::Failed
    } else {
        PlayerEvent::Ended
    }
}

#[cfg(test)]
mod tests {
    use super::super::command::Volume;
    use super::*;
    use crate::core::domain::TrackReference;

    #[test]
    fn truncated_audio_is_failure_not_normal_end() {
        assert!(matches!(end_event(24_000, 166_000), PlayerEvent::Failed));
        assert!(matches!(end_event(165_000, 166_000), PlayerEvent::Ended));
        assert!(matches!(end_event(166_000, 166_000), PlayerEvent::Ended));
    }
    #[test]
    fn native_confirms_load_play_pause_resume_seek_volume_and_stop() {
        for bits in [16u16, 24, 32] {
            native_controls(bits);
        }
    }
    fn native_controls(bits: u16) {
        let path = std::env::temp_dir().join(format!(
            "lunor-audio-test-{}-{bits}.wav",
            std::process::id()
        ));
        let samples = vec![0u8; 8_000 * usize::from(bits / 8) * 10];
        let mut wav = Vec::new();
        wav.extend(b"RIFF");
        wav.extend((36 + samples.len() as u32).to_le_bytes());
        wav.extend(b"WAVEfmt ");
        wav.extend(16u32.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(1u16.to_le_bytes());
        wav.extend(8_000u32.to_le_bytes());
        wav.extend((8_000u32 * u32::from(bits / 8)).to_le_bytes());
        wav.extend((bits / 8).to_le_bytes());
        wav.extend(bits.to_le_bytes());
        wav.extend(b"data");
        wav.extend((samples.len() as u32).to_le_bytes());
        wav.extend(samples);
        std::fs::write(&path, wav).unwrap();
        let mpv = Mpv::with_initializer(|init| {
            init.set_option("config", "no")?;
            init.set_option("ao", "null")?;
            init.set_option("vid", "no")?;
            init.set_option("terminal", "no")
        })
        .unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        let (reports, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let worker = thread::spawn(move || run(mpv, receiver, reports, AudioControl::default()));
        let track = TrackReference::new(
            ProviderId::YouTube,
            "a_B-9zZ0-_x".into(),
            "https://www.youtube.com/watch?v=a_B-9zZ0-_x".into(),
            "Test".into(),
        )
        .unwrap();
        let commands = [
            PlaybackCommand::Load(track),
            PlaybackCommand::Play,
            PlaybackCommand::Pause,
            PlaybackCommand::Resume,
            PlaybackCommand::Seek { position_ms: 1000 },
            PlaybackCommand::SetVolume(Volume::new(25).unwrap()),
            PlaybackCommand::Stop,
        ];
        for (i, command) in commands.into_iter().enumerate() {
            let is_load = matches!(command, PlaybackCommand::Load(_));
            let (reply, mut outcome) = oneshot::channel();
            sender
                .send(Request {
                    revision: i as u64 + 1,
                    command,
                    audio: AudioSettings {
                        pcm_format: super::super::audio::PcmFormat::Float32,
                        sample_rate: super::super::audio::SampleRate::Hz48000,
                        ..Default::default()
                    },
                    url: is_load.then(|| path.to_string_lossy().into_owned()),
                    reply,
                })
                .unwrap();
            let deadline = Instant::now() + Duration::from_secs(15);
            loop {
                match outcome.try_recv() {
                    Ok(result) => {
                        assert_eq!(result, Ok(()), "command {i}");
                        break;
                    }
                    Err(oneshot::error::TryRecvError::Empty) if Instant::now() < deadline => {
                        thread::sleep(Duration::from_millis(10))
                    }
                    other => panic!("native command {i} failed: {other:?}"),
                }
            }
        }
        drop(sender);
        worker.join().unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
