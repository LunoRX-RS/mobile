//! Lock-screen / notification metadata for iOS and Android. Mirrors `macos_media`: `poke()` marks that player state may
//! have changed, one task coalesces pokes, reads core progress and hands the snapshot to the lunor-native plugin.
//! Transport presses come back the other way, through `core::playback::mobile::watch` and `platform::control`.
use crate::commands::player::{PlaybackStateDto, PlayerState, progress};
use serde_json::json;
use std::{sync::OnceLock, time::Duration};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_lunor_native::LunorNativeExt;
use tokio::sync::Notify;

static CHANGED: Notify = Notify::const_new();
static APP: OnceLock<AppHandle<Wry>> = OnceLock::new();

pub fn poke() {
    CHANGED.notify_one();
}

pub fn install(app: &AppHandle<Wry>) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    tauri::async_runtime::spawn(async {
        let Some(app) = APP.get() else { return };
        loop {
            CHANGED.notified().await;
            // A seek or track change fires several pokes in a row; publish once.
            tokio::time::sleep(Duration::from_millis(150)).await;
            let state = app.state::<PlayerState>();
            let dto = progress(&state).await;
            use PlaybackStateDto as S;
            let Some(track) = dto.track.as_ref() else {
                continue;
            };
            if !matches!(
                dto.player.state,
                S::Resolving | S::Preparing | S::Ready | S::Playing | S::Paused | S::Buffering
            ) {
                continue;
            }
            let position = if dto.duration_ms > 0 {
                dto.position_ms.min(dto.duration_ms)
            } else {
                dto.position_ms
            };
            let payload = json!({
                "title": track.title,
                "artist": track.artists.join(", "),
                "artworkUrl": track.artwork_url,
                "durationMs": dto.duration_ms,
                "positionMs": position,
                "playing": dto.player.state == S::Playing,
            });
            let app = app.clone();
            // Blocking plugin call; a failure only means the lock screen keeps the previous metadata.
            let _ = tauri::async_runtime::spawn_blocking(move || {
                app.lunor_native()
                    .call::<serde_json::Value>("nowPlaying", payload)
            })
            .await;
        }
    });
}
