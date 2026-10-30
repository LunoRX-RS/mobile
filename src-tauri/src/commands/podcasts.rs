use crate::commands::dto::{IpcError, SearchHitDto};
use crate::commands::youtube::YouTubeState;
use crate::core::{errors::CoreError, podcasts, rss_podcast};
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

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShowDto {
    /// The show's feed address.
    pub id: String,
    pub title: String,
    pub author: String,
    pub artwork_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EpisodeDto {
    /// The episode's audio address, which is also what `player_load` takes as its id and address.
    pub id: String,
    pub title: String,
    pub show: String,
    pub published: Option<String>,
    pub duration_ms: Option<u64>,
    pub artwork_url: Option<String>,
}

fn failure(error: CoreError) -> IpcError {
    match error {
        CoreError::Validation(_) => IpcError::Validation {
            message: "invalid podcast request".into(),
        },
        _ => IpcError::Internal,
    }
}

/// Shows matching `query`, found through Apple's public podcast directory.
#[tauri::command]
pub async fn podcast_shows(query: String) -> Result<Vec<ShowDto>, IpcError> {
    let shows = rss_podcast::search_shows(&query).await.map_err(failure)?;
    Ok(shows
        .into_iter()
        .map(|s| ShowDto {
            id: s.id,
            title: s.title,
            author: s.author,
            artwork_url: s.artwork_url,
        })
        .collect())
}

/// Episodes of the show whose feed is at `feed_url`, newest first.
#[tauri::command]
pub async fn podcast_episodes(feed_url: String) -> Result<Vec<EpisodeDto>, IpcError> {
    let episodes = rss_podcast::episodes(&feed_url).await.map_err(failure)?;
    Ok(episodes
        .into_iter()
        .map(|e| EpisodeDto {
            id: e.id,
            title: e.title,
            show: e.show,
            published: e.published,
            duration_ms: e.duration_ms,
            artwork_url: e.artwork_url,
        })
        .collect())
}
