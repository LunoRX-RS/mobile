use std::collections::HashMap;

use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;

// Capability records only; no adapter trait until Phase 2 engines land.
#[derive(Default)]
pub struct ProviderRegistry {
    entries: HashMap<ProviderId, ProviderCapabilities>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn capabilities_for(&self, id: ProviderId) -> ProviderCapabilities {
        self.entries
            .get(&id)
            .cloned()
            .unwrap_or_else(ProviderCapabilities::unverified)
    }

    // Capabilities that depend on runtime state (e.g. a saved API key) are refreshed through this.
    pub fn set_search(&mut self, id: ProviderId, status: CapabilityStatus) {
        if let Some(caps) = self.entries.get_mut(&id) {
            caps.search = status;
        }
    }

    pub fn register(
        &mut self,
        id: ProviderId,
        caps: ProviderCapabilities,
    ) -> Result<(), CoreError> {
        if self.entries.contains_key(&id) {
            return Err(CoreError::Validation(format!(
                "provider already registered: {id:?}"
            )));
        }
        self.entries.insert(id, caps);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_registry_returns_unverified_for_both_providers() {
        let registry = ProviderRegistry::new();
        for id in [ProviderId::Spotify, ProviderId::YouTube] {
            assert_eq!(
                registry.capabilities_for(id),
                ProviderCapabilities::unverified()
            );
        }
    }

    #[test]
    fn register_then_capabilities_for_returns_registered_value() {
        let mut registry = ProviderRegistry::new();
        let caps = ProviderCapabilities {
            search: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        registry
            .register(ProviderId::Spotify, caps.clone())
            .unwrap();
        assert_eq!(registry.capabilities_for(ProviderId::Spotify), caps);
    }

    #[test]
    fn set_search_changes_only_search_and_ignores_unregistered() {
        let mut registry = ProviderRegistry::new();
        registry.set_search(ProviderId::YouTube, CapabilityStatus::Supported);
        assert_eq!(
            registry.capabilities_for(ProviderId::YouTube),
            ProviderCapabilities::unverified()
        );
        let caps = ProviderCapabilities {
            playback: CapabilityStatus::Unsupported,
            ..ProviderCapabilities::unverified()
        };
        registry.register(ProviderId::YouTube, caps).unwrap();
        registry.set_search(ProviderId::YouTube, CapabilityStatus::Supported);
        let got = registry.capabilities_for(ProviderId::YouTube);
        assert_eq!(got.search, CapabilityStatus::Supported);
        assert_eq!(got.playback, CapabilityStatus::Unsupported);
    }

    #[test]
    fn duplicate_register_is_validation_error_and_keeps_first_value() {
        let mut registry = ProviderRegistry::new();
        let first = ProviderCapabilities {
            search: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        let second = ProviderCapabilities {
            search: CapabilityStatus::Unsupported,
            ..ProviderCapabilities::unverified()
        };
        registry
            .register(ProviderId::YouTube, first.clone())
            .unwrap();
        let err = registry.register(ProviderId::YouTube, second).unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)), "got: {err:?}");
        assert_eq!(registry.capabilities_for(ProviderId::YouTube), first);
    }
}
