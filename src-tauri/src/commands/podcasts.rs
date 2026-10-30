use crate::commands::dto::{IpcError, SearchHitDto};
use crate::commands::youtube::YouTubeState;
use crate::core::{errors::CoreError, podcasts};
use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PodcastPageDto {
    pub hits: Vec<SearchHitDto>,
    /// Pass back as `continuation` for the next page; `None` when the list ends.
    pub continuation: Option<String>,
}

/// Episodes for a show name or topic. Works signed out: it is an anonymous video search.
#[tauri::command]
pub async fn podcast_browse(
    state: tauri::State<'_, YouTubeState>,
    query: String,
    continuation: Option<String>,
) -> Result<PodcastPageDto, IpcError> {
    let yt = state.yt.clone().ok_or(IpcError::Internal)?;
    match podcasts::browse(&yt, &query, continuation.as_deref()).await {
        Ok(page) => Ok(PodcastPageDto {
            hits: page.hits.iter().map(SearchHitDto::from).collect(),
            continuation: page.continuation,
        }),
        Err(CoreError::Validation(_)) => Err(IpcError::Validation {
            message: "invalid podcast search".into(),
        }),
        Err(_) => Err(IpcError::Internal),
    }
}
