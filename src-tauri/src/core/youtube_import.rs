//! Reads the signed-in user's YouTube Music and YouTube collections: liked songs and videos, saved playlists,
//! Watch later and recent history.
//! Reading never changes the account. The one write is `rate`, a like or un-like the user did here, sent only when they turned that on.
//! Each section fails on its own, so a partial import still lands.
use super::{
    library::{LibraryError, SavedTrack, saved_track},
    yt::{Yt, extend},
};
use std::{collections::HashMap, time::Duration};

const LIKED_MAX: usize = 1000;
const HISTORY_MAX: usize = 50;
const PLAYLISTS_MAX: usize = 30;
const PLAYLIST_TRACKS_MAX: usize = 300;
const STEP_TIMEOUT: Duration = Duration::from_secs(45);

/// Where an imported playlist came from. It decides the marker in the local playlist's description.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Music,
    YouTube,
}

pub struct ImportedPlaylist {
    pub id: String,
    pub name: String,
    pub origin: Origin,
    pub tracks: Vec<SavedTrack>,
}

/// A YouTube video as a saved track. Livestreams and Shorts are not music to play, so they are left out.
pub fn video_track(v: innerlight::youtube::Video) -> Option<SavedTrack> {
    // A row with neither channel nor length is a deleted or private video: only its placeholder title is left.
    if v.is_live
        || v.is_short
        || !super::playback::extract::valid_id(&v.id)
        || (v.channel.is_none() && v.duration_ms.is_none())
    {
        return None;
    }
    let (artists, artist_refs) = match v.channel {
        Some(c) if !c.name.trim().is_empty() => (
            vec![c.name.clone()],
            vec![super::search::hit::ArtistRef {
                provider: super::domain::ProviderId::YouTube,
                id: c.id.filter(|id| {
                    super::library::artist_id(&super::domain::ProviderId::YouTube, id)
                }),
                name: c.name,
            }],
        ),
        _ => (Vec::new(), Vec::new()),
    };
    let mut track = SavedTrack {
        source: Some(super::search::hit::SearchSource::YouTube),
        provider: super::domain::ProviderId::YouTube,
        provider_track_id: v.id.clone(),
        canonical_url: format!("https://www.youtube.com/watch?v={}", v.id),
        title: v.title,
        artists,
        artist_refs,
        artwork_url: v
            .thumbnails
            .into_iter()
            .max_by_key(|t| t.width)
            .and_then(|t| super::playback::track_info::artwork_url(&t.url)),
        duration_ms: v.duration_ms,
    };
    track.validate().ok().map(|()| track)
}

#[derive(Default)]
pub struct Import {
    pub liked: Vec<SavedTrack>,
    /// How the liked-songs read went, for the diagnostic file. Counts only.
    pub liked_stats: LikedStats,
    pub history: Vec<SavedTrack>,
    pub playlists: Vec<ImportedPlaylist>,
    /// True when every liked song was read (the list ended before the cap), so a missing one was really unliked.
    pub liked_complete: bool,
    /// Track counts of every saved playlist seen, for the next quick pass.
    pub counts: HashMap<String, u64>,
    /// Sections that could not be read, by name.
    pub failed: Vec<&'static str>,
}

async fn step<T>(
    fut: impl std::future::Future<Output = Result<T, innerlight::Error>>,
) -> Result<T, LibraryError> {
    tokio::time::timeout(STEP_TIMEOUT, fut)
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)
}

/// Likes or un-likes one video on the signed-in account (`like/like`, `like/removelike`), as the YouTube site does.
/// Needs the session cookie. `Err` means nothing was changed.
pub async fn rate(yt: &Yt, video_id: &str, like: bool) -> Result<(), LibraryError> {
    if !super::playback::extract::valid_id(video_id) {
        return Err(LibraryError::Validation);
    }
    let account = yt.account();
    if like {
        step(account.like_video(video_id)).await
    } else {
        step(account.unlike_video(video_id)).await
    }
}

/// The number in a count line such as "25 songs" or "1,204 videos".
fn count_of(text: Option<&str>) -> Option<u64> {
    let digits: String = text?.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok().filter(|_| digits.len() <= 15)
}

/// Counts for one liked-songs read. `stop` says why paging ended: `end` (no next page), `limit`, `repeat` (the same
/// token came back), `empty_page` (a page with no track rows but a token).
#[derive(Clone, Debug, Default)]
pub struct LikedStats {
    pub pages: u32,
    pub rows: u64,
    pub no_id: u64,
    pub other: u64,
    pub tracks: u64,
    pub kept: u64,
    pub stop: &'static str,
    /// The count YouTube printed in the first page's header, when there was one.
    pub header_count: Option<u64>,
    /// Names of continuation-looking keys seen across the pages (no values).
    pub continuation_keys: Vec<String>,
}

async fn liked(yt: &Yt) -> Result<(Vec<SavedTrack>, bool, LikedStats), LibraryError> {
    let music = yt.music();
    let (first, tally, probe) = step(music.saved_tracks_tally()).await?;
    let mut stats = LikedStats {
        pages: 1,
        rows: tally.rows as u64,
        no_id: tally.no_id as u64,
        other: tally.other as u64,
        tracks: tally.tracks as u64,
        header_count: probe.header_count,
        continuation_keys: probe.continuation_keys,
        ..LikedStats::default()
    };
    let mut items = first.items;
    let mut next = first.continuation;
    let mut last: Option<String> = None;
    // Same rules as `extend`, plus the reason it stopped.
    stats.stop = loop {
        if items.len() >= LIKED_MAX {
            break "limit";
        }
        let Some(token) = next.take() else {
            break "end";
        };
        if last.as_ref() == Some(&token) {
            break "repeat";
        }
        let (page, tally, probe) = step(music.saved_tracks_more_tally(&token)).await?;
        for key in probe.continuation_keys {
            if stats.continuation_keys.len() < 40 && !stats.continuation_keys.contains(&key) {
                stats.continuation_keys.push(key);
            }
        }
        stats.pages += 1;
        stats.rows += tally.rows as u64;
        stats.no_id += tally.no_id as u64;
        stats.other += tally.other as u64;
        stats.tracks += tally.tracks as u64;
        if page.items.is_empty() {
            break "empty_page";
        }
        last = Some(token);
        items.extend(page.items);
        next = page.continuation;
    };
    let complete = items.len() < LIKED_MAX;
    let kept: Vec<SavedTrack> = items
        .into_iter()
        .take(LIKED_MAX)
        .filter_map(|t| saved_track(t, None))
        .collect();
    stats.kept = kept.len() as u64;
    Ok((kept, complete, stats))
}

async fn history(yt: &Yt) -> Result<Vec<SavedTrack>, LibraryError> {
    let page = step(yt.music().history()).await?;
    Ok(page
        .items
        .into_iter()
        .take(HISTORY_MAX)
        .filter_map(|t| saved_track(t, None))
        .collect())
}

async fn playlists(
    yt: &Yt,
    known: &HashMap<String, u64>,
    counts: &mut HashMap<String, u64>,
) -> Result<Vec<ImportedPlaylist>, LibraryError> {
    let music = yt.music();
    let first = step(music.saved_playlists()).await?;
    let mut saved = first.items;
    step(extend(
        &mut saved,
        first.continuation,
        PLAYLISTS_MAX,
        |t| async move {
            let page = music.saved_playlists_more(&t).await?;
            Ok((page.items, page.continuation))
        },
    ))
    .await?;
    let mut out = Vec::new();
    // "LM" is the liked-songs list, which the likes section already brings over.
    for item in saved
        .into_iter()
        .filter(|p| p.id != "LM")
        .take(PLAYLISTS_MAX)
    {
        if let Some(n) = count_of(item.count_text.as_deref()) {
            counts.insert(item.id.clone(), n);
            // A quick pass reads only playlists whose size changed since the last pass.
            if known.get(&item.id) == Some(&n) {
                continue;
            }
        }
        // One unreadable playlist must not drop the others.
        let Ok(playlist) = step(music.playlist(&item.id)).await else {
            continue;
        };
        let mut items = playlist.tracks.items;
        if step(extend(
            &mut items,
            playlist.tracks.continuation,
            PLAYLIST_TRACKS_MAX,
            |t| async move {
                let page = music.playlist_more(&t).await?;
                Ok((page.items, page.continuation))
            },
        ))
        .await
        .is_err()
        {
            continue;
        }
        let tracks: Vec<SavedTrack> = items
            .into_iter()
            .take(PLAYLIST_TRACKS_MAX)
            .filter_map(|t| saved_track(t, None))
            .collect();
        if !tracks.is_empty() {
            out.push(ImportedPlaylist {
                id: item.id,
                name: item.title,
                origin: Origin::Music,
                tracks,
            });
        }
    }
    Ok(out)
}

async fn youtube_liked(yt: &Yt) -> Result<(Vec<SavedTrack>, bool), LibraryError> {
    let youtube = yt.youtube();
    let first = step(youtube.liked_videos(None)).await?;
    let mut videos = first.items;
    step(extend(
        &mut videos,
        first.continuation,
        LIKED_MAX,
        |t| async move {
            let page = youtube.liked_videos(Some(&t)).await?;
            Ok((page.items, page.continuation))
        },
    ))
    .await?;
    let complete = videos.len() < LIKED_MAX;
    Ok((
        videos
            .into_iter()
            .take(LIKED_MAX)
            .filter_map(video_track)
            .collect(),
        complete,
    ))
}

async fn youtube_history(yt: &Yt) -> Result<Vec<SavedTrack>, LibraryError> {
    let page = step(yt.youtube().history(None)).await?;
    Ok(page
        .items
        .into_iter()
        .take(HISTORY_MAX)
        .filter_map(video_track)
        .collect())
}

async fn video_playlist(yt: &Yt, id: &str, watch_later: bool) -> Option<Vec<SavedTrack>> {
    let youtube = yt.youtube();
    let first = if watch_later {
        step(youtube.watch_later(None)).await.ok()?
    } else {
        step(youtube.playlist(id, None)).await.ok()?
    };
    let mut videos = first.items;
    step(extend(
        &mut videos,
        first.continuation,
        PLAYLIST_TRACKS_MAX,
        |t| async move {
            let page = if watch_later {
                youtube.watch_later(Some(&t)).await?
            } else {
                youtube.playlist(id, Some(&t)).await?
            };
            Ok((page.items, page.continuation))
        },
    ))
    .await
    .ok()?;
    let tracks: Vec<SavedTrack> = videos
        .into_iter()
        .take(PLAYLIST_TRACKS_MAX)
        .filter_map(video_track)
        .collect();
    (!tracks.is_empty()).then_some(tracks)
}

/// Saved YouTube playlists and Watch later. Playlists already read through YouTube Music (`skip`) are not read twice.
async fn youtube_playlists(
    yt: &Yt,
    known: &HashMap<String, u64>,
    counts: &mut HashMap<String, u64>,
    skip: &std::collections::HashSet<String>,
) -> Result<Vec<ImportedPlaylist>, LibraryError> {
    let youtube = yt.youtube();
    let first = step(youtube.saved_playlists(None)).await?;
    let mut saved = first.items;
    step(extend(
        &mut saved,
        first.continuation,
        PLAYLISTS_MAX,
        |t| async move {
            let page = youtube.saved_playlists(Some(&t)).await?;
            Ok((page.items, page.continuation))
        },
    ))
    .await?;
    let mut out = Vec::new();
    for item in saved
        .into_iter()
        .filter(|p| !skip.contains(&p.id))
        .take(PLAYLISTS_MAX)
    {
        if let Some(n) = item.video_count {
            counts.insert(item.id.clone(), n);
            if known.get(&item.id) == Some(&n) {
                continue;
            }
        }
        if let Some(tracks) = video_playlist(yt, &item.id, false).await {
            out.push(ImportedPlaylist {
                id: item.id,
                name: item.title,
                origin: Origin::YouTube,
                tracks,
            });
        }
    }
    if let Some(tracks) = video_playlist(yt, "WL", true).await {
        out.push(ImportedPlaylist {
            id: "WL".into(),
            name: "Watch later".into(),
            origin: Origin::YouTube,
            tracks,
        });
    }
    Ok(out)
}

/// `known` holds playlist sizes from the last pass; pass an empty map to read every playlist.
pub async fn fetch(yt: &Yt, known: &HashMap<String, u64>) -> Import {
    let mut import = Import::default();
    match liked(yt).await {
        Ok((v, complete, stats)) => {
            import.liked = v;
            import.liked_complete = complete;
            import.liked_stats = stats;
        }
        Err(_) => import.failed.push("likes"),
    }
    match youtube_liked(yt).await {
        Ok((videos, complete)) => {
            let seen: std::collections::HashSet<String> = import
                .liked
                .iter()
                .map(|t| t.provider_track_id.clone())
                .collect();
            import.liked.extend(
                videos
                    .into_iter()
                    .filter(|t| !seen.contains(&t.provider_track_id)),
            );
            import.liked_complete &= complete;
        }
        Err(_) => {
            import.failed.push("ytLikes");
            // A missing list cannot prove a song was un-liked.
            import.liked_complete = false;
        }
    }
    let mut counts = HashMap::new();
    let result = playlists(yt, known, &mut counts).await;
    match result {
        Ok(v) => import.playlists = v,
        Err(_) => import.failed.push("playlists"),
    }
    let music_ids: std::collections::HashSet<String> = counts.keys().cloned().collect();
    match youtube_playlists(yt, known, &mut counts, &music_ids).await {
        Ok(v) => import.playlists.extend(v),
        Err(_) => import.failed.push("ytPlaylists"),
    }
    import.counts = counts;
    match history(yt).await {
        Ok(v) => import.history = v,
        Err(_) => import.failed.push("history"),
    }
    match youtube_history(yt).await {
        Ok(v) => {
            let seen: std::collections::HashSet<String> = import
                .history
                .iter()
                .map(|t| t.provider_track_id.clone())
                .collect();
            import.history.extend(
                v.into_iter()
                    .filter(|t| !seen.contains(&t.provider_track_id)),
            );
        }
        Err(_) => import.failed.push("ytHistory"),
    }
    import
}

#[cfg(test)]
mod tests {
    use super::*;
    use innerlight::{Client, FixtureTransport};
    use serde_json::json;
    use std::sync::Arc;

    fn signed_in(transport: FixtureTransport) -> (Yt, Arc<FixtureTransport>) {
        let transport = Arc::new(transport);
        let client = Client::builder()
            .transport(transport.clone())
            .build()
            .unwrap();
        client.set_cookie("SAPISID=abc123; SID=x").unwrap();
        (Yt::from_client(client), transport)
    }

    #[tokio::test]
    async fn rate_refuses_a_bad_video_id_before_any_request() {
        let (yt, transport) = signed_in(FixtureTransport::new());
        for bad in ["", "short", "has space!!", "../../etc/pa"] {
            assert!(matches!(
                rate(&yt, bad, true).await,
                Err(LibraryError::Validation)
            ));
        }
        assert!(transport.requests().is_empty());
    }

    #[tokio::test]
    async fn rate_sends_like_and_removelike_signed_in_and_nothing_without_a_session() {
        let (yt, transport) = signed_in(
            FixtureTransport::new()
                .with("like/like", json!({"responseContext": {}}))
                .with("like/removelike", json!({"responseContext": {}})),
        );
        rate(&yt, "r-G2pKr2_n8", true).await.unwrap();
        rate(&yt, "aqz-KE-bpKQ", false).await.unwrap();
        let sent = transport.requests();
        assert_eq!(
            sent.iter()
                .map(|r| (
                    r.endpoint.as_str(),
                    r.body["target"]["videoId"].as_str().unwrap()
                ))
                .collect::<Vec<_>>(),
            [
                ("like/like", "r-G2pKr2_n8"),
                ("like/removelike", "aqz-KE-bpKQ")
            ]
        );
        assert!(sent[0].header("authorization").is_some());

        yt.clear_session();
        assert!(rate(&yt, "r-G2pKr2_n8", true).await.is_err());
        assert_eq!(transport.requests().len(), 2, "no request without a cookie");
    }

    #[test]
    fn count_lines_become_numbers() {
        assert_eq!(count_of(Some("25 songs")), Some(25));
        assert_eq!(count_of(Some("1,204 videos")), Some(1204));
        assert_eq!(count_of(Some("No videos")), None);
        assert_eq!(count_of(None), None);
    }

    #[test]
    fn deleted_private_live_and_short_rows_are_not_tracks() {
        use innerlight::youtube::{ChannelRef, Video};
        let video = |id: &str| Video {
            id: id.into(),
            title: "Title".into(),
            channel: Some(ChannelRef {
                id: Some("UCuAXFkgsw1L7xaCfnd5JJOw".into()),
                name: "Channel".into(),
                verification: Default::default(),
            }),
            duration_ms: Some(200_000),
            thumbnails: vec![],
            is_live: false,
            is_short: false,
            set_video_id: None,
        };
        let ok = video_track(video("r-G2pKr2_n8")).unwrap();
        assert_eq!(ok.artists, ["Channel"]);
        assert_eq!(ok.duration_ms, Some(200_000));
        assert!(
            video_track(Video {
                is_live: true,
                ..video("r-G2pKr2_n8")
            })
            .is_none()
        );
        assert!(
            video_track(Video {
                is_short: true,
                ..video("r-G2pKr2_n8")
            })
            .is_none()
        );
        assert!(video_track(video("bad id")).is_none());
        let deleted = Video {
            channel: None,
            duration_ms: None,
            title: "[Deleted video]".into(),
            ..video("r-G2pKr2_n8")
        };
        assert!(video_track(deleted).is_none());
    }

    #[tokio::test]
    async fn a_failed_section_is_reported_and_the_others_still_land() {
        // Every endpoint answers with something unreadable: each section fails on its own, nothing panics.
        let (yt, _) = signed_in(
            FixtureTransport::new()
                .with("browse", json!({"unexpected": [1, 2, 3]}))
                .with_raw("like/like", 500, b"{}"),
        );
        let import = fetch(&yt, &HashMap::new()).await;
        assert!(
            import.liked.is_empty() && import.history.is_empty() && import.playlists.is_empty()
        );
        assert!(!import.liked_complete);
        assert!(import.failed.contains(&"likes") && import.failed.contains(&"history"));
    }
}
