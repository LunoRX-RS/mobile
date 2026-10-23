//! Presentation metadata and real, untimed lyrics from YouTube Music.
use super::extract::{failed, valid_id};
use super::lyrics::{FallbackLyrics, LyricsError, TimedLine};
use crate::core::errors::PlaybackErrorKind;
use crate::core::yt::Yt;
use serde::Serialize;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackInfo {
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artist_refs: Vec<crate::core::search::hit::ArtistRef>,
    pub artwork_url: Option<String>,
    pub lyrics: Option<String>,
    pub lyrics_source: Option<String>,
    pub lyrics_provider: Option<&'static str>,
    pub lyrics_status: &'static str,
    /// Real LRCLIB timestamps, or null. Sent as null explicitly, never omitted.
    pub lyrics_lines: Option<Vec<TimedLine>>,
    pub lyrics_synced: bool,
    /// Every candidate source, always youtubeMusic then lrclib. The fields above keep the
    /// "auto" pick for older UI code.
    pub lyrics_sources: Vec<LyricsSource>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LyricsSource {
    pub id: &'static str,
    pub label: String,
    pub available: bool,
    pub text: Option<String>,
    pub lines: Option<Vec<TimedLine>>,
    pub synced: bool,
    /// True only when the lines carry real word timings.
    pub words: bool,
    /// True when the user chose this source's lyrics by hand ("Wrong song?").
    pub bound: bool,
    pub attribution: Option<String>,
    pub status: &'static str,
}

/// Both sources, unavailable. Used when no lookup ran (cached metadata only).
pub fn no_sources() -> Vec<LyricsSource> {
    let mut list = sources(None, "unavailable", &Ok(None));
    list[1].status = "unavailable";
    list
}

/// The LRCLIB entry alone, for "Try again" on that source.
pub fn lrclib_source(lookup: &Result<Option<FallbackLyrics>, LyricsError>) -> LyricsSource {
    let mut list = sources(None, "unavailable", lookup);
    list.swap_remove(1)
}

/// One entry per source, whatever the outcome. Lines are never mixed between entries.
fn sources(
    youtube: Option<&(String, String)>,
    youtube_status: &'static str,
    lookup: &Result<Option<FallbackLyrics>, LyricsError>,
) -> Vec<LyricsSource> {
    let yt = LyricsSource {
        id: "youtubeMusic",
        label: match youtube {
            Some((_, footer)) if footer.to_lowercase().contains("lyricfind") => {
                "YouTube Music (LyricFind)".into()
            }
            _ => "YouTube Music".into(),
        },
        available: youtube.is_some(),
        text: youtube.map(|(text, _)| text.clone()),
        lines: None,
        synced: false,
        words: false,
        bound: false,
        attribution: youtube.map(|(_, footer)| footer.clone()),
        status: if youtube.is_some() {
            "available"
        } else {
            youtube_status
        },
    };
    let lr = match lookup {
        Ok(Some(found)) if !found.instrumental && found.text.is_some() => LyricsSource {
            id: "lrclib",
            label: "LRCLIB".into(),
            available: true,
            text: found.text.clone(),
            lines: found.timed.clone(),
            synced: found.timed.is_some(),
            words: false,
            bound: false,
            attribution: Some(
                if found.timed.is_some() {
                    SYNCED_SOURCE
                } else {
                    PLAIN_SOURCE
                }
                .into(),
            ),
            // Text without timestamps because the record's length differs too much from this track.
            status: if found.timing_mismatch {
                "timingMismatch"
            } else {
                "available"
            },
        },
        other => LyricsSource {
            id: "lrclib",
            label: "LRCLIB".into(),
            available: false,
            text: None,
            lines: None,
            synced: false,
            words: false,
            bound: false,
            attribution: None,
            status: match other {
                Ok(Some(_)) => "instrumental",
                // Callers that did not search overwrite this with "unavailable" (see `no_sources`).
                Ok(None) => "noMatch",
                Err(_) => "failed",
            },
        },
    };
    vec![yt, lr]
}

const SYNCED_SOURCE: &str = "Lyrics from LRCLIB · synced";
const PLAIN_SOURCE: &str = "Lyrics from LRCLIB · https://lrclib.net";

struct Chosen {
    lyrics: Option<String>,
    source: Option<String>,
    provider: Option<&'static str>,
    status: &'static str,
    lines: Option<Vec<TimedLine>>,
}

/// Picks what to show. Timed LRCLIB lines win for display; YouTube Music plain text stays in
/// `lyrics` when it came first. Lines from different sources are never mixed.
fn select(
    youtube: Option<(String, String)>,
    youtube_status: &'static str,
    lookup: Result<Option<FallbackLyrics>, LyricsError>,
) -> Chosen {
    let timed = lookup
        .as_ref()
        .ok()
        .and_then(|found| found.as_ref())
        .and_then(|found| found.timed.clone());
    if let Some((text, footer)) = youtube {
        return match timed {
            Some(lines) => Chosen {
                lyrics: Some(text),
                source: Some(SYNCED_SOURCE.into()),
                provider: Some("LRCLIB"),
                status: "available",
                lines: Some(lines),
            },
            None => Chosen {
                lyrics: Some(text),
                source: Some(footer),
                provider: Some("YouTube Music"),
                status: "available",
                lines: None,
            },
        };
    }
    match lookup {
        Ok(Some(found)) => Chosen {
            status: if found.instrumental {
                "instrumental"
            } else {
                "available"
            },
            source: Some(
                if found.timed.is_some() {
                    SYNCED_SOURCE
                } else {
                    PLAIN_SOURCE
                }
                .into(),
            ),
            lyrics: found.text,
            provider: Some("LRCLIB"),
            lines: found.timed,
        },
        Ok(None) => Chosen {
            lyrics: None,
            source: None,
            provider: None,
            status: youtube_status,
            lines: None,
        },
        Err(_) => Chosen {
            lyrics: None,
            source: None,
            provider: None,
            status: "failed",
            lines: None,
        },
    }
}

pub fn artwork_url(raw: &str) -> Option<String> {
    let url = reqwest::Url::parse(raw).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https"
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && (host == "i.ytimg.com"
            || host == "yt3.ggpht.com"
            || host.ends_with(".googleusercontent.com")))
    .then(|| url.into())
}

pub async fn fetch(yt: &Yt, id: &str) -> Result<TrackInfo, PlaybackErrorKind> {
    fetch_cached(yt, id, None).await
}

pub async fn fetch_cached(
    yt: &Yt,
    id: &str,
    cache: Option<&std::path::Path>,
) -> Result<TrackInfo, PlaybackErrorKind> {
    fetch_with(yt, id, cache, &super::lyric_sources::Ctx::default()).await
}

/// Entry rank for the Auto pick: real word timings, then timestamps, then plain text.
fn tier(entry: &LyricsSource) -> u8 {
    match (
        entry.available && entry.text.is_some(),
        entry.words,
        entry.synced,
    ) {
        (false, ..) => 0,
        (true, true, _) => 3,
        (true, false, true) => 2,
        (true, false, false) => 1,
    }
}

/// Auto pick across every source: highest tier, ties by the user's order. Never mixes sources.
fn auto_pick<'a>(
    entries: &'a [LyricsSource],
    ctx: &super::lyric_sources::Ctx,
) -> Option<&'a LyricsSource> {
    entries
        .iter()
        .filter(|e| tier(e) > 0)
        .max_by_key(|e| (tier(e), std::cmp::Reverse(ctx.settings.rank(e.id))))
}

fn chosen_from(entry: &LyricsSource) -> Chosen {
    Chosen {
        lyrics: entry.text.clone(),
        source: entry.attribution.clone(),
        provider: super::lyric_prefs::SOURCES
            .iter()
            .find(|(id, _)| *id == entry.id)
            .map(|(_, label)| *label),
        status: "available",
        lines: entry.lines.clone(),
    }
}

struct Details {
    track: innerlight::music::Track,
    lyrics_id: Option<String>,
}

/// The track's own entry in its radio answer, plus the id of its lyrics page. The service lists the requested video
/// first; an answer without it (an unavailable video) is an error rather than another track's data.
async fn details(yt: &Yt, id: &str) -> Result<Details, innerlight::Error> {
    let radio = yt.music().radio(id).await?;
    let track = radio
        .queue
        .into_iter()
        .find(|t| t.id == id)
        .ok_or(innerlight::Error::Parse)?;
    Ok(Details {
        track,
        lyrics_id: radio.lyrics_browse_id,
    })
}

pub async fn fetch_with(
    yt: &Yt,
    id: &str,
    cache: Option<&std::path::Path>,
    ctx: &super::lyric_sources::Ctx,
) -> Result<TrackInfo, PlaybackErrorKind> {
    if !valid_id(id) {
        return Err(PlaybackErrorKind::Validation("invalid YouTube id"));
    }
    let details = tokio::time::timeout(std::time::Duration::from_secs(10), details(yt, id))
        .await
        .map_err(|_| failed())?
        .map_err(|_| failed())?;
    let artwork = details
        .track
        .thumbnails
        .iter()
        .max_by_key(|image| image.width)
        .and_then(|image| artwork_url(&image.url));
    let artist_refs = details
        .track
        .artists
        .iter()
        .map(|a| crate::core::search::hit::ArtistRef {
            provider: crate::core::domain::ProviderId::YouTube,
            id: a.id.clone(),
            name: a.name.clone(),
        })
        .collect();
    let artists: Vec<String> = details
        .track
        .artists
        .into_iter()
        .map(|artist| artist.name)
        .collect();
    let seconds = details
        .track
        .duration_ms
        .and_then(|ms| u32::try_from(ms / 1000).ok());
    // Both sources are best effort and run together, each with its own deadline; one failing or
    // timing out never fails the other or the call.
    let youtube_lookup = async {
        if ctx.settings.is_disabled("youtubeMusic") {
            return (None, "disabled");
        }
        let Some(lyrics_id) = details.lyrics_id.clone() else {
            return (None, "unavailable");
        };
        match tokio::time::timeout(
            std::time::Duration::from_secs(6),
            yt.music().lyrics(&lyrics_id),
        )
        .await
        {
            Ok(Ok(lyrics)) if !lyrics.text.trim().is_empty() => (
                Some((lyrics.text, lyrics.source.unwrap_or_default())),
                "available",
            ),
            Ok(Ok(_)) => (None, "unavailable"),
            _ => (None, "failed"),
        }
    };
    let lrclib_lookup = async {
        if ctx.settings.is_disabled("lrclib") {
            return Ok(None);
        }
        tokio::time::timeout(
            std::time::Duration::from_secs(9),
            super::lyrics::fallback_cached(
                &details.track.title,
                &artists,
                details
                    .track
                    .album
                    .as_ref()
                    .map(|album| album.name.as_str()),
                seconds,
                cache,
            ),
        )
        .await
        .unwrap_or(Err(LyricsError))
    };
    let query = super::lyric_sources::Query {
        title: details.track.title.clone(),
        artists: artists.clone(),
        album: details.track.album.as_ref().map(|album| album.name.clone()),
        duration_ms: seconds.unwrap_or(0).saturating_mul(1000),
    };
    let extra_lookup = super::lyric_sources::run_all(ctx, id, &query);
    let (((youtube, youtube_status), lookup), extra) = tokio::join!(
        async { tokio::join!(youtube_lookup, lrclib_lookup) },
        extra_lookup
    );
    let mut lyrics_sources = sources(youtube.as_ref(), youtube_status, &lookup);
    if ctx.settings.is_disabled("lrclib") {
        lyrics_sources[1].status = "disabled";
    } else if matches!(lookup, Ok(None)) && !super::lyrics::searchable(&artists, seconds) {
        lyrics_sources[1].status = "unavailable";
    }
    lyrics_sources.extend(
        extra
            .iter()
            .map(|(kind, outcome)| super::lyric_sources::entry(*kind, outcome)),
    );
    let picked = auto_pick(&lyrics_sources, ctx).map(|e| e.id);
    let chosen = match picked {
        Some("youtubeMusic" | "lrclib") | None => select(youtube, youtube_status, lookup),
        Some(_) => auto_pick(&lyrics_sources, ctx)
            .map_or_else(|| select(None, "unavailable", Ok(None)), chosen_from),
    };
    Ok(TrackInfo {
        provider_track_id: id.into(),
        title: details.track.title,
        artists,
        artist_refs,
        artwork_url: artwork,
        lyrics: chosen.lyrics,
        lyrics_source: chosen.source,
        lyrics_provider: chosen.provider,
        lyrics_status: chosen.status,
        lyrics_synced: chosen.lines.is_some(),
        lyrics_lines: chosen.lines,
        lyrics_sources,
    })
}

/// Cache file for one video id: hex of its bytes, so case and characters stay distinct.
pub fn cache_file(root: &std::path::Path, id: &str) -> std::path::PathBuf {
    root.join("lyrics").join(format!(
        "{}.json",
        id.bytes().map(|b| format!("{b:02x}")).collect::<String>()
    ))
}

static CACHE_ROOT: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();
/// The app cache directory, remembered so background prefetch writes where the UI call reads.
pub fn set_cache_root(root: std::path::PathBuf) {
    let _ = CACHE_ROOT.set(root);
}

/// Pure decision for the next-track lyrics prefetch: only public metadata, only when a lookup can
/// run, nothing is cached yet (positive or negative) and this track is not already being fetched.
pub fn should_prefetch(
    have_cache_dir: bool,
    title: &str,
    artists: &[String],
    duration_secs: Option<u32>,
    cached: bool,
    already_in_flight: bool,
) -> bool {
    have_cache_dir
        && !title.trim().is_empty()
        && super::lyrics::searchable(artists, duration_secs)
        && !cached
        && !already_in_flight
}

/// Best-effort background lookup for the next autoplay track. One at a time: a new track cancels
/// the previous prefetch. Never blocks the caller, never panics, errors (offline, timeout) are
/// silent and are not remembered as "no match".
pub fn prefetch_lyrics(id: &str, title: &str, artists: &[String], duration_ms: Option<i64>) {
    static IN_FLIGHT: std::sync::Mutex<Option<(String, tokio::task::AbortHandle)>> =
        std::sync::Mutex::new(None);
    let secs = duration_ms
        .filter(|ms| *ms > 0)
        .and_then(|ms| u32::try_from(ms / 1000).ok());
    let path = CACHE_ROOT.get().map(|root| cache_file(root, id));
    let Ok(runtime) = tokio::runtime::Handle::try_current() else {
        return;
    };
    let Ok(mut slot) = IN_FLIGHT.lock() else {
        return;
    };
    let same = slot
        .as_ref()
        .is_some_and(|(old, task)| old == id && !task.is_finished());
    let cached = path
        .as_deref()
        .zip(secs)
        .is_some_and(|(path, secs)| super::lyrics::cache_has_entry(path, title, artists, secs));
    if !should_prefetch(path.is_some(), title, artists, secs, cached, same) {
        return;
    }
    if let Some((_, old)) = slot.take() {
        old.abort();
    }
    let (title, artists) = (title.to_owned(), artists.to_vec());
    let task = runtime.spawn(async move {
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(9),
            super::lyrics::fallback_cached(&title, &artists, None, secs, path.as_deref()),
        )
        .await;
    });
    *slot = Some((id.to_owned(), task.abort_handle()));
}

pub fn cached_lrclib(id: &str, path: &std::path::Path) -> Option<TrackInfo> {
    let record = super::lyrics_cache::load(path)?;
    let title = record.track_name.clone();
    let artists = vec![record.artist_name.clone()];
    let lookup = Ok(Some(super::lyrics::usable(record)?));
    let lyrics_sources = sources(None, "failed", &lookup);
    let chosen = select(None, "failed", lookup);
    let synced = chosen.lines.is_some();
    Some(TrackInfo {
        provider_track_id: id.into(),
        title,
        artists,
        artist_refs: vec![],
        artwork_url: None,
        lyrics: chosen.lyrics,
        lyrics_source: chosen.source,
        lyrics_provider: chosen.provider,
        lyrics_status: chosen.status,
        lyrics_lines: chosen.lines,
        lyrics_synced: synced,
        lyrics_sources,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn offline_cache_restores_lrclib_timestamps_and_reports_youtube_failure() {
        let root =
            std::env::temp_dir().join(format!("lunor-lyric-info-{:032x}", rand::random::<u128>()));
        let path = root.join("song.json");
        let record=serde_json::from_value(serde_json::json!({"trackName":"Song","artistName":"Artist","duration":180,"syncedLyrics":"[00:01.00]Fixture lyric line one\n[00:02.00]Fixture lyric line two\n[00:03.00]Fixture lyric line three\n[00:04.00]Fixture lyric line four"})).unwrap();
        super::super::lyrics_cache::save(&path, &record);
        let info = cached_lrclib("r-G2pKr2_n8", &path).unwrap();
        assert!(info.lyrics_synced);
        assert_eq!(info.lyrics_lines.unwrap()[0].start_ms, 1000);
        assert_eq!(info.lyrics_sources[0].status, "failed");
        assert!(info.lyrics_sources[1].synced);
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn artwork_only_accepts_expected_https_cdn_hosts() {
        for url in [
            "https://i.ytimg.com/vi/x/hqdefault.jpg",
            "https://lh3.googleusercontent.com/image",
            "https://yt3.ggpht.com/image",
        ] {
            assert!(artwork_url(url).is_some());
        }
        for url in [
            "http://i.ytimg.com/x",
            "https://i.ytimg.com.evil.test/x",
            "https://evil.test/x",
            "data:image/svg+xml,x",
            "https://u@i.ytimg.com/x",
        ] {
            assert!(artwork_url(url).is_none());
        }
    }
    fn found(timed: bool) -> FallbackLyrics {
        FallbackLyrics {
            text: Some("lrclib plain".into()),
            timed: timed.then(|| {
                vec![TimedLine {
                    start_ms: 1500,
                    text: "lrclib line".into(),
                    words: None,
                }]
            }),
            instrumental: false,
            timing_mismatch: false,
        }
    }
    fn yt() -> Option<(String, String)> {
        Some(("youtube plain".into(), "Source: LyricFind".into()))
    }
    #[test]
    fn timed_lines_win_for_display_and_youtube_text_stays_in_lyrics() {
        let chosen = select(yt(), "available", Ok(Some(found(true))));
        assert_eq!(chosen.lyrics.as_deref(), Some("youtube plain"));
        assert_eq!(chosen.lines.unwrap()[0].text, "lrclib line");
        assert_eq!(
            chosen.source.as_deref(),
            Some("Lyrics from LRCLIB · synced")
        );
        assert_eq!(chosen.provider, Some("LRCLIB"));
    }
    #[test]
    fn plain_fallbacks_when_no_timing_or_lookup_fails() {
        for lookup in [Ok(Some(found(false))), Ok(None), Err(LyricsError)] {
            let chosen = select(yt(), "available", lookup);
            assert!(chosen.lines.is_none());
            assert_eq!(chosen.provider, Some("YouTube Music"));
            assert_eq!(chosen.source.as_deref(), Some("Source: LyricFind"));
            assert_eq!(chosen.status, "available");
        }
        let chosen = select(None, "unavailable", Ok(Some(found(false))));
        assert_eq!(chosen.lyrics.as_deref(), Some("lrclib plain"));
        assert!(chosen.lines.is_none());
        let chosen = select(None, "unavailable", Ok(Some(found(true))));
        assert!(chosen.lines.is_some());
        assert_eq!(chosen.lyrics.as_deref(), Some("lrclib plain"));
        assert_eq!(select(None, "unavailable", Ok(None)).status, "unavailable");
        assert_eq!(select(None, "failed", Ok(None)).status, "failed");
        assert_eq!(
            select(None, "unavailable", Err(LyricsError)).status,
            "failed"
        );
    }
    #[test]
    fn dto_serializes_camel_case_with_explicit_null_lines() {
        let mut info = TrackInfo {
            provider_track_id: "abcdefghijk".into(),
            title: "T".into(),
            artist_refs: vec![],
            artists: vec![],
            artwork_url: None,
            lyrics: None,
            lyrics_source: None,
            lyrics_provider: None,
            lyrics_status: "unavailable",
            lyrics_lines: None,
            lyrics_synced: false,
            lyrics_sources: no_sources(),
        };
        let json = serde_json::to_value(&info).unwrap();
        assert!(json["lyricsLines"].is_null());
        assert_eq!(json["lyricsSynced"], false);
        info.lyrics_lines = found(true).timed;
        info.lyrics_synced = true;
        let json = serde_json::to_value(&info).unwrap();
        assert_eq!(json["lyricsLines"][0]["startMs"], 1500);
        assert_eq!(json["lyricsLines"][0]["text"], "lrclib line");
        assert_eq!(json["lyricsSynced"], true);
    }
    fn ids(list: &[LyricsSource]) -> Vec<(&str, &str, bool)> {
        list.iter().map(|s| (s.id, s.status, s.available)).collect()
    }
    #[test]
    fn sources_both_available_keep_order_and_never_mix_lines() {
        let list = sources(yt().as_ref(), "available", &Ok(Some(found(true))));
        assert_eq!(
            ids(&list),
            [
                ("youtubeMusic", "available", true),
                ("lrclib", "available", true)
            ]
        );
        assert_eq!(list[0].label, "YouTube Music (LyricFind)");
        assert_eq!(list[0].text.as_deref(), Some("youtube plain"));
        assert!(list[0].lines.is_none() && !list[0].synced);
        assert_eq!(list[0].attribution.as_deref(), Some("Source: LyricFind"));
        assert_eq!(list[1].label, "LRCLIB");
        assert_eq!(list[1].text.as_deref(), Some("lrclib plain"));
        assert_eq!(list[1].lines.as_ref().unwrap()[0].start_ms, 1500);
        assert!(list[1].synced);
        assert_eq!(
            list[1].attribution.as_deref(),
            Some("Lyrics from LRCLIB · synced")
        );
    }
    #[test]
    fn label_names_lyricfind_only_when_attribution_says_so() {
        let other = Some(("t".to_string(), "Source: Musixmatch".to_string()));
        let list = sources(other.as_ref(), "available", &Ok(None));
        assert_eq!(list[0].label, "YouTube Music");
        assert_eq!(
            sources(None, "unavailable", &Ok(None))[0].label,
            "YouTube Music"
        );
    }
    #[test]
    fn sources_single_none_instrumental_and_failed() {
        let only_yt = sources(yt().as_ref(), "available", &Ok(None));
        assert_eq!(
            ids(&only_yt),
            [
                ("youtubeMusic", "available", true),
                ("lrclib", "noMatch", false)
            ]
        );
        let only_lr = sources(None, "unavailable", &Ok(Some(found(false))));
        assert_eq!(
            ids(&only_lr),
            [
                ("youtubeMusic", "unavailable", false),
                ("lrclib", "available", true)
            ]
        );
        assert!(only_lr[1].lines.is_none() && !only_lr[1].synced);
        assert_eq!(
            only_lr[1].attribution.as_deref(),
            Some("Lyrics from LRCLIB · https://lrclib.net")
        );
        assert_eq!(
            ids(&no_sources()),
            [
                ("youtubeMusic", "unavailable", false),
                ("lrclib", "unavailable", false)
            ]
        );
        let instrumental = FallbackLyrics {
            text: None,
            timed: None,
            instrumental: true,
            timing_mismatch: false,
        };
        let inst = sources(None, "unavailable", &Ok(Some(instrumental)));
        assert_eq!(ids(&inst)[1], ("lrclib", "instrumental", false));
        assert!(inst[1].text.is_none());
        let failed = sources(yt().as_ref(), "available", &Err(LyricsError));
        assert_eq!(
            ids(&failed),
            [
                ("youtubeMusic", "available", true),
                ("lrclib", "failed", false)
            ]
        );
        let both_failed = sources(None, "failed", &Err(LyricsError));
        assert_eq!(ids(&both_failed)[0], ("youtubeMusic", "failed", false));
    }
    #[test]
    fn lrclib_status_says_why_it_is_unavailable() {
        let mut mismatch = found(false);
        mismatch.timing_mismatch = true;
        let list = sources(None, "unavailable", &Ok(Some(mismatch)));
        assert_eq!(ids(&list)[1], ("lrclib", "timingMismatch", true));
        assert!(list[1].lines.is_none() && !list[1].synced && list[1].text.is_some());
        assert_eq!(
            ids(&sources(None, "unavailable", &Ok(None)))[1],
            ("lrclib", "noMatch", false)
        );
        assert_eq!(
            ids(&sources(None, "unavailable", &Err(LyricsError)))[1],
            ("lrclib", "failed", false)
        );
        assert_eq!(ids(&no_sources())[1], ("lrclib", "unavailable", false));
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json[1]["status"], "timingMismatch");
    }
    #[test]
    fn should_prefetch_only_when_nothing_is_cached_and_the_track_is_new() {
        let ok = |dir, cached, busy_same, artists: &[&str], secs| {
            should_prefetch(
                dir,
                "Song",
                &artists.iter().map(|a| (*a).into()).collect::<Vec<_>>(),
                secs,
                cached,
                busy_same,
            )
        };
        assert!(ok(true, false, false, &["A"], Some(180)));
        assert!(!ok(false, false, false, &["A"], Some(180)), "no cache dir");
        assert!(
            !ok(true, true, false, &["A"], Some(180)),
            "positive or negative entry exists"
        );
        assert!(
            !ok(true, false, true, &["A"], Some(180)),
            "same track already in flight"
        );
        assert!(!ok(true, false, false, &[], Some(180)), "no artist");
        assert!(!ok(true, false, false, &["A"], None), "unknown length");
        assert!(
            !ok(true, false, false, &["A"], Some(7200)),
            "hour-long recording"
        );
        assert!(!should_prefetch(
            true,
            "  ",
            &["A".into()],
            Some(180),
            false,
            false
        ));
    }
    #[test]
    fn sources_serialize_camel_case_with_explicit_nulls() {
        let json =
            serde_json::to_value(sources(None, "unavailable", &Ok(Some(found(true))))).unwrap();
        assert_eq!(json[0]["id"], "youtubeMusic");
        assert_eq!(json[0]["available"], false);
        assert!(json[0]["text"].is_null() && json[0]["lines"].is_null());
        assert!(json[0]["attribution"].is_null());
        assert_eq!(json[1]["lines"][0]["startMs"], 1500);
        assert_eq!(json[1]["synced"], true);
        assert_eq!(json[1]["status"], "available");
    }
    fn entry(id: &'static str, text: bool, synced: bool, words: bool) -> LyricsSource {
        LyricsSource {
            id,
            label: id.into(),
            available: text,
            text: text.then(|| "t".into()),
            lines: None,
            synced,
            words,
            bound: false,
            attribution: None,
            status: "available",
        }
    }
    #[test]
    fn auto_pick_prefers_words_then_timestamps_then_plain_then_user_order() {
        let ctx = super::super::lyric_sources::Ctx::default();
        let pick = |list: &[LyricsSource], ctx: &super::super::lyric_sources::Ctx| {
            auto_pick(list, ctx).map(|e| e.id)
        };
        let list = [
            entry("lrclib", true, true, false),
            entry("netease", true, true, true),
            entry("youtubeMusic", true, false, false),
        ];
        assert_eq!(pick(&list, &ctx), Some("netease"));
        let list = [
            entry("qq", true, false, false),
            entry("kugou", true, true, false),
            entry("lrclib", true, false, false),
        ];
        assert_eq!(pick(&list, &ctx), Some("kugou"));
        // Same tier: the default order puts lrclib before qq.
        let list = [
            entry("qq", true, false, false),
            entry("lrclib", true, false, false),
        ];
        assert_eq!(pick(&list, &ctx), Some("lrclib"));
        // The user's order wins ties, and disabled/empty entries never win.
        let custom = super::super::lyric_sources::Ctx {
            settings: super::super::lyric_prefs::normalize(&["qq".to_owned()], &[]).unwrap(),
            ..Default::default()
        };
        assert_eq!(pick(&list, &custom), Some("qq"));
        assert_eq!(pick(&[entry("qq", false, false, false)], &ctx), None);
    }
    #[test]
    fn extra_sources_extend_the_dto_additively() {
        let mut list = no_sources();
        list.push(entry("boidu", true, true, true));
        let json = serde_json::to_value(&list).unwrap();
        assert_eq!(json[0]["words"], false);
        assert_eq!(json[0]["bound"], false);
        assert_eq!(json[2]["id"], "boidu");
        assert_eq!(json[2]["words"], true);
        assert_eq!(chosen_from(&list[2]).provider, Some("Boidu"));
        assert_eq!(
            chosen_from(&entry("netease", true, true, false)).provider,
            Some("Netease Cloud Music")
        );
    }
}
