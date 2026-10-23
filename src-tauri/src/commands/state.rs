use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::commands::dto::IpcError;
use crate::commands::providers::KeyringKeyStore;
#[cfg(test)]
use crate::commands::providers::MemoryKeyStore;
use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
use crate::core::domain::ProviderId;
use crate::core::registry::ProviderRegistry;
use crate::core::search::youtube::{DEFAULT_DAILY_SEARCH_LIMIT, YouTubeSearch};
use crate::core::search::{CancelToken, QuotaBudget, SearchEngine, SearchProvider};
use crate::sync::credentials::TokenStore;

pub struct SearchState {
    engine: tokio::sync::Mutex<SearchEngine>,
    cancels: Mutex<HashMap<String, CancelToken>>,
    // Write-only from the UI's point of view: commands/providers.rs stores, the YouTube adapter reads.
    youtube_key: Arc<dyn TokenStore>,
    direct_youtube: bool,
}

impl SearchState {
    // Production uses direct YouTube Music search; the Data API adapter remains for compatibility tests.
    pub fn production(yt: Option<crate::core::yt::Yt>) -> Self {
        let mut state = Self::with_key_store(Arc::new(KeyringKeyStore::youtube()));
        if let Some(yt) = yt {
            let engine = state.engine.get_mut();
            engine.providers = vec![Arc::new(
                crate::core::search::youtube_music::YouTubeMusicSearch::new(yt),
            )];
            engine
                .registry
                .set_search(ProviderId::YouTube, CapabilityStatus::Supported);
            // Local abuse guard, not an API quota: direct YouTube Music does not use Data API keys.
            engine
                .budgets
                .insert(ProviderId::YouTube, QuotaBudget::new(1000));
            engine.timeout = Duration::from_secs(20);
            state.direct_youtube = true;
        }
        state
    }

    pub fn with_key_store(youtube_key: Arc<dyn TokenStore>) -> Self {
        let mut registry = ProviderRegistry::new();
        let spotify = ProviderCapabilities {
            search: CapabilityStatus::Unsupported,
            playback: CapabilityStatus::Unsupported,
            external_playback: CapabilityStatus::Unsupported,
            ..ProviderCapabilities::unverified()
        };
        let youtube = ProviderCapabilities {
            search: CapabilityStatus::RequiresAuth,
            playback: CapabilityStatus::Unsupported,
            external_playback: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        };
        registry
            .register(ProviderId::Spotify, spotify)
            .expect("fresh registry has no entries");
        registry
            .register(ProviderId::YouTube, youtube)
            .expect("fresh registry has no entries");
        let mut providers: Vec<Arc<dyn SearchProvider>> = Vec::new();
        // Client construction fails only if the TLS backend cannot start; YouTube then stays skipped.
        if let Ok(adapter) = YouTubeSearch::new(youtube_key.clone()) {
            providers.push(Arc::new(adapter));
        }
        let budgets = HashMap::from([(
            ProviderId::YouTube,
            QuotaBudget::new(DEFAULT_DAILY_SEARCH_LIMIT),
        )]);
        Self {
            engine: tokio::sync::Mutex::new(SearchEngine::new(
                registry,
                budgets,
                providers,
                Duration::from_secs(8),
            )),
            cancels: Mutex::new(HashMap::new()),
            youtube_key,
            direct_youtube: false,
        }
    }

    #[cfg(test)]
    pub fn for_test() -> Self {
        Self::with_key_store(Arc::new(MemoryKeyStore::default()))
    }

    pub(crate) fn youtube_key(&self) -> &dyn TokenStore {
        &*self.youtube_key
    }

    // A poisoned lock still holds a valid map; recover it instead of panicking.
    fn cancels(&self) -> MutexGuard<'_, HashMap<String, CancelToken>> {
        self.cancels.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn register(&self, id: &str) -> Result<CancelToken, IpcError> {
        let mut map = self.cancels();
        if map.contains_key(id) {
            return Err(IpcError::DuplicateCancellationId);
        }
        let token = CancelToken::new();
        map.insert(id.to_owned(), token.clone());
        Ok(token)
    }

    // The engine field stays private; search.rs needs the lock to run and to read the registry.
    // Every user of the engine goes through here, so the YouTube search capability always matches the key store.
    // A store error counts as "no key": Supported is never reported without a readable key.
    pub(crate) async fn engine_lock(&self) -> tokio::sync::MutexGuard<'_, SearchEngine> {
        let mut engine = self.engine.lock().await;
        if self.direct_youtube {
            return engine;
        }
        let status = match self.youtube_key.get() {
            Ok(Some(k)) if !k.is_empty() => CapabilityStatus::Supported,
            _ => CapabilityStatus::RequiresAuth,
        };
        engine.registry.set_search(ProviderId::YouTube, status);
        engine
    }

    // Touches only the cancel map, never the engine lock, so cancel is not blocked by a running search.
    pub fn cancel(&self, id: &str) {
        if let Some(token) = self.cancels().get(id) {
            token.cancel();
        }
    }

    pub fn release(&self, id: &str) {
        self.cancels().remove(id);
    }

    #[cfg(test)]
    pub fn cancel_map_len(&self) -> usize {
        self.cancels().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::dto::IpcError;
    use crate::core::domain::ProviderId;

    #[tokio::test]
    async fn direct_youtube_search_does_not_require_an_api_key() {
        let state =
            SearchState::production(Some(crate::core::playback::extract::resolver().unwrap()));
        let engine = state.engine_lock().await;
        assert_eq!(
            engine.registry.capabilities_for(ProviderId::YouTube).search,
            CapabilityStatus::Supported
        );
        assert_eq!(engine.budgets[&ProviderId::YouTube].limit_per_day, 1000);
        assert!(state.direct_youtube);
    }

    #[test]
    fn duplicate_register_keeps_first_token() {
        let s = SearchState::for_test();
        let first = s.register("a").unwrap();
        assert!(matches!(
            s.register("a"),
            Err(IpcError::DuplicateCancellationId)
        ));
        assert_eq!(s.cancel_map_len(), 1);
        s.cancel("a");
        assert!(first.is_cancelled());
    }

    #[test]
    fn release_removes_and_unknown_release_is_noop() {
        let s = SearchState::for_test();
        s.register("a").unwrap();
        s.release("zzz");
        assert_eq!(s.cancel_map_len(), 1);
        s.release("a");
        assert_eq!(s.cancel_map_len(), 0);
    }

    #[test]
    fn register_after_release_succeeds() {
        let s = SearchState::for_test();
        s.register("a").unwrap();
        s.release("a");
        assert!(s.register("a").is_ok());
    }

    #[test]
    fn cancel_unknown_id_changes_nothing() {
        let s = SearchState::for_test();
        let t = s.register("a").unwrap();
        s.cancel("missing");
        assert!(!t.is_cancelled());
        assert_eq!(s.cancel_map_len(), 1);
    }

    #[test]
    fn cancel_sets_registered_token_without_engine_lock() {
        let s = SearchState::for_test();
        let t = s.register("a").unwrap();
        let _engine_guard = s.engine.try_lock().unwrap();
        s.cancel("a");
        assert!(t.is_cancelled());
    }

    #[tokio::test]
    async fn youtube_search_is_supported_only_while_a_key_is_saved() {
        let store = Arc::new(MemoryKeyStore::default());
        let s = SearchState::with_key_store(store.clone());
        let search = |s: &SearchState| {
            let e = s.engine.try_lock().unwrap();
            (
                e.registry.capabilities_for(ProviderId::Spotify).search,
                e.registry.capabilities_for(ProviderId::YouTube).search,
            )
        };
        drop(s.engine_lock().await);
        assert_eq!(
            search(&s),
            (
                CapabilityStatus::Unsupported,
                CapabilityStatus::RequiresAuth
            )
        );
        store.set("a".repeat(30).as_str()).unwrap();
        drop(s.engine_lock().await);
        assert_eq!(search(&s).1, CapabilityStatus::Supported);
        store.clear().unwrap();
        drop(s.engine_lock().await);
        assert_eq!(search(&s).1, CapabilityStatus::RequiresAuth);
        // A broken store never yields Supported.
        store.set("a".repeat(30).as_str()).unwrap();
        store.fail.store(true, std::sync::atomic::Ordering::SeqCst);
        drop(s.engine_lock().await);
        assert_eq!(search(&s).1, CapabilityStatus::RequiresAuth);
    }

    #[test]
    fn production_registers_youtube_adapter_with_100_a_day_budget_and_no_spotify() {
        let s = SearchState::for_test();
        let e = s.engine.try_lock().unwrap();
        assert_eq!(e.providers.len(), 1);
        assert_eq!(e.providers[0].id(), ProviderId::YouTube);
        assert_eq!(
            e.budgets.get(&ProviderId::YouTube).unwrap().limit_per_day,
            100
        );
        assert!(!e.budgets.contains_key(&ProviderId::Spotify));
    }
}
