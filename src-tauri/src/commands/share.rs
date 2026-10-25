//! Share-link commands: the `lunorx://share/{token}` handoff from the OS, and the read of a shared playlist.
//! Thin: validation and the request live in `core::share_link` and `sync::api`.

use std::sync::Mutex as StdMutex;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use crate::commands::emit::emit_main;
use crate::core::share_link::token_from_url;
use crate::sync::api::{ApiClient, Provider};
use crate::sync::engine::{self, ErrorCategory};
use crate::sync::settings::default_base_url;

/// Emitted to the main window with the token when a share link arrives while the app runs.
pub const SHARE_EVENT: &str = "core://share/open";

/// A share token that arrived before the window listened for `SHARE_EVENT`. The window takes it once on mount.
#[derive(Default)]
pub struct PendingShare(StdMutex<Option<String>>);

/// Handles deep-link URLs from the OS. Links for other routes are ignored.
pub fn accept_urls(app: &AppHandle, urls: &[url::Url]) {
    let Some(token) = urls.iter().find_map(|url| token_from_url(url.as_str())) else {
        return;
    };
    // The token is a capability; whichever way it surfaces in free text it must mask.
    crate::core::secrets::registry().register(&token);
    if let Ok(mut slot) = app.state::<PendingShare>().0.lock() {
        *slot = Some(token.clone());
    }
    let _ = emit_main(app, SHARE_EVENT, &token);
}

#[tauri::command]
pub fn shared_link_take(pending: State<'_, PendingShare>) -> Option<String> {
    pending.0.lock().ok()?.take()
}

/// Extracts a share token from any LunoRX link (deep link or web URL) or a bare token.
/// Returns `None` for anything that is not a valid share link, so the UI can show one error.
#[tauri::command]
pub fn shared_token_from_url(raw: String) -> Option<String> {
    crate::core::share_link::token_from_url(&raw)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedEntryDto {
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: String,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPlaylistDto {
    pub name: String,
    pub description: Option<String>,
    pub entries: Vec<SharedEntryDto>,
}

/// Reads a shared playlist from the backend. The token is validated by the API client before any request.
#[tauri::command]
pub async fn shared_playlist_get(token: String) -> Result<SharedPlaylistDto, ErrorCategory> {
    let api = ApiClient::new(&default_base_url()).map_err(|e| engine::category(&e))?;
    let shared = api
        .get_shared(&token)
        .await
        .map_err(|e| engine::category(&e))?;
    Ok(SharedPlaylistDto {
        name: shared.name,
        description: shared.description,
        entries: shared
            .entries
            .into_iter()
            .map(|e| SharedEntryDto {
                provider: e.provider,
                provider_track_id: e.provider_track_id,
                provider_url: e.provider_url,
                title: e.title,
                artists: e.artists,
                artwork_url: e.artwork_url,
                duration_ms: e.duration_ms,
            })
            .collect(),
    })
}
