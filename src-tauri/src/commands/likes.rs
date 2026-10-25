//! Glue between the liked-songs sync engine and the app: the engine (`sync::likes`) owns the account, the outbox and the
//! socket; this module gives it the device library and the Tauri events. No token or account id crosses to the webview.
use std::path::{Path, PathBuf};
use std::sync::{Arc, MutexGuard, PoisonError};

use tauri::{AppHandle, Emitter, Manager};

use crate::commands::library::LibraryState;
use crate::core::domain::ProviderId;
use crate::core::library::{SavedLibrary, SavedTrack};
use crate::sync::api::ApiClient;
use crate::sync::credentials::{KeyringTokenStore, TokenStore};
use crate::sync::likes::{self, HttpBackend, Local, Shared, Unavailable};
use crate::sync::settings::default_base_url;

pub struct LikesState {
    shared: Arc<Shared>,
}

impl LikesState {
    pub fn new(root: PathBuf) -> Self {
        Self {
            shared: Arc::new(Shared::new(root)),
        }
    }

    /// Held while a like is written and while the account switches, so a toggle queues for the account it belongs to.
    pub fn gate(&self) -> MutexGuard<'_, ()> {
        self.shared
            .gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Queues one change for the backend. Signed out, the local file still holds it and nothing is queued.
    pub fn note_like(&self, track: &SavedTrack, liked: bool) {
        if self.shared.note(track, liked) {
            self.shared.flush.notify_one();
        }
    }

    /// A sign-in or sign-out finished: re-read the token now instead of at the next wait.
    pub fn kick(&self) {
        self.shared.kick.notify_one();
    }
}

/// The device library as the engine sees it. Every call happens between awaits, so a plain mutex is enough.
struct TauriLocal {
    app: AppHandle,
}

impl Local for TauriLocal {
    fn list(&self) -> Vec<SavedTrack> {
        self.app
            .state::<LibraryState>()
            .with(SavedLibrary::list)
            .unwrap_or_default()
    }

    fn replace(&self, tracks: Vec<SavedTrack>) {
        let _ = self
            .app
            .state::<LibraryState>()
            .with(|library| library.replace(tracks));
    }

    fn upsert(&self, track: SavedTrack) {
        let _ = self
            .app
            .state::<LibraryState>()
            .with(|library| library.set_like(track, true));
    }

    fn remove(&self, provider: &ProviderId, id: &str) {
        let _ = self
            .app
            .state::<LibraryState>()
            .with(|library| library.remove_like(provider, id));
    }

    fn switch(&self, path: &Path) -> Result<(), Unavailable> {
        self.app
            .state::<LibraryState>()
            .switch(path.to_path_buf())
            .map_err(|_| Unavailable)
    }
}

/// Starts the engine once, at setup. Does nothing when the app data directory is unknown (the library is unavailable
/// then too); likes stay local and work as before.
pub fn start(app: &AppHandle) {
    let Some(state) = app.try_state::<LikesState>() else {
        return;
    };
    let Ok(api) = ApiClient::new(&default_base_url()) else {
        return;
    };
    let tokens = Arc::new(KeyringTokenStore);
    let backend = Arc::new(HttpBackend::new(api, tokens.clone()));
    let local = Arc::new(TauriLocal { app: app.clone() });
    let shared = state.shared.clone();
    let handle = app.clone();
    let on_change: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = handle.emit_to("main", "core://library/changed", ());
    });
    tauri::async_runtime::spawn(likes::run(
        shared,
        local,
        backend,
        tokens as Arc<dyn TokenStore + Send + Sync>,
        default_base_url(),
        on_change,
    ));
}
