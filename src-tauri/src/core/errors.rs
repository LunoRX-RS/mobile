use crate::core::playback::state::PlaybackState;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CoreError {
    #[error("search error: {0}")]
    Search(String),
    #[error("provider error: {0}")]
    Provider(String),
    #[error("provider unavailable")]
    ProviderUnavailable,
    #[error("authentication required")]
    AuthenticationRequired,
    // The provider itself reported its quota as spent (distinct from the local QuotaExhausted skip).
    #[error("provider quota exceeded")]
    QuotaExceeded,
    #[error("unsupported capability")]
    UnsupportedCapability,
    #[error("playback error: {0}")]
    Playback(PlaybackErrorKind),
    #[error("queue error: {0}")]
    Queue(QueueErrorKind),
    #[error("playlist error: {0}")]
    Playlist(PlaylistErrorKind),
    #[error("validation error: {0}")]
    Validation(String),
    #[error("cache error: {0}")]
    Cache(String),
    #[error("network error: {0}")]
    Network(String),
}

// Display is fixed text only: no provider payloads reach the message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlaybackErrorKind {
    #[error("invalid transition from {from:?} to {to:?}")]
    InvalidTransition {
        from: PlaybackState,
        to: PlaybackState,
    },
    #[error("unsupported capability")]
    UnsupportedCapability,
    #[error("no active provider")]
    NoActiveProvider,
    #[error("cross-provider transition unsupported")]
    CrossProviderUnsupported,
    #[error("no entry in that direction")]
    QueueBoundary,
    #[error("host rejected: {0}")]
    HostRejected(HostRejectKind),
    #[error("stale outcome")]
    StaleOutcome,
    #[error("invalid input: {0}")]
    Validation(&'static str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum HostRejectKind {
    #[error("timeout")]
    Timeout,
    #[error("failed")]
    Failed,
    /// YouTube answered with a sign-in or "confirm you're not a bot" challenge. Nothing is retried or worked around.
    #[error("verification required")]
    VerificationRequired,
    /// The media host refused the audio bytes themselves. Without an authorised session it serves only the first
    /// part of a stream, so a refusal is a sign-in question, not a network one.
    #[error("media host refused the audio bytes")]
    MediaRefused,
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum PlaylistErrorKind {
    #[error("not found")]
    NotFound,
    #[error("version conflict: expected {expected}, actual {actual}")]
    VersionConflict { expected: u64, actual: u64 },
    #[error("index {index} out of range for length {len}")]
    IndexOutOfRange { index: usize, len: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum QueueErrorKind {
    #[error("empty")]
    Empty,
    #[error("index {index} out of range for length {len}")]
    IndexOutOfRange { index: usize, len: usize },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_display_has_stable_lowercase_prefix() {
        assert!(
            CoreError::Validation("x".into())
                .to_string()
                .contains("validation")
        );
    }

    #[test]
    fn version_conflict_display_includes_both_versions() {
        let e = CoreError::Playlist(PlaylistErrorKind::VersionConflict {
            expected: 3,
            actual: 5,
        })
        .to_string();
        assert!(e.contains('3') && e.contains('5'), "{e}");
    }

    #[test]
    fn queue_index_out_of_range_display_includes_index_and_len() {
        let e = CoreError::Queue(QueueErrorKind::IndexOutOfRange { index: 4, len: 2 }).to_string();
        assert!(e.contains('4') && e.contains('2'), "{e}");
    }

    #[test]
    fn queue_empty_display_contains_empty() {
        assert!(
            CoreError::Queue(QueueErrorKind::Empty)
                .to_string()
                .contains("empty")
        );
    }

    #[test]
    fn playback_unsupported_capability_display_is_fixed_and_provider_free() {
        let s = CoreError::Playback(PlaybackErrorKind::UnsupportedCapability).to_string();
        assert_eq!(s, "playback error: unsupported capability");
        assert!(!s.contains("spotify") && !s.contains("youtube"), "{s}");
    }

    #[test]
    fn playback_invalid_transition_display_names_states() {
        let s = CoreError::Playback(PlaybackErrorKind::InvalidTransition {
            from: PlaybackState::Idle,
            to: PlaybackState::Playing,
        })
        .to_string();
        assert!(s.contains("Idle") && s.contains("Playing"), "{s}");
    }

    #[test]
    fn display_has_no_stack_trace_markers() {
        let all = [
            CoreError::Search("s".into()),
            CoreError::Provider("p".into()),
            CoreError::ProviderUnavailable,
            CoreError::AuthenticationRequired,
            CoreError::QuotaExceeded,
            CoreError::UnsupportedCapability,
            CoreError::Playback(PlaybackErrorKind::UnsupportedCapability),
            CoreError::Queue(QueueErrorKind::Empty),
            CoreError::Queue(QueueErrorKind::IndexOutOfRange { index: 1, len: 0 }),
            CoreError::Playlist(PlaylistErrorKind::NotFound),
            CoreError::Playlist(PlaylistErrorKind::VersionConflict {
                expected: 1,
                actual: 2,
            }),
            CoreError::Playlist(PlaylistErrorKind::IndexOutOfRange { index: 1, len: 0 }),
            CoreError::Validation("v".into()),
            CoreError::Cache("c".into()),
            CoreError::Network("n".into()),
        ];
        for e in all {
            let s = e.to_string().to_lowercase();
            assert!(!s.contains("backtrace") && !s.contains("at src/"), "{s}");
        }
    }
}
