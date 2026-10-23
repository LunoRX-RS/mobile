use crate::commands::clock;
use crate::commands::dto::{IpcError, ProviderStatusDto, SearchQueryArgs, SearchResponseDto};
use crate::commands::state::SearchState;
use crate::commands::validate::{build_request, validate_cancellation_id};
use crate::core::domain::ProviderId;

// Removes the token on every exit path (success, error, cancel).
struct Registration<'a> {
    state: &'a SearchState,
    id: String,
}

impl Drop for Registration<'_> {
    fn drop(&mut self) {
        self.state.release(&self.id);
    }
}

pub async fn run_search(
    state: &SearchState,
    mut args: SearchQueryArgs,
    today: u64,
) -> Result<SearchResponseDto, IpcError> {
    // Empty means all providers; the engine only runs the providers it is asked for.
    if args.providers.is_empty() {
        args.providers = [ProviderId::Spotify, ProviderId::YouTube]
            .iter()
            .map(|p| p.as_str().to_owned())
            .collect();
    }
    let request = build_request(&args)?;
    let token = state.register(&args.cancellation_id)?;
    let _registration = Registration {
        state,
        id: args.cancellation_id.clone(),
    };
    let mut engine = state.engine_lock().await;
    let response = engine.search(request, token, today).await;
    Ok(SearchResponseDto::from(&response))
}

pub fn cancel_search(state: &SearchState, id: &str) -> Result<(), IpcError> {
    validate_cancellation_id(id)?;
    state.cancel(id);
    Ok(())
}

// Async because the engine lock is held for the whole of a running search; a sync read would block the Tauri main thread.
pub async fn list_providers(state: &SearchState) -> Vec<ProviderStatusDto> {
    let engine = state.engine_lock().await;
    [ProviderId::Spotify, ProviderId::YouTube]
        .into_iter()
        .map(|id| {
            let caps = engine.registry.capabilities_for(id.clone());
            ProviderStatusDto::from((id, &caps))
        })
        .collect()
}

#[tauri::command]
pub async fn search_query(
    state: tauri::State<'_, SearchState>,
    args: SearchQueryArgs,
) -> Result<SearchResponseDto, IpcError> {
    run_search(&state, args, clock::today()).await
}

#[tauri::command]
pub fn search_cancel(
    state: tauri::State<'_, SearchState>,
    cancellation_id: String,
) -> Result<(), IpcError> {
    cancel_search(&state, &cancellation_id)
}

#[tauri::command]
pub async fn providers_list(
    state: tauri::State<'_, SearchState>,
    player: tauri::State<'_, crate::commands::player::PlayerState>,
) -> Result<Vec<ProviderStatusDto>, IpcError> {
    let mut providers = list_providers(&state).await;
    if player.available {
        for provider in &mut providers {
            if provider.provider == "YouTube" {
                provider.playback = "Supported".into();
            }
        }
    }
    Ok(providers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn args(id: &str, query: &str, providers: &[&str], limit: u8) -> SearchQueryArgs {
        SearchQueryArgs {
            query: query.into(),
            providers: providers.iter().map(|s| s.to_string()).collect(),
            limit,
            cancellation_id: id.into(),
        }
    }

    #[tokio::test]
    async fn unknown_provider_is_validation_and_registers_nothing() {
        let s = SearchState::for_test();
        let r = run_search(&s, args("c1", "rock", &["Deezer"], 5), 0).await;
        assert!(
            matches!(r, Err(IpcError::Validation { ref message }) if message == "unknown provider")
        );
        assert_eq!(s.cancel_map_len(), 0);
    }

    #[tokio::test]
    async fn empty_providers_means_both_skipped_in_declaration_order() {
        let s = SearchState::for_test();
        let dto = run_search(&s, args("c1", "rock", &[], 5), 0).await.unwrap();
        assert_eq!(
            serde_json::to_value(&dto).unwrap(),
            json!({"query": "rock", "hits": [], "groups": [
                {"kind": "skipped", "provider": "Spotify", "reason": "capabilityNotSupported"},
                {"kind": "skipped", "provider": "YouTube", "reason": "capabilityNotSupported"}
            ]})
        );
    }

    #[tokio::test]
    async fn duplicate_id_in_flight_keeps_first_token() {
        let s = SearchState::for_test();
        let first = s.register("x").unwrap();
        let r = run_search(&s, args("x", "rock", &[], 5), 0).await;
        assert!(matches!(r, Err(IpcError::DuplicateCancellationId)));
        assert_eq!(s.cancel_map_len(), 1);
        cancel_search(&s, "x").unwrap();
        assert!(first.is_cancelled());
    }

    #[tokio::test]
    async fn token_is_released_after_success() {
        let s = SearchState::for_test();
        assert!(
            run_search(&s, args("c1", "rock", &["YouTube"], 5), 0)
                .await
                .is_ok()
        );
        assert_eq!(s.cancel_map_len(), 0);
    }

    #[tokio::test]
    async fn token_is_not_registered_after_validation_failure() {
        let s = SearchState::for_test();
        let r = run_search(&s, args("a b", "rock", &[], 5), 0).await;
        assert!(matches!(r, Err(IpcError::Validation { .. })));
        assert_eq!(s.cancel_map_len(), 0);
    }

    #[test]
    fn cancel_unknown_id_is_ok() {
        let s = SearchState::for_test();
        assert!(cancel_search(&s, "missing").is_ok());
        assert_eq!(s.cancel_map_len(), 0);
    }

    #[test]
    fn cancel_registered_id_sets_its_token() {
        let s = SearchState::for_test();
        let t = s.register("a").unwrap();
        cancel_search(&s, "a").unwrap();
        assert!(t.is_cancelled());
    }

    #[test]
    fn cancel_invalid_id_is_validation() {
        let s = SearchState::for_test();
        assert!(matches!(
            cancel_search(&s, "a b"),
            Err(IpcError::Validation { .. })
        ));
    }

    #[tokio::test]
    async fn providers_list_reports_capabilities_in_order() {
        let s = SearchState::for_test();
        assert_eq!(
            serde_json::to_value(list_providers(&s).await).unwrap(),
            json!([
                {"provider": "Spotify", "search": "Unsupported", "playback": "Unsupported", "externalPlayback": "Unsupported"},
                {"provider": "YouTube", "search": "RequiresAuth", "playback": "Unsupported", "externalPlayback": "Supported"}
            ])
        );
    }
}
