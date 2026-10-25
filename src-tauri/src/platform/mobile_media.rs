//! Lock-screen / notification metadata for iOS and Android. Mirrors `macos_media`: `poke()` marks that player state may
//! have changed, one task coalesces pokes, reads core progress and hands the snapshot to the lunor-native plugin.
//! Transport presses come back the other way, through `core::playback::mobile::watch` and `platform::control`.
use crate::commands::player::{PlaybackStateDto, PlayerState, progress};
use crate::core::playback::{lyrics, track_info};
use serde_json::json;
use std::{
    path::PathBuf,
    sync::{Arc, Mutex, OnceLock, PoisonError},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Manager, Wry};
use tauri_plugin_lunor_native::LunorNativeExt;
use tokio::sync::Notify;

static CHANGED: Notify = Notify::const_new();
static APP: OnceLock<AppHandle<Wry>> = OnceLock::new();

pub fn poke() {
    CHANGED.notify_one();
}

/// The playing track's timed lyric lines, loaded from the LRCLIB sidecar. `loaded: None` means "not
/// read yet"; retried once a second for a short window while the background prefetch may still write it.
struct LyricLines {
    id: String,
    loaded: Option<Option<Arc<[lyrics::TimedLine]>>>,
    since: Instant,
    last_try: Option<Instant>,
}

static LYRICS: Mutex<Option<LyricLines>> = Mutex::new(None);
/// The background LRCLIB fetch gives up after 9 s; retry reads a little past that, then stop.
const LYRIC_WINDOW: Duration = Duration::from_secs(15);
const LYRIC_RETRY: Duration = Duration::from_secs(1);

/// Best-effort line on screen right now. Reads the sidecar on a worker thread; every failure
/// (offline, no match yet, no timed lyrics) yields `None` and never delays the metadata push.
async fn current_lyric(
    app: &AppHandle<Wry>,
    id: &str,
    title: &str,
    artists: &[String],
    duration_ms: u64,
    position_ms: u64,
) -> Option<String> {
    let new_track = {
        let mut guard = LYRICS.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.as_ref().is_some_and(|state| state.id == id) {
            false
        } else {
            *guard = Some(LyricLines {
                id: id.to_owned(),
                loaded: None,
                since: Instant::now(),
                last_try: None,
            });
            true
        }
    };
    if new_track {
        // Cached (positive or negative) tracks are skipped inside; this only starts a network lookup.
        track_info::prefetch_lyrics(id, title, artists, i64::try_from(duration_ms).ok());
    }
    let (loaded, try_load) = {
        let mut guard = LYRICS.lock().unwrap_or_else(PoisonError::into_inner);
        let state = guard.as_mut().expect("track state just initialised");
        let due = state.loaded.is_none()
            && state.since.elapsed() < LYRIC_WINDOW
            && state.last_try.is_none_or(|t| t.elapsed() >= LYRIC_RETRY);
        if due {
            state.last_try = Some(Instant::now());
        }
        (state.loaded.clone(), due)
    };
    let mut lines = loaded.flatten();
    if try_load {
        let app = app.clone();
        let cache_id = id.to_owned();
        let found = tauri::async_runtime::spawn_blocking(move || {
            let root: PathBuf = app.path().app_cache_dir().ok()?;
            track_info::cached_lrclib(&cache_id, &track_info::cache_file(&root, &cache_id))
                .and_then(|info| info.lyrics_lines)
                .map(Arc::from)
        })
        .await
        .ok()
        .flatten();
        let mut guard = LYRICS.lock().unwrap_or_else(PoisonError::into_inner);
        // Use and remember the fresh read only while this track is still the playing one.
        if let Some(state) = guard.as_mut().filter(|state| state.id == id) {
            state.loaded = Some(found.clone());
            lines = found;
        }
    }
    lines
        .as_ref()
        .and_then(|lines| lyrics::line_at(lines, position_ms))
        .map(|line| line.text.clone())
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
            let lyric = current_lyric(
                app,
                &track.provider_track_id,
                &track.title,
                &track.artists,
                dto.duration_ms,
                position,
            )
            .await;
            let payload = json!({
                "title": track.title,
                "artist": track.artists.join(", "),
                "artworkUrl": track.artwork_url,
                "durationMs": dto.duration_ms,
                "positionMs": position,
                "playing": dto.player.state == S::Playing,
                "lyric": lyric,
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
