use crate::{
    commands::{
        library::{HistoryState, LibraryState},
        youtube::YouTubeState,
    },
    core::{
        discovery::FeedSection,
        library::{LibraryError, SavedLibrary},
    },
};
use tauri::{State, WebviewWindow};
fn main(window: &WebviewWindow) -> Result<(), LibraryError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(LibraryError::Validation)
    }
}
#[tauri::command]
pub async fn discovery_feed(
    window: WebviewWindow,
    youtube: State<'_, YouTubeState>,
    history: State<'_, HistoryState>,
    likes: State<'_, LibraryState>,
    kind: String,
    topic: String,
) -> Result<Vec<FeedSection>, LibraryError> {
    main(&window)?;
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| LibraryError::Unavailable)?;
    let seed = history
        .0
        .with(SavedLibrary::list)
        .ok()
        .and_then(|t| {
            t.into_iter()
                .find(|t| t.provider == crate::core::domain::ProviderId::YouTube)
        })
        .or_else(|| {
            likes.with(SavedLibrary::list).ok().and_then(|t| {
                t.into_iter()
                    .find(|t| t.provider == crate::core::domain::ProviderId::YouTube)
            })
        })
        .map(|t| t.provider_track_id);
    crate::core::discovery::feed(
        youtube.yt.as_ref().ok_or(LibraryError::Unavailable)?,
        &kind,
        &topic,
        seed.as_deref(),
    )
    .await
}
/// The moods and genres on YouTube Music's Explore page.
#[tauri::command]
pub async fn discovery_genres(
    window: WebviewWindow,
    youtube: State<'_, YouTubeState>,
) -> Result<Vec<crate::core::discovery::Genre>, LibraryError> {
    main(&window)?;
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(1);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| LibraryError::Unavailable)?;
    crate::core::discovery::genres(youtube.yt.as_ref().ok_or(LibraryError::Unavailable)?).await
}
#[tauri::command]
pub async fn search_suggestions(
    window: WebviewWindow,
    youtube: State<'_, YouTubeState>,
    query: String,
) -> Result<Vec<String>, LibraryError> {
    main(&window)?;
    static REQUESTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
    let _permit = REQUESTS
        .try_acquire()
        .map_err(|_| LibraryError::Unavailable)?;
    crate::core::discovery::suggestions(
        youtube.yt.as_ref().ok_or(LibraryError::Unavailable)?,
        &query,
    )
    .await
}
