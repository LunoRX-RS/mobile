//! YouTube Data API v3 `search.list` adapter (blueprint 8.5, compliance record Y1-Y15).
//!
//! The API key comes from the OS keyring through `TokenStore` on every call. It travels only in the
//! `x-goog-api-key` header (never in the URL, so reqwest error text cannot carry it), is never
//! logged, and no error built here contains provider text.
//!
//! Caching (Y5): none. Results live only in the response of the request that fetched them.

use std::sync::Arc;
use std::time::Duration;

use serde::Deserialize;

use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::search::hit::SearchHit;
use crate::core::search::provider::{SearchFuture, SearchProvider};
use crate::sync::credentials::TokenStore;

/// Google's default `search.list` quota (compliance record Y13). One call costs one unit of this bucket.
pub const DEFAULT_DAILY_SEARCH_LIMIT: u32 = 100;

const ENDPOINT: &str = "https://www.googleapis.com/youtube/v3/search";
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MUSIC_CATEGORY_ID: &str = "10";

pub struct YouTubeSearch {
    http: reqwest::Client,
    keys: Arc<dyn TokenStore>,
    endpoint: String,
}

impl YouTubeSearch {
    pub fn new(keys: Arc<dyn TokenStore>) -> Result<Self, CoreError> {
        Self::with_endpoint(keys, ENDPOINT)
    }

    // Separate constructor so a test can point the adapter at a local server.
    pub fn with_endpoint(keys: Arc<dyn TokenStore>, endpoint: &str) -> Result<Self, CoreError> {
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(REQUEST_TIMEOUT)
            .build()
            .map_err(|_| CoreError::ProviderUnavailable)?;
        Ok(Self {
            http,
            keys,
            endpoint: endpoint.to_owned(),
        })
    }

    async fn run(&self, query: &str, limit: u8) -> Result<Vec<SearchHit>, CoreError> {
        let key = match self.keys.get() {
            Ok(Some(k)) if !k.is_empty() => k,
            Ok(_) => return Err(CoreError::AuthenticationRequired),
            Err(_) => return Err(CoreError::ProviderUnavailable),
        };
        let max_results = limit.clamp(1, 10).to_string();
        let resp = self
            .http
            .get(&self.endpoint)
            .header("x-goog-api-key", key)
            .query(&[
                ("part", "snippet"),
                ("type", "video"),
                ("videoCategoryId", MUSIC_CATEGORY_ID),
                ("maxResults", max_results.as_str()),
                ("q", query),
            ])
            .send()
            .await
            .map_err(|_| CoreError::Network("request failed".into()))?;
        let status = resp.status().as_u16();
        let body = resp
            .bytes()
            .await
            .map_err(|_| CoreError::Network("read failed".into()))?;
        if body.len() > MAX_BODY_BYTES {
            return Err(CoreError::Provider("response too large".into()));
        }
        parse_response(status, &body)
    }
}

impl SearchProvider for YouTubeSearch {
    fn id(&self) -> ProviderId {
        ProviderId::YouTube
    }

    fn search<'a>(&'a self, query: &'a str, limit: u8) -> SearchFuture<'a> {
        Box::pin(self.run(query, limit))
    }
}

#[derive(Deserialize)]
struct ListBody {
    #[serde(default)]
    items: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    id: Option<ItemId>,
    snippet: Option<Snippet>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ItemId {
    video_id: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Snippet {
    title: Option<String>,
    channel_title: Option<String>,
    thumbnails: Option<Thumbnails>,
}

#[derive(Deserialize)]
struct Thumbnails {
    medium: Option<Thumb>,
    default: Option<Thumb>,
    high: Option<Thumb>,
}

#[derive(Deserialize)]
struct Thumb {
    url: Option<String>,
}

#[derive(Deserialize)]
struct ErrorBody {
    error: Option<ErrorInner>,
}

#[derive(Deserialize)]
struct ErrorInner {
    #[serde(default)]
    errors: Vec<ErrorReason>,
    #[serde(default)]
    details: Vec<ErrorReason>,
}

#[derive(Deserialize)]
struct ErrorReason {
    reason: Option<String>,
}

/// HTTP status and body to hits or a typed error. Pure, so tests need no network.
fn parse_response(status: u16, body: &[u8]) -> Result<Vec<SearchHit>, CoreError> {
    if (200..300).contains(&status) {
        return parse_hits(body);
    }
    // Error bodies are best effort: an unparsable one still maps by status.
    let reasons: Vec<String> = serde_json::from_slice::<ErrorBody>(body)
        .ok()
        .and_then(|b| b.error)
        .map(|e| {
            e.errors
                .into_iter()
                .chain(e.details)
                .filter_map(|r| r.reason)
                .collect()
        })
        .unwrap_or_default();
    let has = |names: &[&str]| reasons.iter().any(|r| names.contains(&r.as_str()));
    if has(&["quotaExceeded", "dailyLimitExceeded", "rateLimitExceeded"]) {
        return Err(CoreError::QuotaExceeded);
    }
    if status == 401
        || has(&[
            "keyInvalid",
            "API_KEY_INVALID",
            "keyExpired",
            "accessNotConfigured",
            "ipRefererBlocked",
        ])
    {
        return Err(CoreError::AuthenticationRequired);
    }
    Err(match status {
        429 => CoreError::QuotaExceeded,
        500..=599 => CoreError::ProviderUnavailable,
        _ => CoreError::Provider("request rejected".into()),
    })
}

fn parse_hits(body: &[u8]) -> Result<Vec<SearchHit>, CoreError> {
    let parsed: ListBody = serde_json::from_slice(body)
        .map_err(|_| CoreError::Provider("malformed response".into()))?;
    // An item without a valid id or snippet is dropped, not fabricated.
    Ok(parsed.items.into_iter().filter_map(to_hit).collect())
}

fn to_hit(item: Item) -> Option<SearchHit> {
    let id = item.id?.video_id?;
    if !is_video_id(&id) {
        return None;
    }
    let snippet = item.snippet?;
    let mut hit = SearchHit::new(
        ProviderId::YouTube,
        id.clone(),
        format!("https://www.youtube.com/watch?v={id}"),
        unescape_html(&snippet.title?),
        snippet
            .channel_title
            .map(|c| unescape_html(&c))
            .filter(|c| !c.is_empty())
            .into_iter()
            .collect(),
    )
    .ok()?;
    hit.artwork_url = snippet.thumbnails.and_then(pick_thumbnail);
    // search.list carries no duration; duration_ms stays None.
    Some(hit)
}

fn is_video_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

fn pick_thumbnail(t: Thumbnails) -> Option<String> {
    [t.medium, t.default, t.high]
        .into_iter()
        .flatten()
        .filter_map(|t| t.url)
        .find(|u| u.starts_with("https://"))
}

/// Decodes the entities search.list puts in titles: the five named XML ones plus numeric.
fn unescape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let decoded = rest.find(';').filter(|&e| e <= 10).and_then(|e| {
            let c = match &rest[1..e] {
                "amp" => '&',
                "lt" => '<',
                "gt" => '>',
                "quot" => '"',
                "apos" => '\'',
                num => {
                    let n = num.strip_prefix('#')?;
                    let code = match n.strip_prefix(['x', 'X']) {
                        Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                        None => n.parse().ok()?,
                    };
                    char::from_u32(code)?
                }
            };
            Some((c, e))
        });
        match decoded {
            Some((c, e)) => {
                out.push(c);
                rest = &rest[e + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::credentials::TokenStoreError;
    use std::sync::Mutex;

    const SENTINEL_KEY: &str = "SENTINEL-KEY-abcdefghij0123456789";

    #[derive(Default)]
    struct Keys(Mutex<Option<String>>);

    impl TokenStore for Keys {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            Ok(self.0.lock().unwrap().clone())
        }
        fn set(&self, t: &str) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = Some(t.into());
            Ok(())
        }
        fn clear(&self) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    const OK_BODY: &str = r#"{"items":[
        {"id":{"kind":"youtube#video","videoId":"dQw4w9WgXcQ"},
         "snippet":{"title":"Rock &amp; Roll &#39;24 &quot;Live&quot;","channelTitle":"The Band",
           "thumbnails":{"default":{"url":"https://i.ytimg.com/d.jpg"},"medium":{"url":"https://i.ytimg.com/m.jpg"}}}},
        {"id":{"videoId":"short"},"snippet":{"title":"bad id","channelTitle":"x"}},
        {"id":{"kind":"youtube#channel","channelId":"UC1"},"snippet":{"title":"a channel"}},
        {"id":{"videoId":"abcdefghij_"},"snippet":{"title":"No thumb","channelTitle":"",
           "thumbnails":{"default":{"url":"http://insecure/x.jpg"}}}}
    ]}"#;

    #[test]
    fn maps_hits_and_drops_invalid_items() {
        let hits = parse_response(200, OK_BODY.as_bytes()).unwrap();
        assert_eq!(hits.len(), 2);
        let h = &hits[0];
        assert_eq!(h.provider(), ProviderId::YouTube);
        assert_eq!(h.provider_track_id(), "dQw4w9WgXcQ");
        assert_eq!(
            h.canonical_url(),
            "https://www.youtube.com/watch?v=dQw4w9WgXcQ"
        );
        assert_eq!(h.title(), "Rock & Roll '24 \"Live\"");
        assert_eq!(h.artists(), ["The Band".to_string()].as_slice());
        assert_eq!(h.artwork_url(), Some("https://i.ytimg.com/m.jpg"));
        assert_eq!(h.duration_ms(), None);
        let second = &hits[1];
        assert!(second.artists().is_empty());
        assert_eq!(second.artwork_url(), None);
    }

    #[test]
    fn empty_items_is_ok_and_missing_items_is_ok() {
        assert_eq!(parse_response(200, br#"{"items":[]}"#), Ok(vec![]));
        assert_eq!(parse_response(200, b"{}"), Ok(vec![]));
    }

    #[test]
    fn malformed_json_is_provider_error() {
        assert!(matches!(
            parse_response(200, b"<html>"),
            Err(CoreError::Provider(_))
        ));
        assert!(matches!(
            parse_response(200, br#"{"items":"x"}"#),
            Err(CoreError::Provider(_))
        ));
    }

    fn err_body(reason: &str) -> Vec<u8> {
        format!(r#"{{"error":{{"code":403,"errors":[{{"reason":"{reason}"}}]}}}}"#).into_bytes()
    }

    #[test]
    fn quota_exceeded_maps_to_quota_error() {
        for r in ["quotaExceeded", "dailyLimitExceeded", "rateLimitExceeded"] {
            assert_eq!(
                parse_response(403, &err_body(r)),
                Err(CoreError::QuotaExceeded),
                "{r}"
            );
        }
        assert_eq!(parse_response(429, b""), Err(CoreError::QuotaExceeded));
    }

    #[test]
    fn key_problems_map_to_authentication_required() {
        assert_eq!(
            parse_response(400, &err_body("keyInvalid")),
            Err(CoreError::AuthenticationRequired)
        );
        assert_eq!(
            parse_response(403, &err_body("accessNotConfigured")),
            Err(CoreError::AuthenticationRequired)
        );
        let details = br#"{"error":{"code":400,"details":[{"reason":"API_KEY_INVALID"}]}}"#;
        assert_eq!(
            parse_response(400, details),
            Err(CoreError::AuthenticationRequired)
        );
        assert_eq!(
            parse_response(401, b""),
            Err(CoreError::AuthenticationRequired)
        );
    }

    #[test]
    fn other_statuses_map_to_fixed_errors() {
        assert_eq!(
            parse_response(503, b""),
            Err(CoreError::ProviderUnavailable)
        );
        assert!(matches!(
            parse_response(404, b"not json"),
            Err(CoreError::Provider(_))
        ));
        // An unknown 403 reason is not mistaken for quota or key trouble.
        assert!(matches!(
            parse_response(403, &err_body("forbidden")),
            Err(CoreError::Provider(_))
        ));
    }

    #[test]
    fn unescape_handles_named_numeric_and_stray_ampersands() {
        assert_eq!(unescape_html("a &amp; b &lt;c&gt;"), "a & b <c>");
        assert_eq!(unescape_html("&#39;x&#x27; &#128512;"), "'x' \u{1F600}");
        assert_eq!(
            unescape_html("AT&T &unknown; &#zz; &"),
            "AT&T &unknown; &#zz; &"
        );
        // Decoding is single pass: an escaped entity is not decoded twice.
        assert_eq!(unescape_html("&amp;amp;"), "&amp;");
    }

    #[test]
    fn video_id_validation() {
        assert!(is_video_id("dQw4w9WgXcQ"));
        assert!(is_video_id("a-_1234567B"));
        assert!(!is_video_id("dQw4w9WgXc"));
        assert!(!is_video_id("dQw4w9WgXcQ1"));
        assert!(!is_video_id("dQw4w9WgX/Q"));
    }

    #[tokio::test]
    async fn no_key_is_authentication_required_without_a_request() {
        // Port 1 would refuse; AuthenticationRequired proves no request was attempted.
        let p =
            YouTubeSearch::with_endpoint(Arc::new(Keys::default()), "http://127.0.0.1:1").unwrap();
        assert_eq!(
            p.search("lofi", 5).await,
            Err(CoreError::AuthenticationRequired)
        );
    }

    #[tokio::test]
    async fn network_failure_is_network_error_without_the_key() {
        let keys = Keys::default();
        keys.set(SENTINEL_KEY).unwrap();
        let p = YouTubeSearch::with_endpoint(Arc::new(keys), "http://127.0.0.1:1").unwrap();
        let err = p.search("lofi", 5).await.unwrap_err();
        assert!(matches!(err, CoreError::Network(_)), "{err:?}");
        assert!(!format!("{err:?}{err}").contains(SENTINEL_KEY));
    }

    #[test]
    fn id_and_default_limit() {
        let p = YouTubeSearch::new(Arc::new(Keys::default())).unwrap();
        assert_eq!(p.id(), ProviderId::YouTube);
        assert_eq!(DEFAULT_DAILY_SEARCH_LIMIT, 100);
    }
}
