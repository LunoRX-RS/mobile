// Blueprint §8.2 / TARGET_STATE §3. Unknown or missing status is Unverified, never Supported.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CapabilityStatus {
    Supported,
    Unsupported,
    RequiresAuth,
    RequiresEntitlement,
    TemporarilyUnavailable,
    #[default]
    Unverified,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Search,
    TrackMetadata,
    PlaylistMetadata,
    Connect,
    Playback,
    Pause,
    Resume,
    Seek,
    Volume,
    QueueControl,
    EmbeddedPlayer,
    ExternalPlayback,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProviderCapabilities {
    pub search: CapabilityStatus,
    pub track_metadata: CapabilityStatus,
    pub playlist_metadata: CapabilityStatus,
    pub connect: CapabilityStatus,
    pub playback: CapabilityStatus,
    pub pause: CapabilityStatus,
    pub resume: CapabilityStatus,
    pub seek: CapabilityStatus,
    pub volume: CapabilityStatus,
    pub queue_control: CapabilityStatus,
    pub embedded_player: CapabilityStatus,
    pub external_playback: CapabilityStatus,
}

impl ProviderCapabilities {
    pub fn unverified() -> Self {
        Self::default()
    }

    pub fn is_supported(&self, cap: Capability) -> bool {
        self.status(cap) == CapabilityStatus::Supported
    }

    fn status(&self, cap: Capability) -> CapabilityStatus {
        match cap {
            Capability::Search => self.search,
            Capability::TrackMetadata => self.track_metadata,
            Capability::PlaylistMetadata => self.playlist_metadata,
            Capability::Connect => self.connect,
            Capability::Playback => self.playback,
            Capability::Pause => self.pause,
            Capability::Resume => self.resume,
            Capability::Seek => self.seek,
            Capability::Volume => self.volume,
            Capability::QueueControl => self.queue_control,
            Capability::EmbeddedPlayer => self.embedded_player,
            Capability::ExternalPlayback => self.external_playback,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use CapabilityStatus::*;

    const CAPABILITIES: [Capability; 12] = [
        Capability::Search,
        Capability::TrackMetadata,
        Capability::PlaylistMetadata,
        Capability::Connect,
        Capability::Playback,
        Capability::Pause,
        Capability::Resume,
        Capability::Seek,
        Capability::Volume,
        Capability::QueueControl,
        Capability::EmbeddedPlayer,
        Capability::ExternalPlayback,
    ];

    const STATUSES: [CapabilityStatus; 6] = [
        Supported,
        Unsupported,
        RequiresAuth,
        RequiresEntitlement,
        TemporarilyUnavailable,
        Unverified,
    ];

    // Sets only `cap` to `status`; every other field stays Unverified.
    fn with(cap: Capability, status: CapabilityStatus) -> ProviderCapabilities {
        let mut caps = ProviderCapabilities::unverified();
        match cap {
            Capability::Search => caps.search = status,
            Capability::TrackMetadata => caps.track_metadata = status,
            Capability::PlaylistMetadata => caps.playlist_metadata = status,
            Capability::Connect => caps.connect = status,
            Capability::Playback => caps.playback = status,
            Capability::Pause => caps.pause = status,
            Capability::Resume => caps.resume = status,
            Capability::Seek => caps.seek = status,
            Capability::Volume => caps.volume = status,
            Capability::QueueControl => caps.queue_control = status,
            Capability::EmbeddedPlayer => caps.embedded_player = status,
            Capability::ExternalPlayback => caps.external_playback = status,
        }
        caps
    }

    #[test]
    fn default_status_is_unverified() {
        assert_eq!(CapabilityStatus::default(), Unverified);
    }

    #[test]
    fn unverified_sets_every_field_unverified() {
        assert_eq!(
            ProviderCapabilities::unverified(),
            ProviderCapabilities {
                search: Unverified,
                track_metadata: Unverified,
                playlist_metadata: Unverified,
                connect: Unverified,
                playback: Unverified,
                pause: Unverified,
                resume: Unverified,
                seek: Unverified,
                volume: Unverified,
                queue_control: Unverified,
                embedded_player: Unverified,
                external_playback: Unverified,
            }
        );
    }

    #[test]
    fn is_supported_is_false_for_unverified() {
        let caps = ProviderCapabilities::unverified();
        for cap in CAPABILITIES {
            assert!(!caps.is_supported(cap), "{cap:?} should not be supported");
        }
    }

    #[test]
    fn is_supported_is_true_only_for_supported_field() {
        for cap in CAPABILITIES {
            for status in STATUSES {
                let caps = with(cap, status);
                assert_eq!(
                    caps.is_supported(cap),
                    status == Supported,
                    "{cap:?} = {status:?}"
                );
                for other in CAPABILITIES.into_iter().filter(|c| *c != cap) {
                    assert!(!caps.is_supported(other), "{other:?} leaked from {cap:?}");
                }
            }
        }
    }
}
