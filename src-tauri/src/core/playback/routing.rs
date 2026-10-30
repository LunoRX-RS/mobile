//! Chooses between two hosts for one provider: the direct player first, and the provider's own page player only when
//! the direct one refuses a load. Every later command goes to whichever host holds the track, and each new load
//! starts from the direct player again. No platform code here, so it is built and tested on every target.
use super::{
    command::PlaybackCommand,
    diag::stage_log,
    host::{HostFuture, HostOutcome, HostRequest, PlayerHost},
};
use crate::core::{
    domain::ProviderId,
    errors::{HostRejectKind, PlaybackErrorKind},
};
use std::sync::Mutex;

pub struct RoutingHost<D, P> {
    direct: D,
    page: P,
    on_page: Mutex<bool>,
}

impl<D: PlayerHost, P: PlayerHost> RoutingHost<D, P> {
    pub fn new(direct: D, page: P) -> Self {
        Self {
            direct,
            page,
            on_page: Mutex::new(false),
        }
    }

    fn holder(&self) -> bool {
        *self.on_page.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_holder(&self, on_page: bool) {
        *self.on_page.lock().unwrap_or_else(|e| e.into_inner()) = on_page;
    }
}

/// A sign-in challenge or a timeout says nothing about the stream, so the page does not retry those.
fn page_can_retry(kind: &PlaybackErrorKind) -> bool {
    matches!(
        kind,
        PlaybackErrorKind::HostRejected(HostRejectKind::Failed | HostRejectKind::MediaRefused)
    )
}

impl<D: PlayerHost, P: PlayerHost> PlayerHost for RoutingHost<D, P> {
    fn provider(&self) -> ProviderId {
        self.direct.provider()
    }

    fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
        Box::pin(async move {
            if !matches!(req.command, PlaybackCommand::Load(_)) {
                return if self.holder() {
                    let stopping = matches!(req.command, PlaybackCommand::Stop);
                    let outcome = self.page.prepare(req).await;
                    if stopping {
                        self.set_holder(false);
                    }
                    outcome
                } else {
                    self.direct.prepare(req).await
                };
            }
            // A new load always starts from the direct player; the page is closed first.
            if self.holder() {
                self.set_holder(false);
                let _ = self
                    .page
                    .prepare(HostRequest {
                        request_id: req.request_id,
                        revision: req.revision,
                        command: PlaybackCommand::Stop,
                    })
                    .await;
            }
            let retry = HostRequest {
                request_id: req.request_id,
                revision: req.revision,
                command: req.command.clone(),
            };
            match self.direct.prepare(req).await {
                HostOutcome::Rejected {
                    request_id,
                    revision,
                    kind,
                } if page_can_retry(&kind) => {
                    stage_log!("[debug] routing: direct load rejected ({kind:?}), trying the page");
                    let outcome = self.page.prepare(retry).await;
                    stage_log!(
                        "[debug] routing: page {}",
                        if matches!(outcome, HostOutcome::Confirmed { .. }) {
                            "confirmed"
                        } else {
                            "rejected too"
                        }
                    );
                    self.set_holder(matches!(outcome, HostOutcome::Confirmed { .. }));
                    match outcome {
                        confirmed @ HostOutcome::Confirmed { .. } => confirmed,
                        // The page failing too says less than why the direct stream was refused.
                        HostOutcome::Rejected { .. } => HostOutcome::Rejected {
                            request_id,
                            revision,
                            kind,
                        },
                    }
                }
                other => other,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{domain::TrackReference, playback::command::Volume};
    use std::sync::{Arc, Mutex};

    type Log = Arc<Mutex<Vec<String>>>;

    struct Fake {
        name: &'static str,
        log: Log,
        load: Result<(), PlaybackErrorKind>,
    }

    impl PlayerHost for Fake {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }
        fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
            Box::pin(async move {
                let what = match &req.command {
                    PlaybackCommand::Load(_) => "load",
                    PlaybackCommand::Play => "play",
                    PlaybackCommand::Stop => "stop",
                    _ => "other",
                };
                self.log
                    .lock()
                    .unwrap()
                    .push(format!("{}:{what}", self.name));
                let result = if what == "load" { self.load } else { Ok(()) };
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

    fn host(
        direct: Result<(), PlaybackErrorKind>,
        page: Result<(), PlaybackErrorKind>,
    ) -> (RoutingHost<Fake, Fake>, Log) {
        let log = Log::default();
        let fake = |name, load| Fake {
            name,
            log: log.clone(),
            load,
        };
        (
            RoutingHost::new(fake("direct", direct), fake("page", page)),
            log,
        )
    }

    fn req(command: PlaybackCommand) -> HostRequest {
        HostRequest {
            request_id: 1,
            revision: 1,
            command,
        }
    }

    fn load() -> PlaybackCommand {
        PlaybackCommand::Load(
            TrackReference::new(
                ProviderId::YouTube,
                "dQw4w9WgXcQ".into(),
                "https://youtu.be/dQw4w9WgXcQ".into(),
                "t".into(),
            )
            .unwrap(),
        )
    }

    fn confirmed(outcome: &HostOutcome) -> bool {
        matches!(outcome, HostOutcome::Confirmed { .. })
    }

    fn refused() -> PlaybackErrorKind {
        PlaybackErrorKind::HostRejected(HostRejectKind::MediaRefused)
    }

    fn entries(log: &Log) -> Vec<String> {
        log.lock().unwrap().clone()
    }

    #[tokio::test]
    async fn the_direct_player_keeps_a_load_it_confirms() {
        let (routing, log) = host(Ok(()), Ok(()));
        assert!(confirmed(&routing.prepare(req(load())).await));
        assert!(confirmed(
            &routing.prepare(req(PlaybackCommand::Play)).await
        ));
        assert_eq!(entries(&log), ["direct:load", "direct:play"]);
    }

    #[tokio::test]
    async fn a_refused_load_moves_to_the_page_and_later_commands_follow() {
        let (routing, log) = host(Err(refused()), Ok(()));
        assert!(confirmed(&routing.prepare(req(load())).await));
        assert!(confirmed(
            &routing.prepare(req(PlaybackCommand::Play)).await
        ));
        assert!(confirmed(
            &routing
                .prepare(req(PlaybackCommand::SetVolume(Volume::new(50).unwrap())))
                .await
        ));
        assert_eq!(
            entries(&log),
            ["direct:load", "page:load", "page:play", "page:other"]
        );
    }

    #[tokio::test]
    async fn a_new_load_closes_the_page_and_starts_from_the_direct_player() {
        let (routing, log) = host(Err(refused()), Ok(()));
        routing.prepare(req(load())).await;
        routing.prepare(req(load())).await;
        assert_eq!(
            entries(&log),
            [
                "direct:load",
                "page:load",
                "page:stop",
                "direct:load",
                "page:load"
            ]
        );
    }

    #[tokio::test]
    async fn a_stop_on_the_page_hands_the_next_command_back_to_the_direct_player() {
        let (routing, log) = host(Err(refused()), Ok(()));
        routing.prepare(req(load())).await;
        routing.prepare(req(PlaybackCommand::Stop)).await;
        routing.prepare(req(PlaybackCommand::Play)).await;
        assert_eq!(
            entries(&log),
            ["direct:load", "page:load", "page:stop", "direct:play"]
        );
    }

    #[tokio::test]
    async fn the_page_failing_too_reports_the_direct_refusal() {
        let failed = PlaybackErrorKind::HostRejected(HostRejectKind::Failed);
        let (routing, log) = host(Err(refused()), Err(failed));
        match routing.prepare(req(load())).await {
            HostOutcome::Rejected { kind, .. } => assert_eq!(kind, refused()),
            HostOutcome::Confirmed { .. } => panic!("expected a rejection"),
        }
        routing.prepare(req(PlaybackCommand::Play)).await;
        assert_eq!(entries(&log), ["direct:load", "page:load", "direct:play"]);
    }

    #[tokio::test]
    async fn a_challenge_or_timeout_is_not_retried_on_the_page() {
        for kind in [
            PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired),
            PlaybackErrorKind::HostRejected(HostRejectKind::Timeout),
        ] {
            let (routing, log) = host(Err(kind), Ok(()));
            match routing.prepare(req(load())).await {
                HostOutcome::Rejected { kind: got, .. } => assert_eq!(got, kind),
                HostOutcome::Confirmed { .. } => panic!("expected a rejection"),
            }
            assert_eq!(entries(&log), ["direct:load"]);
        }
    }
}
