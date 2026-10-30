//! Podcast browsing on YouTube. YouTube has no podcast listing in the library `innerlight` wraps, and its music search
//! skips episodes on purpose, so a show is found the way a person finds one: a video search for the name, narrowed to
//! uploads long enough to be an episode. Every result is an ordinary YouTube video and plays through the same player.
use crate::core::{
    domain::ProviderId,
    errors::CoreError,
    playback::{extract::valid_id, track_info::artwork_url},
    search::hit::{ArtistRef, SearchHit, SearchSource},
    yt::Yt,
};
use innerlight::youtube::Video;
use std::time::Duration;

/// Shorter uploads are clips, trailers and songs, not episodes.
pub const MIN_EPISODE_MS: u64 = 20 * 60 * 1000;
const MAX_QUERY_CHARS: usize = 120;
const MAX_TOKEN_LEN: usize = 4096;
const TIMEOUT: Duration = Duration::from_secs(14);

/// One page of episodes and the handle for the next page, when YouTube has one.
pub struct EpisodePage {
    pub hits: Vec<SearchHit>,
    pub continuation: Option<String>,
}

/// The text sent to YouTube. "podcast" is added so a show with a common name (or a topic such as "true crime") lists
/// its episodes first; a query that already says so is left alone.
pub fn episode_query(raw: &str) -> Result<String, CoreError> {
    let text = raw.trim();
    if text.is_empty()
        || text.chars().count() > MAX_QUERY_CHARS
        || text.chars().any(char::is_control)
    {
        return Err(CoreError::Validation("podcast query".into()));
    }
    let lower = text.to_lowercase();
    if lower.contains("podcast") || lower.contains("พอดแคสต์") || lower.contains("พอดคาสต์")
    {
        Ok(text.to_owned())
    } else {
        Ok(format!("{text} podcast"))
    }
}

/// Keeps what can be played as an episode: a normal upload with a known length of at least [`MIN_EPISODE_MS`].
pub fn episodes(videos: Vec<Video>) -> Vec<SearchHit> {
    videos
        .into_iter()
        .filter(|v| {
            valid_id(&v.id)
                && !v.is_live
                && !v.is_short
                && v.duration_ms.is_some_and(|ms| ms >= MIN_EPISODE_MS)
        })
        .filter_map(|video| {
            let show = video.channel.as_ref().map(|c| c.name.clone());
            let mut hit = SearchHit::new(
                ProviderId::YouTube,
                video.id.clone(),
                format!("https://www.youtube.com/watch?v={}", video.id),
                video.title,
                show.into_iter().collect(),
            )
            .ok()?;
            hit.source = Some(SearchSource::YouTube);
            hit.artist_refs = video
                .channel
                .into_iter()
                .map(|c| ArtistRef {
                    provider: ProviderId::YouTube,
                    id: c.id,
                    name: c.name,
                })
                .collect();
            hit.duration_ms = video.duration_ms;
            hit.artwork_url = video
                .thumbnails
                .into_iter()
                .max_by_key(|c| c.width)
                .and_then(|c| artwork_url(&c.url));
            Some(hit)
        })
        .collect()
}

/// A continuation handle is opaque; only its size is checked here, the library checks the rest.
pub fn valid_continuation(token: &str) -> bool {
    (1..=MAX_TOKEN_LEN).contains(&token.len()) && !token.chars().any(char::is_control)
}

/// The first page for `query`, or the page after `continuation`. A page can hold no episode at all (a run of short
/// clips); the caller then asks for the next page, which `continuation` makes possible.
pub async fn browse(
    yt: &Yt,
    query: &str,
    continuation: Option<&str>,
) -> Result<EpisodePage, CoreError> {
    let text = episode_query(query)?;
    if continuation.is_some_and(|t| !valid_continuation(t)) {
        return Err(CoreError::Validation("podcast continuation".into()));
    }
    let page = tokio::time::timeout(TIMEOUT, yt.youtube().search_videos(&text, continuation))
        .await
        .map_err(|_| CoreError::ProviderUnavailable)?
        .map_err(|_| CoreError::ProviderUnavailable)?;
    Ok(EpisodePage {
        hits: episodes(page.items),
        continuation: page.continuation,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use innerlight::youtube::ChannelRef;

    fn video(id: &str, minutes: Option<u64>) -> Video {
        Video {
            id: id.into(),
            title: format!("Episode {id}"),
            channel: Some(ChannelRef {
                id: Some("UC1234567890123456789012".into()),
                name: "The Show".into(),
                verification: Default::default(),
            }),
            duration_ms: minutes.map(|m| m * 60_000),
            thumbnails: vec![],
            is_live: false,
            is_short: false,
            set_video_id: None,
        }
    }

    #[test]
    fn only_long_normal_uploads_are_episodes() {
        let mut live = video("liveliveliv", Some(90));
        live.is_live = true;
        let mut short = video("shortshorts", Some(90));
        short.is_short = true;
        let hits = episodes(vec![
            video("aaaaaaaaaaa", Some(45)),
            video("bbbbbbbbbbb", Some(19)),
            video("ccccccccccc", None),
            video("not-an-id", Some(60)),
            live,
            short,
            video("ddddddddddd", Some(20)),
        ]);
        let ids: Vec<_> = hits.iter().map(|h| h.provider_track_id()).collect();
        assert_eq!(ids, ["aaaaaaaaaaa", "ddddddddddd"]);
        assert_eq!(hits[0].artists(), ["The Show".to_owned()]);
        assert_eq!(hits[0].duration_ms(), Some(45 * 60_000));
        assert_eq!(hits[0].source(), Some(SearchSource::YouTube));
    }

    #[test]
    fn a_query_gets_podcast_once() {
        assert_eq!(episode_query(" 9arm ").unwrap(), "9arm podcast");
        assert_eq!(
            episode_query("The Ghost Radio Podcast").unwrap(),
            "The Ghost Radio Podcast"
        );
        assert_eq!(episode_query("พอดแคสต์ ธุรกิจ").unwrap(), "พอดแคสต์ ธุรกิจ");
    }

    #[test]
    fn empty_long_or_control_queries_are_refused() {
        assert!(episode_query("   ").is_err());
        assert!(episode_query(&"a".repeat(MAX_QUERY_CHARS + 1)).is_err());
        assert!(episode_query("a\nb").is_err());
        assert!(episode_query(&"ก".repeat(MAX_QUERY_CHARS)).is_ok());
    }

    #[test]
    fn continuation_tokens_are_bounded() {
        assert!(valid_continuation("EpoDEg5"));
        assert!(!valid_continuation(""));
        assert!(!valid_continuation(&"x".repeat(MAX_TOKEN_LEN + 1)));
        assert!(!valid_continuation("a\0b"));
    }
}
