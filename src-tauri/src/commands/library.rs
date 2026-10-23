use crate::core::library::{ArtistProfile, LibraryError, SavedLibrary, SavedTrack};
use tauri::{AppHandle, Emitter, State, WebviewWindow};
pub struct LibraryState {
    library: std::sync::Mutex<Result<SavedLibrary, LibraryError>>,
    path: Result<std::path::PathBuf, LibraryError>,
}
impl LibraryState {
    pub fn open(path: Result<std::path::PathBuf, LibraryError>) -> Self {
        let library = path
            .as_ref()
            .map_err(|e| *e)
            .and_then(|p| SavedLibrary::open(p.clone()));
        if let Err(kind) = &library {
            eprintln!("device library unavailable: {kind:?}");
        }
        Self {
            library: std::sync::Mutex::new(library),
            path,
        }
    }
    pub(crate) fn with<T>(
        &self,
        action: impl FnOnce(&SavedLibrary) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let mut library = self.library.lock().map_err(|_| LibraryError::Storage)?;
        if library.is_err() {
            *library = self
                .path
                .as_ref()
                .map_err(|e| *e)
                .and_then(|p| SavedLibrary::open(p.clone()));
        }
        action(library.as_ref().map_err(|e| *e)?)
    }
}
pub struct HistoryState(pub LibraryState);
#[tauri::command]
pub fn library_history(
    window: WebviewWindow,
    state: State<'_, HistoryState>,
) -> Result<Vec<SavedTrack>, LibraryError> {
    main(&window)?;
    state.0.with(SavedLibrary::list)
}
#[tauri::command]
pub fn library_clear_history(
    window: WebviewWindow,
    state: State<'_, HistoryState>,
) -> Result<Vec<SavedTrack>, LibraryError> {
    main(&window)?;
    state.0.with(SavedLibrary::clear)
}
fn main(window: &WebviewWindow) -> Result<(), LibraryError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(LibraryError::Validation)
    }
}
#[tauri::command]
pub fn library_likes(
    window: WebviewWindow,
    state: State<'_, LibraryState>,
) -> Result<Vec<SavedTrack>, LibraryError> {
    main(&window)?;
    state.with(SavedLibrary::list)
}
#[tauri::command]
pub fn library_set_like(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, LibraryState>,
    track: SavedTrack,
    liked: bool,
) -> Result<Vec<SavedTrack>, LibraryError> {
    main(&window)?;
    let youtube_id = (track.provider == crate::core::domain::ProviderId::YouTube)
        .then(|| track.provider_track_id.clone());
    let result = state.with(|library| library.set_like(track, liked))?;
    // Sent to the YouTube account only when the user turned that on; it never delays or fails the local like.
    if let Some(id) = youtube_id {
        crate::commands::youtube_import::push_like(&app, &id, liked);
    }
    let _ = app.emit_to("main", "core://library/changed", ());
    Ok(result)
}
#[tauri::command]
pub async fn library_artist(
    window: WebviewWindow,
    youtube: State<'_, crate::commands::youtube::YouTubeState>,
    provider: crate::core::domain::ProviderId,
    id: String,
) -> Result<ArtistProfile, LibraryError> {
    main(&window)?;
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| LibraryError::Unavailable)?;
    crate::core::library::profile(
        youtube.yt.as_ref().ok_or(LibraryError::Unavailable)?,
        provider,
        &id,
    )
    .await
}

#[tauri::command]
pub async fn library_album(
    window: WebviewWindow,
    youtube: State<'_, crate::commands::youtube::YouTubeState>,
    provider: crate::core::domain::ProviderId,
    id: String,
) -> Result<crate::core::library::AlbumProfile, LibraryError> {
    main(&window)?;
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| LibraryError::Unavailable)?;
    crate::core::library::album(
        youtube.yt.as_ref().ok_or(LibraryError::Unavailable)?,
        provider,
        &id,
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn retry_reopens_library_after_another_instance_releases_it() {
        let root = std::env::temp_dir().join(format!(
            "lunor-library-retry-{:032x}",
            rand::random::<u128>()
        ));
        let path = root.join("likes.json");
        let first = SavedLibrary::open(path.clone()).unwrap();
        let state = LibraryState::open(Ok(path));
        assert!(matches!(
            state.with(SavedLibrary::list),
            Err(LibraryError::InUse)
        ));
        drop(first);
        assert!(state.with(SavedLibrary::list).unwrap().is_empty());
        drop(state);
        std::fs::remove_dir_all(root).unwrap();
    }
}
