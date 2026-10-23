//! Provider-backed discovery. All recommendations and search suggestions stay on the desktop.
use crate::core::{
    domain::ProviderId,
    library::{LibraryError, SavedTrack, saved_track},
    yt::Yt,
};
use innerlight::music::{Item, Section, Track};
use serde::Serialize;
use std::time::Duration;
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedSection {
    pub title: String,
    pub tracks: Vec<SavedTrack>,
    /// Albums shown as covers (New albums). Empty for track sections.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub albums: Vec<FeedAlbum>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedAlbum {
    pub id: String,
    pub name: String,
    pub year: Option<u16>,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
}
/// A mood or genre from YouTube Music's Explore page.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Genre {
    pub id: String,
    pub name: String,
    pub mood: bool,
}
fn section(title: &str, tracks: Vec<SavedTrack>) -> FeedSection {
    FeedSection {
        title: title.into(),
        tracks,
        albums: Vec::new(),
    }
}
/// A genre id as YouTube Music gives it (the opaque button value): url-safe characters, `%` and `=`, bounded.
fn genre_id(topic: &str) -> Option<&str> {
    topic.strip_prefix("genre:").filter(|id| {
        (1..=96).contains(&id.len())
            && id
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'%' | b'='))
    })
}
fn tracks(items: Vec<Track>) -> Vec<SavedTrack> {
    let mut seen = std::collections::HashSet::new();
    items
        .into_iter()
        .filter_map(|t| {
            let mut track = saved_track(t, None)?;
            (track.validate().is_ok() && seen.insert(track.provider_track_id.clone()))
                .then_some(track)
        })
        .take(20)
        .collect()
}
/// The tracks a shelf holds, in order. Other item kinds (albums, artists) are not tracks.
fn section_tracks(section: Section) -> Vec<Track> {
    section
        .items
        .into_iter()
        .filter_map(|item| match item {
            Item::Track(t) => Some(t),
            _ => None,
        })
        .collect()
}
pub fn topic_query(topic: &str) -> Option<(&'static str, &'static str)> {
    match topic {
        "focus" => Some(("Focus", "instrumental focus music")),
        "chill" => Some(("Chill", "chill music")),
        "pop" => Some(("Pop", "pop music")),
        "hiphop" => Some(("Hip-hop", "hip hop music")),
        "thai" => Some(("Thai music", "เพลงไทย")),
        "acoustic" => Some(("Acoustic", "acoustic music")),
        _ => None,
    }
}
fn personalized_title(kind: &str) -> Option<&'static str> {
    match kind {
        "home" => Some("More from your listening"),
        "explore" => Some("Picked for you"),
        _ => None,
    }
}
pub async fn feed(
    yt: &Yt,
    kind: &str,
    topic: &str,
    seed: Option<&str>,
) -> Result<Vec<FeedSection>, LibraryError> {
    if !["home", "explore"].contains(&kind)
        || (topic != "charts" && topic_query(topic).is_none() && genre_id(topic).is_none())
    {
        return Err(LibraryError::Validation);
    }
    let music = yt.music();
    let step = Duration::from_secs(12);
    // "Picked for you": YouTube Music's radio from the last song you played or liked.
    let personal = || async {
        let title = personalized_title(kind)?;
        let seed = seed.filter(|id| super::playback::extract::valid_id(id))?;
        let radio = tokio::time::timeout(step, music.radio(seed))
            .await
            .ok()?
            .ok()?;
        let list = tracks(radio.queue.into_iter().filter(|t| t.id != seed).collect());
        (!list.is_empty()).then(|| section(title, list))
    };
    // Home: the radio wins when there is one.
    if kind == "home"
        && let Some(radio) = personal().await
    {
        return Ok(vec![radio]);
    }
    if let Some(id) = genre_id(topic) {
        // A genre or mood: the first playlist of its first few sections, as track lists.
        let genre = tokio::time::timeout(step, music.genre_playlists(id))
            .await
            .map_err(|_| LibraryError::Unavailable)?
            .map_err(|_| LibraryError::Unavailable)?;
        let mut out = Vec::new();
        for part in genre
            .into_iter()
            .filter(|p| p.items.iter().any(|i| matches!(i, Item::Playlist(_))))
            .take(3)
        {
            let Some(first) = part.items.iter().find_map(|i| match i {
                Item::Playlist(p) => Some(p.id.clone()),
                _ => None,
            }) else {
                continue;
            };
            let Ok(Ok(playlist)) = tokio::time::timeout(step, music.playlist(&first)).await else {
                continue;
            };
            let list = tracks(playlist.tracks.items);
            if !list.is_empty() {
                out.push(section(part.title.as_deref().unwrap_or("Playlists"), list));
            }
        }
        return if out.is_empty() {
            Err(LibraryError::Unavailable)
        } else {
            Ok(out)
        };
    }
    if topic == "charts" {
        let (charts, albums, videos, radio) = tokio::join!(
            tokio::time::timeout(step, music.charts(None)),
            tokio::time::timeout(step, music.new_albums()),
            tokio::time::timeout(step, music.new_videos()),
            personal(),
        );
        let mut sections: Vec<FeedSection> = radio.into_iter().collect();
        if let Ok(Ok(charts)) = charts {
            // The first two shelves of songs, under the service's own (localised) titles.
            for (position, shelf) in charts
                .into_iter()
                .filter(|s| s.items.iter().any(|i| matches!(i, Item::Track(_))))
                .take(2)
                .enumerate()
            {
                let title = shelf.title.clone().unwrap_or_else(|| {
                    ["Top music videos", "Trending on YouTube Music"][position].into()
                });
                let list = tracks(section_tracks(shelf));
                if !list.is_empty() {
                    sections.push(section(&title, list));
                }
            }
        }
        if let Ok(Ok(albums)) = albums {
            let albums: Vec<FeedAlbum> = albums
                .into_iter()
                .filter(|a| super::library::album_id(&ProviderId::YouTube, &a.id))
                .take(12)
                .map(|a| FeedAlbum {
                    id: a.id,
                    name: a.title,
                    year: a.year,
                    artists: a.artists.into_iter().map(|x| x.name).collect(),
                    artwork_url: a
                        .thumbnails
                        .into_iter()
                        .max_by_key(|c| c.width)
                        .and_then(|c| super::playback::track_info::artwork_url(&c.url)),
                })
                .collect();
            if !albums.is_empty() {
                sections.push(FeedSection {
                    title: "New albums and singles".into(),
                    tracks: Vec::new(),
                    albums,
                });
            }
        }
        if let Ok(Ok(videos)) = videos {
            let list = tracks(videos);
            if !list.is_empty() {
                sections.push(section("New music videos", list));
            }
        }
        if !sections.is_empty() {
            return Ok(sections);
        }
    }
    let (title, query) = if topic == "charts" {
        ("Discover music", "music")
    } else {
        topic_query(topic).ok_or(LibraryError::Validation)?
    };
    let result = tokio::time::timeout(Duration::from_secs(12), music.search_tracks(query))
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)?;
    let list = tracks(result.items);
    if list.is_empty() {
        return Err(LibraryError::Unavailable);
    }
    Ok(vec![section(title, list)])
}
/// The moods and genres on YouTube Music's Explore page.
pub async fn genres(yt: &Yt) -> Result<Vec<Genre>, LibraryError> {
    let groups = tokio::time::timeout(Duration::from_secs(12), yt.music().genres())
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)?;
    Ok(groups
        .into_iter()
        .flat_map(|group| {
            // The overview groups its buttons under "Moods & moments" and "Genres".
            let mood = group
                .title
                .as_deref()
                .is_some_and(|t| t.to_lowercase().contains("mood"));
            group.genres.into_iter().map(move |g| (g, mood))
        })
        .filter(|(g, _)| {
            genre_id(&format!("genre:{}", g.params)).is_some() && !g.name.trim().is_empty()
        })
        .take(60)
        .map(|(g, mood)| Genre {
            id: g.params,
            name: g.name,
            mood,
        })
        .collect())
}
pub async fn suggestions(yt: &Yt, query: &str) -> Result<Vec<String>, LibraryError> {
    let query = query.trim();
    if query.chars().count() < 2 || query.len() > 256 {
        return Err(LibraryError::Validation);
    }
    let values = tokio::time::timeout(Duration::from_secs(4), yt.music().search_suggestions(query))
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)?;
    let mut seen = std::collections::HashSet::new();
    Ok(values
        .into_iter()
        .filter(|s| !s.trim().is_empty() && s.len() <= 256 && seen.insert(s.clone()))
        .take(6)
        .collect())
}
#[cfg(test)]
mod tests {
    use super::*;
    use innerlight::music::{ArtistRef as MusicArtist, TrackKind};
    use innerlight::{Client, FixtureTransport};
    use std::sync::Arc;

    fn item(id: &str) -> Track {
        Track {
            id: id.into(),
            title: "Real fixture".into(),
            artists: vec![MusicArtist {
                id: None,
                name: "Artist".into(),
            }],
            album: None,
            duration_ms: Some(180_000),
            thumbnails: vec![],
            is_explicit: false,
            kind: TrackKind::Audio,
        }
    }
    fn yt_with(transport: FixtureTransport) -> Yt {
        Yt::from_client(
            Client::builder()
                .transport(Arc::new(transport))
                .build()
                .unwrap(),
        )
    }
    #[test]
    fn feed_tracks_preserve_identity_and_deduplicate() {
        let found = tracks(vec![
            item("r-G2pKr2_n8"),
            item("r-G2pKr2_n8"),
            item("invalid"),
        ]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].provider_track_id, "r-G2pKr2_n8");
        assert_eq!(found[0].duration_ms, Some(180000));
    }
    #[test]
    fn topics_are_bounded_provider_queries() {
        assert!(topic_query("thai").is_some());
        assert!(topic_query("anything untrusted").is_none());
    }
    #[test]
    fn genre_ids_are_opaque_but_bounded() {
        assert!(genre_id("genre:ggMPOg1uX1ZaRGhUa29OSlhr").is_some());
        assert!(genre_id("genre:abc%3D").is_some());
        for bad in ["genre:", "genre:a b", "genre:a/b", "pop", "genre:a\"b"] {
            assert!(genre_id(bad).is_none(), "{bad}");
        }
        assert!(genre_id(&format!("genre:{}", "a".repeat(97))).is_none());
    }
    #[test]
    fn explore_uses_personalized_radio_when_listening_seed_exists() {
        assert_eq!(personalized_title("explore"), Some("Picked for you"));
        assert_eq!(personalized_title("home"), Some("More from your listening"));
        assert_eq!(personalized_title("other"), None);
    }
    #[tokio::test]
    async fn empty_or_malformed_answers_end_as_unavailable_without_panicking() {
        for body in [
            serde_json::json!({}),
            serde_json::json!([]),
            serde_json::json!({"contents": {"tabbedSearchResultsRenderer": 5}}),
            serde_json::json!({"continuationContents": null}),
        ] {
            let yt = yt_with(
                FixtureTransport::new()
                    .with("browse", body.clone())
                    .with("search", body.clone())
                    .with("next", body.clone())
                    .with("music/get_search_suggestions", body),
            );
            for topic in ["charts", "pop", "genre:ggMPOg1uX1ZaRGhUa29OSlhr"] {
                for kind in ["home", "explore"] {
                    assert!(
                        matches!(
                            feed(&yt, kind, topic, Some("r-G2pKr2_n8")).await,
                            Err(LibraryError::Unavailable)
                        ),
                        "{kind} {topic}"
                    );
                }
            }
            assert!(genres(&yt).await.is_err());
            assert!(
                suggestions(&yt, "ab")
                    .await
                    .map_or(true, |list| list.is_empty())
            );
        }
        let yt = yt_with(FixtureTransport::new());
        assert!(matches!(
            feed(&yt, "nope", "charts", None).await,
            Err(LibraryError::Validation)
        ));
        assert!(matches!(
            suggestions(&yt, "x").await,
            Err(LibraryError::Validation)
        ));
    }
    #[tokio::test]
    async fn genres_split_moods_from_genres_and_drop_unusable_params() {
        let button = |name: &str, params: &str| {
            serde_json::json!({"musicNavigationButtonRenderer": {
                "buttonText": {"runs": [{"text": name}]},
                "clickCommand": {"browseEndpoint": {
                    "browseId": "FEmusic_moods_and_genres_category", "params": params}}}})
        };
        let section = |title: &str, buttons: Vec<serde_json::Value>| {
            serde_json::json!({"gridRenderer": {
                "header": {"gridHeaderRenderer": {"title": {"runs": [{"text": title}]}}},
                "items": buttons}})
        };
        let body = serde_json::json!({"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
            section("Moods & moments", vec![button("Chill", "ggMPOg1uX1ZaRGhUa29OSlhr"), button("Bad", "has space")]),
            section("Genres", vec![button("Pop", "ggMPOg1uX1BvcA")]),
        ]}}}}]}}});
        let yt = yt_with(FixtureTransport::new().with("browse", body));
        let list = genres(&yt).await.unwrap();
        let summary: Vec<_> = list.iter().map(|g| (g.name.as_str(), g.mood)).collect();
        assert_eq!(summary, [("Chill", true), ("Pop", false)]);
    }
}
