use std::collections::HashMap;
use std::time::Duration;

use tokio::time::timeout;

use crate::core::capabilities::Capability;
use crate::core::domain::{ProviderId, TrackReference};
use crate::core::errors::{CoreError, HostRejectKind, PlaybackErrorKind};
use crate::core::playback::command::PlaybackCommand;
use crate::core::playback::host::{HostOutcome, HostRequest, PlayerEvent, PlayerHost};
use crate::core::playback::state::{PlaybackState, validate_transition};
use crate::core::queue::QueueEngine;
use crate::core::registry::ProviderRegistry;
use PlaybackState::*;

pub struct PlaybackEngine {
    state: PlaybackState,
    revision: u64,
    next_request_id: u64,
    queue: QueueEngine,
    registry: ProviderRegistry,
    hosts: HashMap<ProviderId, Box<dyn PlayerHost>>,
    active: Option<TrackReference>,
    timeout: Duration,
    stale_count: u64,
}

impl PlaybackEngine {
    pub fn new(
        registry: ProviderRegistry,
        hosts: HashMap<ProviderId, Box<dyn PlayerHost>>,
        timeout: Duration,
    ) -> Self {
        Self {
            state: Idle,
            revision: 0,
            next_request_id: 1,
            queue: QueueEngine::new(),
            registry,
            hosts,
            active: None,
            timeout,
            stale_count: 0,
        }
    }

    // No host and no verified capability: every provider command returns UnsupportedCapability.
    pub fn production() -> Self {
        Self::new(
            ProviderRegistry::new(),
            HashMap::new(),
            Duration::from_secs(8),
        )
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn active_track(&self) -> Option<&TrackReference> {
        self.active.as_ref()
    }

    // Every state write goes through here, so the table is checked before the field changes.
    fn set(&mut self, to: PlaybackState) -> Result<(), CoreError> {
        validate_transition(self.state, to).map_err(CoreError::Playback)?;
        self.state = to;
        Ok(())
    }

    pub async fn dispatch(&mut self, cmd: PlaybackCommand) -> Result<PlaybackState, CoreError> {
        // Target is the state the command must be valid for. Load's final state is Ready (see apply_outcome).
        // Seek and SetVolume keep the state, so they skip the table.
        let keeps_state = matches!(
            cmd,
            PlaybackCommand::Seek { .. } | PlaybackCommand::SetVolume(_)
        );
        let target = match &cmd {
            PlaybackCommand::GetState => return Ok(self.state),
            PlaybackCommand::Load(_) | PlaybackCommand::Next | PlaybackCommand::Previous => {
                Resolving
            }
            PlaybackCommand::Play | PlaybackCommand::Resume => Playing,
            PlaybackCommand::Pause => Paused,
            PlaybackCommand::Stop => Idle,
            PlaybackCommand::Seek { .. } | PlaybackCommand::SetVolume(_) => self.state,
        };
        if !keeps_state {
            validate_transition(self.state, target).map_err(CoreError::Playback)?;
        }

        // Stop with nothing loaded has no host to tell, so the reset to Idle is local.
        if matches!(cmd, PlaybackCommand::Stop) && self.active.is_none() {
            self.set(Idle)?;
            return Ok(Idle);
        }

        // Next and Previous peek the neighbour; the queue moves only after the host confirms.
        let neighbour = match &cmd {
            PlaybackCommand::Previous if self.active.is_none() => {
                return Err(CoreError::Playback(PlaybackErrorKind::NoActiveProvider));
            }
            PlaybackCommand::Next => self.queue.peek_next(),
            PlaybackCommand::Previous => self.queue.peek_previous(),
            _ => None,
        }
        .map(|e| e.track.clone());
        let (provider, host_cmd) = match &cmd {
            PlaybackCommand::Load(t) => (t.provider.clone(), cmd.clone()),
            // The host needs the track to play, so Next and Previous send it as a Load.
            PlaybackCommand::Next | PlaybackCommand::Previous => {
                let t = neighbour.ok_or(CoreError::Playback(PlaybackErrorKind::QueueBoundary))?;
                (t.provider.clone(), PlaybackCommand::Load(t))
            }
            _ => match &self.active {
                Some(t) => (t.provider.clone(), cmd.clone()),
                None => return Err(CoreError::Playback(PlaybackErrorKind::NoActiveProvider)),
            },
        };
        // Seek and SetVolume need a loaded track. Any other state refuses them before the host is called.
        if keeps_state && !matches!(self.state, Ready | Playing | Paused | Buffering) {
            return Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: self.state,
                to: self.state,
            }));
        }
        // Spec 3: one provider per session. A Load or neighbour from another provider is refused
        // before any host call; with nothing loaded, the first track sets the provider.
        if matches!(
            cmd,
            PlaybackCommand::Load(_) | PlaybackCommand::Next | PlaybackCommand::Previous
        ) && self.active.as_ref().is_some_and(|a| a.provider != provider)
        {
            return Err(CoreError::Playback(
                PlaybackErrorKind::CrossProviderUnsupported,
            ));
        }
        // A refused Next or Previous is a cross-provider refusal; any other command is UnsupportedCapability.
        let refused = CoreError::Playback(
            if matches!(cmd, PlaybackCommand::Next | PlaybackCommand::Previous) {
                PlaybackErrorKind::CrossProviderUnsupported
            } else {
                PlaybackErrorKind::UnsupportedCapability
            },
        );
        if !self
            .registry
            .capabilities_for(provider.clone())
            .is_supported(Capability::Playback)
        {
            return Err(refused);
        }
        // A supported provider with no wired host is treated the same as no capability.
        let Some(host) = self.hosts.get(&provider) else {
            return Err(refused);
        };

        self.revision += 1;
        let (request_id, revision) = (self.next_request_id, self.revision);
        self.next_request_id += 1;
        if matches!(
            cmd,
            PlaybackCommand::Load(_) | PlaybackCommand::Next | PlaybackCommand::Previous
        ) {
            self.state = Resolving; // validated above: the table allows any state -> Resolving
        }

        let req = HostRequest {
            request_id,
            revision,
            command: host_cmd,
        };
        let outcome = match timeout(self.timeout, host.prepare(req)).await {
            Ok(outcome) => outcome,
            Err(_) => {
                self.set(Error)?;
                return Err(CoreError::Playback(PlaybackErrorKind::HostRejected(
                    HostRejectKind::Timeout,
                )));
            }
        };

        self.apply_outcome(cmd, request_id, revision, outcome)
    }

    pub fn stale_count(&self) -> u64 {
        self.stale_count
    }

    // The revision a host event must carry to be accepted. Advances on every command sent to a host.
    pub fn revision(&self) -> u64 {
        self.revision
    }

    // Player-reported buffering, end, restriction or resume. Only the current revision may change state;
    // an older one is counted and dropped (spec 9.4, 9.6). Transitions are validated like any other.
    pub fn observe(
        &mut self,
        revision: u64,
        event: PlayerEvent,
    ) -> Result<PlaybackState, CoreError> {
        if revision != self.revision {
            self.stale_count += 1;
            return Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome));
        }
        let to = match event {
            PlayerEvent::Buffering => Buffering,
            PlayerEvent::Playing => Playing,
            PlayerEvent::Ended => Ended,
            PlayerEvent::Blocked => Blocked,
            PlayerEvent::Failed => Error,
        };
        self.set(to)?;
        Ok(self.state)
    }

    // Only the current request may change state. An older revision, or an outcome whose ids
    // differ from the request, is counted and dropped (spec 5.4, 9.4).
    fn apply_outcome(
        &mut self,
        cmd: PlaybackCommand,
        request_id: u64,
        revision: u64,
        outcome: HostOutcome,
    ) -> Result<PlaybackState, CoreError> {
        let (id, rev) = match &outcome {
            HostOutcome::Confirmed {
                request_id,
                revision,
            }
            | HostOutcome::Rejected {
                request_id,
                revision,
                ..
            } => (*request_id, *revision),
        };
        // A Confirmed that names another request is a host failure, not a stale message (spec fix 3).
        let names_other_request = matches!(
            outcome,
            HostOutcome::Confirmed { request_id: i, revision: r }
                if revision == self.revision && (i, r) != (request_id, revision)
        );
        if names_other_request {
            self.set(Error)?;
            return Err(CoreError::Playback(PlaybackErrorKind::HostRejected(
                HostRejectKind::Failed,
            )));
        }
        if revision != self.revision || (id, rev) != (request_id, revision) {
            self.stale_count += 1;
            return Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome));
        }
        match outcome {
            HostOutcome::Confirmed { .. } => {
                match &cmd {
                    PlaybackCommand::Load(track) => {
                        self.active = Some(track.clone());
                        self.set(Preparing)?;
                        self.set(Ready)?;
                    }
                    // Peeked before the request and the engine holds &mut self while awaiting, so the advance lands on that entry.
                    PlaybackCommand::Next | PlaybackCommand::Previous => {
                        let entry = if matches!(cmd, PlaybackCommand::Next) {
                            self.queue.advance()
                        } else {
                            self.queue.previous()
                        };
                        self.active = Some(
                            entry
                                .ok_or(CoreError::Playback(PlaybackErrorKind::QueueBoundary))?
                                .track
                                .clone(),
                        );
                        self.set(Preparing)?;
                        self.set(Ready)?;
                    }
                    PlaybackCommand::Stop => {
                        self.active = None;
                        self.set(Idle)?;
                    }
                    PlaybackCommand::Play | PlaybackCommand::Resume => self.set(Playing)?,
                    PlaybackCommand::Pause => self.set(Paused)?,
                    // Seek and SetVolume keep the state; GetState never reaches a host.
                    PlaybackCommand::Seek { .. }
                    | PlaybackCommand::SetVolume(_)
                    | PlaybackCommand::GetState => {}
                }
                Ok(self.state)
            }
            HostOutcome::Rejected { kind, .. } => {
                self.set(Error)?;
                Err(CoreError::Playback(kind))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
    use crate::core::domain::PlaylistEntry;
    use crate::core::playback::command::Volume;
    use crate::core::playback::host::{HostFuture, HostRequest};
    use std::sync::{Arc, Mutex};

    // Every command the fake host receives, in order.
    type Log = Arc<Mutex<Vec<PlaybackCommand>>>;

    #[derive(Clone, Copy)]
    enum Answer {
        Confirm,
        Reject(PlaybackErrorKind),
        NeverOnPlay,
        WrongId,
    }

    struct FakeHost {
        answer: Answer,
        log: Log,
    }

    impl PlayerHost for FakeHost {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }

        fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
            self.log.lock().unwrap().push(req.command.clone());
            let answer = self.answer;
            let is_play = req.command == PlaybackCommand::Play;
            let (request_id, revision) = (req.request_id, req.revision);
            Box::pin(async move {
                match answer {
                    Answer::Confirm => HostOutcome::Confirmed {
                        request_id,
                        revision,
                    },
                    Answer::Reject(kind) => HostOutcome::Rejected {
                        request_id,
                        revision,
                        kind,
                    },
                    Answer::WrongId => HostOutcome::Confirmed {
                        request_id: request_id + 1,
                        revision,
                    },
                    Answer::NeverOnPlay if is_play => std::future::pending().await,
                    Answer::NeverOnPlay => HostOutcome::Confirmed {
                        request_id,
                        revision,
                    },
                }
            })
        }
    }

    fn track() -> TrackReference {
        TrackReference::new(
            ProviderId::YouTube,
            "vid1".into(),
            "https://youtu.be/vid1".into(),
            "t".into(),
        )
        .unwrap()
    }

    fn playable() -> ProviderRegistry {
        let mut r = ProviderRegistry::new();
        let caps = ProviderCapabilities {
            playback: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        r.register(ProviderId::YouTube, caps).unwrap();
        r
    }

    fn engine(
        registry: ProviderRegistry,
        answer: Answer,
        timeout: Duration,
    ) -> (PlaybackEngine, Log) {
        let log = Log::default();
        let host: Box<dyn PlayerHost> = Box::new(FakeHost {
            answer,
            log: log.clone(),
        });
        let hosts = HashMap::from([(ProviderId::YouTube, host)]);
        (PlaybackEngine::new(registry, hosts, timeout), log)
    }

    #[tokio::test]
    async fn play_from_idle_is_invalid_transition() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        assert_eq!(
            e.dispatch(PlaybackCommand::Play).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Idle,
                to: Playing
            }))
        );
        assert_eq!(e.state(), Idle);
        assert!(log.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn load_confirmed_reaches_ready() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        assert_eq!(e.dispatch(PlaybackCommand::Load(track())).await, Ok(Ready));
        assert_eq!(e.state(), Ready);
    }

    #[tokio::test]
    async fn play_from_ready_confirmed_is_playing() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        assert_eq!(e.dispatch(PlaybackCommand::Play).await, Ok(Playing));
        assert_eq!(e.state(), Playing);
    }

    #[tokio::test(start_paused = true)]
    async fn play_with_silent_host_times_out_to_error_never_playing() {
        let (mut e, _) = engine(playable(), Answer::NeverOnPlay, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Play).await,
            Err(CoreError::Playback(PlaybackErrorKind::HostRejected(
                HostRejectKind::Timeout
            )))
        );
        assert_eq!(e.state(), Error);
    }

    #[tokio::test]
    async fn rejected_host_moves_to_error_with_its_kind() {
        let kind = PlaybackErrorKind::HostRejected(HostRejectKind::Failed);
        let (mut e, _) = engine(playable(), Answer::Reject(kind), Duration::from_secs(8));
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(track())).await,
            Err(CoreError::Playback(kind))
        );
        assert_eq!(e.state(), Error);
    }

    #[tokio::test]
    async fn unsupported_capability_calls_no_host() {
        let (mut e, log) = engine(
            ProviderRegistry::new(),
            Answer::Confirm,
            Duration::from_secs(8),
        );
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(track())).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::UnsupportedCapability
            ))
        );
        assert!(log.lock().unwrap().is_empty());
        assert_eq!(e.state(), Idle);
    }

    #[tokio::test]
    async fn production_engine_rejects_every_load() {
        let mut e = PlaybackEngine::production();
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(track())).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::UnsupportedCapability
            ))
        );
        assert_eq!(e.state(), Idle);
        for id in [ProviderId::YouTube, ProviderId::Spotify] {
            assert!(
                !e.registry
                    .capabilities_for(id)
                    .is_supported(Capability::Playback)
            );
        }
    }

    #[tokio::test]
    async fn confirmed_with_wrong_request_id_is_host_failure() {
        let (mut e, _) = engine(playable(), Answer::WrongId, Duration::from_secs(8));
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(track())).await,
            Err(CoreError::Playback(PlaybackErrorKind::HostRejected(
                HostRejectKind::Failed
            )))
        );
        assert_eq!(e.state(), Error);
        assert_eq!(e.stale_count, 0);
    }

    fn track2() -> TrackReference {
        TrackReference::new(
            ProviderId::YouTube,
            "vid2".into(),
            "https://youtu.be/vid2".into(),
            "t2".into(),
        )
        .unwrap()
    }

    fn confirmed(request_id: u64, revision: u64) -> HostOutcome {
        HostOutcome::Confirmed {
            request_id,
            revision,
        }
    }

    #[tokio::test]
    async fn late_outcome_for_superseded_revision_is_discarded() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap(); // request 1, revision 1
        e.dispatch(PlaybackCommand::Play).await.unwrap(); // request 2, revision 2
        assert_eq!(
            e.apply_outcome(PlaybackCommand::Load(track()), 1, 1, confirmed(1, 1)),
            Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome))
        );
        assert_eq!(e.state(), Playing);
        assert_eq!(e.stale_count(), 1);
    }

    #[tokio::test]
    async fn stop_during_preparing_ignores_late_confirmation() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        // dispatch takes &mut self, so a Stop cannot interleave with a live request: set up the in-flight Load by hand.
        e.state = Preparing;
        e.active = Some(track());
        e.revision = 1;
        e.next_request_id = 2;
        assert_eq!(e.dispatch(PlaybackCommand::Stop).await, Ok(Idle)); // request 2, revision 2
        assert_eq!(
            e.apply_outcome(PlaybackCommand::Load(track()), 1, 1, confirmed(1, 1)),
            Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome))
        );
        assert_eq!(e.state(), Idle);
        assert_eq!(e.stale_count(), 1);
    }

    #[tokio::test]
    async fn load_while_ready_replaces_active_and_ignores_previous_outcome() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap(); // request 1, revision 1
        assert_eq!(e.dispatch(PlaybackCommand::Load(track2())).await, Ok(Ready)); // request 2, revision 2
        assert_eq!(
            e.apply_outcome(PlaybackCommand::Load(track()), 1, 1, confirmed(1, 1)),
            Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome))
        );
        assert_eq!(e.active, Some(track2()));
        assert_eq!(e.stale_count(), 1);
    }

    #[tokio::test]
    async fn load_same_provider_while_ready_replaces_active() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        assert_eq!(e.dispatch(PlaybackCommand::Load(track2())).await, Ok(Ready));
        assert_eq!(e.state(), Ready);
        assert_eq!(e.active, Some(track2()));
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn load_other_provider_while_active_is_refused_before_host_call() {
        let (mut e, log) = two_provider_engine(Answer::Confirm);
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        let spotify = entry("s", ProviderId::Spotify, "spTrack").track;
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(spotify)).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::CrossProviderUnsupported
            ))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(e.active, Some(track()));
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "host counter unchanged by the refused Load"
        );
    }

    fn entry(id: &str, provider: ProviderId, video: &str) -> PlaylistEntry {
        PlaylistEntry {
            id: id.into(),
            track: TrackReference::new(
                provider,
                video.into(),
                format!("https://example.test/{video}"),
                "t".into(),
            )
            .unwrap(),
            position: 0,
        }
    }

    #[tokio::test]
    async fn next_to_supported_entry_moves_active_only_on_confirm() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        e.queue
            .add(entry("b", ProviderId::YouTube, "vidB"))
            .unwrap();
        assert_eq!(e.dispatch(PlaybackCommand::Next).await, Ok(Ready));
        assert_eq!(
            e.active,
            Some(entry("a", ProviderId::YouTube, "vidA").track)
        );
        assert_eq!(e.queue.current().map(|x| x.id.clone()), Some("a".into()));
    }

    // Both providers are playable and have a host, so only the provider check can refuse a neighbour.
    fn two_provider_engine(answer: Answer) -> (PlaybackEngine, Log) {
        let log = Log::default();
        let host = |log: &Log| -> Box<dyn PlayerHost> {
            Box::new(FakeHost {
                answer,
                log: log.clone(),
            })
        };
        let mut registry = playable();
        let caps = ProviderCapabilities {
            playback: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        registry.register(ProviderId::Spotify, caps).unwrap();
        let hosts = HashMap::from([
            (ProviderId::YouTube, host(&log)),
            (ProviderId::Spotify, host(&log)),
        ]);
        (
            PlaybackEngine::new(registry, hosts, Duration::from_secs(8)),
            log,
        )
    }

    #[tokio::test]
    async fn next_to_other_provider_is_refused_before_host_call() {
        let (mut e, log) = two_provider_engine(Answer::Confirm);
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        e.queue
            .add(entry("s", ProviderId::Spotify, "spTrack"))
            .unwrap();
        e.dispatch(PlaybackCommand::Next).await.unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Next).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::CrossProviderUnsupported
            ))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(
            e.active,
            Some(entry("a", ProviderId::YouTube, "vidA").track)
        );
        assert_eq!(e.queue.current().map(|x| x.id.clone()), Some("a".into()));
        assert_eq!(
            log.lock().unwrap().len(),
            1,
            "only the first Next reached the host"
        );
    }

    #[tokio::test]
    async fn previous_to_other_provider_is_refused_before_host_call() {
        let (mut e, log) = two_provider_engine(Answer::Confirm);
        e.queue
            .add(entry("s", ProviderId::Spotify, "spTrack"))
            .unwrap();
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        e.queue.advance(); // index 0: the Spotify entry
        e.queue.advance(); // index 1: the YouTube entry
        e.active = Some(entry("a", ProviderId::YouTube, "vidA").track);
        e.state = Ready;
        assert_eq!(
            e.dispatch(PlaybackCommand::Previous).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::CrossProviderUnsupported
            ))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(e.queue.current().map(|x| x.id.clone()), Some("a".into()));
        assert!(log.lock().unwrap().is_empty(), "host counter stays 0");
    }

    #[tokio::test]
    async fn previous_at_index_zero_is_error_and_unchanged() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        e.queue
            .add(entry("b", ProviderId::YouTube, "vidB"))
            .unwrap();
        e.dispatch(PlaybackCommand::Next).await.unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Previous).await,
            Err(CoreError::Playback(PlaybackErrorKind::QueueBoundary))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(e.queue.current().map(|x| x.id.clone()), Some("a".into()));
        assert_eq!(log.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn next_at_last_entry_is_queue_boundary_and_unchanged() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        e.queue
            .add(entry("b", ProviderId::YouTube, "vidB"))
            .unwrap();
        e.dispatch(PlaybackCommand::Next).await.unwrap();
        e.dispatch(PlaybackCommand::Next).await.unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Next).await,
            Err(CoreError::Playback(PlaybackErrorKind::QueueBoundary))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(
            e.active,
            Some(entry("b", ProviderId::YouTube, "vidB").track)
        );
        assert_eq!(e.queue.current().map(|x| x.id.clone()), Some("b".into()));
        assert_eq!(log.lock().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn previous_without_current_is_no_active_provider() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.queue
            .add(entry("a", ProviderId::YouTube, "vidA"))
            .unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Previous).await,
            Err(CoreError::Playback(PlaybackErrorKind::NoActiveProvider))
        );
        assert_eq!(e.state(), Idle);
    }

    #[tokio::test]
    async fn seek_and_volume_reach_host_and_keep_state() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::Seek { position_ms: 1500 })
                .await,
            Ok(Ready)
        );
        let volume = Volume::new(50).unwrap();
        assert_eq!(
            e.dispatch(PlaybackCommand::SetVolume(volume)).await,
            Ok(Ready)
        );
        assert_eq!(
            log.lock().unwrap().clone(),
            vec![
                PlaybackCommand::Load(track()),
                PlaybackCommand::Seek { position_ms: 1500 },
                PlaybackCommand::SetVolume(volume),
            ]
        );
    }

    #[tokio::test]
    async fn seek_and_next_are_explicit_errors_not_success() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        assert!(e.dispatch(PlaybackCommand::Next).await.is_err());
        assert!(
            e.dispatch(PlaybackCommand::Seek { position_ms: 1 })
                .await
                .is_err()
        );
        assert_eq!(e.state(), Idle);
    }

    #[tokio::test]
    async fn rapid_repeated_commands_are_each_validated() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        e.dispatch(PlaybackCommand::Load(track2())).await.unwrap();
        assert_eq!(e.dispatch(PlaybackCommand::Play).await, Ok(Playing));
        assert_eq!(
            e.dispatch(PlaybackCommand::Play).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Playing,
                to: Playing
            }))
        );
        assert_eq!(e.dispatch(PlaybackCommand::Pause).await, Ok(Paused));
        assert_eq!(
            e.dispatch(PlaybackCommand::Pause).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Paused,
                to: Paused
            }))
        );
        assert_eq!(e.dispatch(PlaybackCommand::Resume).await, Ok(Playing));
        // Load, Load, Play, Pause, Resume reached the host. The two refused repeats did not.
        assert_eq!(log.lock().unwrap().len(), 5);
        assert_eq!(e.revision(), 5);
        assert_eq!(e.stale_count(), 0);
    }

    #[tokio::test]
    async fn buffering_pause_and_ended_follow_player_events() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        e.dispatch(PlaybackCommand::Play).await.unwrap();
        let rev = e.revision();
        assert_eq!(e.observe(rev, PlayerEvent::Buffering), Ok(Buffering));
        assert_eq!(e.dispatch(PlaybackCommand::Pause).await, Ok(Paused));
        assert_eq!(e.dispatch(PlaybackCommand::Resume).await, Ok(Playing));
        assert_eq!(
            e.observe(e.revision(), PlayerEvent::Buffering),
            Ok(Buffering)
        );
        assert_eq!(e.observe(e.revision(), PlayerEvent::Playing), Ok(Playing));
        assert_eq!(e.observe(e.revision(), PlayerEvent::Ended), Ok(Ended));
        assert_eq!(e.dispatch(PlaybackCommand::Stop).await, Ok(Idle));
    }

    #[tokio::test]
    async fn player_restriction_blocks_and_load_recovers() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        e.dispatch(PlaybackCommand::Play).await.unwrap();
        let rev = e.revision();
        assert_eq!(e.observe(rev, PlayerEvent::Blocked), Ok(Blocked));
        assert_eq!(
            e.dispatch(PlaybackCommand::Play).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Blocked,
                to: Playing
            }))
        );
        assert_eq!(e.dispatch(PlaybackCommand::Load(track2())).await, Ok(Ready));
        // Load, Play, Load: the refused Play never reached the host.
        assert_eq!(log.lock().unwrap().len(), 3);
    }

    #[tokio::test]
    async fn player_event_for_superseded_revision_is_dropped() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap(); // revision 1
        let stale = e.revision();
        e.dispatch(PlaybackCommand::Play).await.unwrap(); // revision 2
        assert_eq!(
            e.observe(stale, PlayerEvent::Buffering),
            Err(CoreError::Playback(PlaybackErrorKind::StaleOutcome))
        );
        assert_eq!(e.state(), Playing);
        assert_eq!(e.stale_count(), 1);
    }

    #[tokio::test]
    async fn player_event_from_wrong_state_is_invalid_transition() {
        let (mut e, _) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        let rev = e.revision();
        assert_eq!(
            e.observe(rev, PlayerEvent::Buffering),
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Ready,
                to: Buffering
            }))
        );
        assert_eq!(
            e.observe(rev, PlayerEvent::Ended),
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Ready,
                to: Ended
            }))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(e.stale_count(), 0);
    }

    #[tokio::test]
    async fn seek_in_error_state_is_refused_without_host_call() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        e.state = Error; // a failed Play leaves the track active in Error
        assert_eq!(
            e.dispatch(PlaybackCommand::Seek { position_ms: 1 }).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Error,
                to: Error
            }))
        );
        assert_eq!(e.state(), Error);
        assert_eq!(log.lock().unwrap().len(), 1, "only the Load");
    }

    #[tokio::test]
    async fn stop_after_failed_load_resets_to_idle() {
        let kind = PlaybackErrorKind::HostRejected(HostRejectKind::Failed);
        let (mut e, log) = engine(playable(), Answer::Reject(kind), Duration::from_secs(8));
        assert!(e.dispatch(PlaybackCommand::Load(track())).await.is_err());
        assert_eq!(e.state(), Error);
        assert_eq!(e.dispatch(PlaybackCommand::Stop).await, Ok(Idle));
        assert_eq!(
            e.dispatch(PlaybackCommand::Stop).await,
            Err(CoreError::Playback(PlaybackErrorKind::InvalidTransition {
                from: Idle,
                to: Idle
            }))
        );
        assert_eq!(log.lock().unwrap().len(), 1, "no host call for the Stops");
    }

    #[tokio::test]
    async fn provider_capability_lost_mid_session_refuses_commands() {
        let (mut e, log) = engine(playable(), Answer::Confirm, Duration::from_secs(8));
        e.dispatch(PlaybackCommand::Load(track())).await.unwrap();
        e.registry = ProviderRegistry::new();
        assert_eq!(
            e.dispatch(PlaybackCommand::Play).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::UnsupportedCapability
            ))
        );
        assert_eq!(
            e.dispatch(PlaybackCommand::Seek { position_ms: 5 }).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::UnsupportedCapability
            ))
        );
        assert_eq!(e.state(), Ready);
        assert_eq!(log.lock().unwrap().len(), 1, "only the Load");
    }

    #[tokio::test]
    async fn supported_provider_without_host_is_unsupported() {
        let mut e = PlaybackEngine::new(playable(), HashMap::new(), Duration::from_secs(8));
        assert_eq!(
            e.dispatch(PlaybackCommand::Load(track())).await,
            Err(CoreError::Playback(
                PlaybackErrorKind::UnsupportedCapability
            ))
        );
        assert_eq!(e.state(), Idle);
    }
}
