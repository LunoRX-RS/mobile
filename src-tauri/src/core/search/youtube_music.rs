//! Unified YouTube Music and ordinary YouTube search using the desktop session.
use super::{
    hit::{SearchHit, SearchSource},
    provider::{SearchFuture, SearchProvider},
};
use crate::core::{domain::ProviderId, errors::CoreError, playback::extract::valid_id, yt::Yt};

pub struct YouTubeMusicSearch {
    yt: Yt,
}
impl YouTubeMusicSearch {
    pub fn new(yt: Yt) -> Self {
        Self { yt }
    }
}
impl SearchProvider for YouTubeMusicSearch {
    fn id(&self) -> ProviderId {
        ProviderId::YouTube
    }
    fn search<'a>(&'a self, query: &'a str, limit: u8) -> SearchFuture<'a> {
        Box::pin(async move {
            let (music_api, youtube_api) = (self.yt.music(), self.yt.youtube());
            let (music, videos) = tokio::join!(
                tokio::time::timeout(
                    std::time::Duration::from_secs(14),
                    music_api.search_tracks(query)
                ),
                tokio::time::timeout(
                    std::time::Duration::from_secs(14),
                    youtube_api.search_videos(query, None)
                ),
            );
            let music = music.ok().and_then(Result::ok);
            let videos = videos.ok().and_then(Result::ok);
            if music.is_none() && videos.is_none() {
                return Err(CoreError::ProviderUnavailable);
            }
            let mut music_hits = Vec::new();
            if let Some(result) = music {
                for track in result
                    .items
                    .into_iter()
                    .take(usize::from(limit.clamp(1, 10)))
                {
                    if !valid_id(&track.id) {
                        continue;
                    }
                    let artist_refs = track
                        .artists
                        .iter()
                        .map(|a| super::hit::ArtistRef {
                            provider: ProviderId::YouTube,
                            id: a.id.clone(),
                            name: a.name.clone(),
                        })
                        .collect();
                    let mut hit = SearchHit::new(
                        ProviderId::YouTube,
                        track.id.clone(),
                        format!("https://www.youtube.com/watch?v={}", track.id),
                        track.title,
                        track
                            .artists
                            .into_iter()
                            .map(|artist| artist.name)
                            .collect(),
                    )?;
                    hit.source = Some(SearchSource::YouTubeMusic);
                    hit.artist_refs = artist_refs;
                    hit.duration_ms = track.duration_ms;
                    hit.artwork_url = track
                        .thumbnails
                        .into_iter()
                        .max_by_key(|cover| cover.width)
                        .and_then(|cover| {
                            crate::core::playback::track_info::artwork_url(&cover.url)
                        });
                    music_hits.push(hit);
                }
            }
            let mut video_hits = Vec::new();
            if let Some(result) = videos {
                for video in result
                    .items
                    .into_iter()
                    // Live streams, Shorts and premieres that have not started (no length) are not songs to play.
                    .filter(|v| {
                        valid_id(&v.id) && !v.is_live && !v.is_short && v.duration_ms.is_some()
                    })
                    .take(usize::from(limit.clamp(1, 10)))
                {
                    let artists = video
                        .channel
                        .as_ref()
                        .map(|c| vec![c.name.clone()])
                        .unwrap_or_default();
                    let mut hit = SearchHit::new(
                        ProviderId::YouTube,
                        video.id.clone(),
                        format!("https://www.youtube.com/watch?v={}", video.id),
                        video.title,
                        artists,
                    )?;
                    hit.source = Some(SearchSource::YouTube);
                    hit.artist_refs = video
                        .channel
                        .into_iter()
                        .map(|c| super::hit::ArtistRef {
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
                        .and_then(|c| crate::core::playback::track_info::artwork_url(&c.url));
                    video_hits.push(hit);
                }
            }
            Ok(combine(music_hits, video_hits, limit))
        })
    }
}

fn combine(music: Vec<SearchHit>, videos: Vec<SearchHit>, limit: u8) -> Vec<SearchHit> {
    let mut music = music.into_iter();
    let mut videos = videos.into_iter();
    let mut seen = std::collections::HashSet::new();
    let mut hits = Vec::new();
    loop {
        let pair = [music.next(), videos.next()];
        if pair.iter().all(Option::is_none) {
            break;
        }
        for hit in pair.into_iter().flatten() {
            if seen.insert(hit.provider_track_id().to_owned()) {
                hits.push(hit);
            }
            if hits.len() >= usize::from(limit.clamp(1, 10)) {
                return hits;
            }
        }
    }
    hits
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hit(id: &str, source: SearchSource) -> SearchHit {
        let mut hit = SearchHit::new(
            ProviderId::YouTube,
            id.into(),
            format!("https://www.youtube.com/watch?v={id}"),
            "Same title".into(),
            vec![],
        )
        .unwrap();
        hit.source = Some(source);
        hit
    }
    #[test]
    fn combines_sources_by_rank_and_deduplicates_only_video_identity() {
        let hits = combine(
            vec![
                hit("r-G2pKr2_n8", SearchSource::YouTubeMusic),
                hit("aqz-KE-bpKQ", SearchSource::YouTubeMusic),
            ],
            vec![
                hit("r-G2pKr2_n8", SearchSource::YouTube),
                hit("abcdefghijk", SearchSource::YouTube),
            ],
            10,
        );
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].source(), Some(SearchSource::YouTubeMusic));
        assert_eq!(hits[2].source(), Some(SearchSource::YouTube));
    }
    #[test]
    fn either_source_can_supply_results_alone_and_limit_is_bounded() {
        assert_eq!(
            combine(vec![], vec![hit("abcdefghijk", SearchSource::YouTube)], 5).len(),
            1
        );
        assert_eq!(
            combine(
                vec![hit("r-G2pKr2_n8", SearchSource::YouTubeMusic)],
                vec![],
                5
            )
            .len(),
            1
        );
        assert_eq!(
            combine(
                vec![hit("r-G2pKr2_n8", SearchSource::YouTubeMusic)],
                vec![hit("abcdefghijk", SearchSource::YouTube)],
                1
            )
            .len(),
            1
        );
    }

    fn provider(transport: innerlight::FixtureTransport) -> YouTubeMusicSearch {
        let client = innerlight::Client::builder()
            .transport(std::sync::Arc::new(transport))
            .build()
            .unwrap();
        YouTubeMusicSearch::new(Yt::from_client(client))
    }
    fn video(id: &str, extra: serde_json::Value) -> serde_json::Value {
        let mut renderer = serde_json::json!({
            "videoId": id, "title": {"simpleText": "Video"},
            "ownerText": {"runs": [{"text": "Channel"}]},
            "lengthText": {"simpleText": "3:21"},
            "thumbnail": {"thumbnails": [{"url": "https://i.ytimg.com/vi/x/hq.jpg", "width": 360, "height": 202}]}
        });
        renderer
            .as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        serde_json::json!({"videoRenderer": renderer})
    }
    #[tokio::test]
    async fn video_search_alone_supplies_hits_and_skips_live_shorts_and_premieres() {
        let page = serde_json::json!({"contents": [
            video("abcdefghijk", serde_json::json!({})),
            video("kJQP7kiw5Fk", serde_json::json!({"badges": [{"metadataBadgeRenderer": {"style": "BADGE_STYLE_TYPE_LIVE_NOW"}}], "lengthText": null})),
            video("aqz-KE-bpKQ", serde_json::json!({"lengthText": null})),
            video("r-G2pKr2_n8", serde_json::json!({"navigationEndpoint": {"reelWatchEndpoint": {"videoId": "r-G2pKr2_n8"}}})),
        ]});
        // Music search answers with nothing readable; the video half still delivers.
        let hits = provider(
            innerlight::FixtureTransport::new()
                .with_match("search", "EgIQAQ", page)
                .with("search", serde_json::json!({"unexpected": true})),
        )
        .search("lofi", 5)
        .await;
        let hits = hits.unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].source(), Some(SearchSource::YouTube));
        assert_eq!(hits[0].provider_track_id(), "abcdefghijk");
    }
    #[tokio::test]
    async fn unreadable_answers_from_both_sources_are_unavailable_not_a_panic() {
        for body in [
            serde_json::json!({}),
            serde_json::json!(null),
            serde_json::json!({"contents": 7}),
        ] {
            let result = provider(innerlight::FixtureTransport::new().with("search", body))
                .search("lofi", 5)
                .await;
            // Parse failures on the music half, an empty list on the video half: no hits, no panic.
            assert!(result.map_or(true, |hits| hits.is_empty()));
        }
        let down = provider(innerlight::FixtureTransport::new().with_raw("search", 503, b"{}"));
        assert!(matches!(
            down.search("lofi", 5).await,
            Err(CoreError::ProviderUnavailable)
        ));
    }
}
