use std::future::Future;
use std::pin::Pin;

use crate::core::domain::ProviderId;
use crate::core::errors::PlaybackErrorKind;
use crate::core::playback::command::PlaybackCommand;

pub type HostFuture<'a> = Pin<Box<dyn Future<Output = HostOutcome> + Send + 'a>>;

// Fields are pub: the engine builds this in the same crate; it is not a trust boundary.
pub struct HostRequest {
    pub request_id: u64,
    pub revision: u64,
    pub command: PlaybackCommand,
}

// Player-reported facts that are not answers to a command. Each carries the revision it was observed under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerEvent {
    Buffering,
    Playing,
    Ended,
    Blocked,
    Failed,
}

pub enum HostOutcome {
    Confirmed {
        request_id: u64,
        revision: u64,
    },
    Rejected {
        request_id: u64,
        revision: u64,
        kind: PlaybackErrorKind,
    },
}

pub trait PlayerHost: Send + Sync {
    fn provider(&self) -> ProviderId;
    fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeHost;

    impl PlayerHost for FakeHost {
        fn provider(&self) -> ProviderId {
            ProviderId::YouTube
        }

        fn prepare<'a>(&'a self, req: HostRequest) -> HostFuture<'a> {
            Box::pin(async move {
                HostOutcome::Confirmed {
                    request_id: req.request_id,
                    revision: req.revision,
                }
            })
        }
    }

    #[tokio::test]
    async fn boxed_host_confirms_given_ids() {
        let host: Box<dyn PlayerHost> = Box::new(FakeHost);
        let req = HostRequest {
            request_id: 7,
            revision: 3,
            command: PlaybackCommand::Play,
        };
        match host.prepare(req).await {
            HostOutcome::Confirmed {
                request_id,
                revision,
            } => assert_eq!((request_id, revision), (7, 3)),
            HostOutcome::Rejected { .. } => panic!("expected Confirmed"),
        }
        assert_eq!(host.provider(), ProviderId::YouTube);
    }
}
