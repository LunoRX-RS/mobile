//! Main-window-only IPC for device-local media. Playlist contents come from the local Core store.
use crate::{
    commands::{player::PlayerState, sync::SyncEngineState, youtube::YouTubeState},
    core::playback::cache::{CacheError, OfflineManager, OfflineStatus, OfflineTrack},
};
use std::sync::Arc;
use tauri::{State, WebviewWindow};
pub struct OfflineState {
    pub manager: Result<Arc<OfflineManager>, CacheError>,
}
fn main(window: &WebviewWindow) -> Result<(), CacheError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(CacheError::Validation)
    }
}
fn manager(state: &OfflineState) -> Result<&Arc<OfflineManager>, CacheError> {
    state.manager.as_ref().map_err(|e| *e)
}
#[tauri::command]
pub async fn offline_status(
    window: WebviewWindow,
    state: State<'_, OfflineState>,
    sync: State<'_, SyncEngineState>,
    playlist_id: String,
    artwork_start: Option<u32>,
) -> Result<OfflineStatus, CacheError> {
    main(&window)?;
    let manager = manager(&state)?;
    let mut status = manager.status(&playlist_id)?;
    if status.keeping {
        let mut co = sync.checkout().await.map_err(|_| CacheError::Storage)?;
        if let Some(p) = co
            .engine()
            .local_playlists()
            .iter()
            .find(|p| p.id == playlist_id)
        {
            status.changed = p.entries.len() != status.tracks.len()
                || p.entries
                    .iter()
                    .zip(&status.tracks)
                    .any(|(a, b)| a.provider_track_id != b.track.id);
        }
    }
    for t in status
        .tracks
        .iter_mut()
        .skip(artwork_start.unwrap_or(0) as usize)
        .take(50)
    {
        t.track.artwork_url = manager
            .cache
            .cover(&t.track.id)
            .or(t.track.artwork_url.take());
    }
    Ok(status)
}
#[tauri::command]
pub async fn offline_keep(
    window: WebviewWindow,
    state: State<'_, OfflineState>,
    sync: State<'_, SyncEngineState>,
    youtube: State<'_, YouTubeState>,
    player: State<'_, PlayerState>,
    playlist_id: String,
) -> Result<(), CacheError> {
    main(&window)?;
    let (name, tracks) = {
        let mut co = sync.checkout().await.map_err(|_| CacheError::Storage)?;
        let p = co
            .engine()
            .local_playlists()
            .iter()
            .find(|p| p.id == playlist_id)
            .ok_or(CacheError::Validation)?;
        (
            p.name.clone(),
            p.entries
                .iter()
                .map(|e| OfflineTrack {
                    provider: match e.provider {
                        crate::sync::api::Provider::YouTube => "YouTube",
                        crate::sync::api::Provider::Spotify => "Spotify",
                    }
                    .into(),
                    id: e.provider_track_id.clone(),
                    title: e
                        .title
                        .clone()
                        .unwrap_or_else(|| e.provider_track_id.clone()),
                    artists: e.artists.clone().unwrap_or_default(),
                    artwork_url: e.artwork_url.clone(),
                    duration_ms: e.duration_ms,
                })
                .collect(),
        )
    };
    manager(&state)?.start(
        youtube.yt.clone().ok_or(CacheError::Unavailable)?,
        playlist_id,
        name,
        tracks,
        player.download_quality(),
    )
}
#[tauri::command]
pub fn offline_cancel(
    window: WebviewWindow,
    state: State<'_, OfflineState>,
    playlist_id: String,
) -> Result<(), CacheError> {
    main(&window)?;
    manager(&state)?.cancel(&playlist_id)
}
#[tauri::command]
pub fn offline_remove(
    window: WebviewWindow,
    state: State<'_, OfflineState>,
    playlist_id: String,
) -> Result<(), CacheError> {
    main(&window)?;
    let m = manager(&state)?;
    m.cancel(&playlist_id)?;
    m.cache.unpin(&playlist_id)
}
#[tauri::command]
pub fn offline_set_limit(
    window: WebviewWindow,
    state: State<'_, OfflineState>,
    bytes: u64,
) -> Result<(), CacheError> {
    main(&window)?;
    manager(&state)?.cache.set_limit(bytes)
}
