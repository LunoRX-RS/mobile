//! Typed HTTP client for the backend sync API (`/api/v1`, see backend/openapi.yaml).

use std::collections::VecDeque;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::settings::{SettingsError, parse_base_url};

#[derive(Debug, thiserror::Error)]
pub enum SyncError {
    #[error("{0}")]
    Settings(SettingsError),
    #[error("{0}")]
    Local(&'static str),
    #[error("backend unreachable")]
    Network,
    #[error("{0}")]
    Unauthenticated(String),
    #[error("{0}")]
    Forbidden(String),
    #[error("{0}")]
    Validation(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{message}")]
    Conflict {
        message: String,
        current_version: Option<i64>,
    },
    #[error("{message}")]
    RateLimited {
        message: String,
        retry_after: Option<u64>,
    },
    #[error("{0}")]
    Internal(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provider {
    Spotify,
    YouTube,
}

/// One upload of anonymous usage counters: `POST /api/v1/telemetry`.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryBatch<'a> {
    pub install_id: &'a str,
    pub app_version: &'a str,
    pub os: &'a str,
    pub arch: &'a str,
    pub counters: Vec<TelemetryCounter<'a>>,
}

#[derive(Debug, Serialize)]
pub struct TelemetryCounter<'a> {
    pub name: &'a str,
    pub count: u32,
}

/// One track's play count, the shape of `PUT /api/v1/stats/{deviceId}` and `GET /api/v1/stats`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsRow {
    pub provider: String,
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub plays: u32,
    pub last_played_ms: u64,
}

#[derive(Debug, Deserialize)]
struct StatsList {
    tracks: Vec<StatsRow>,
}

/// One liked track as the backend stores it (`LikedTrack` in backend/openapi.yaml). The key travels in the body here and
/// in the path for PUT/DELETE, so both shapes are built from this one struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeTrack {
    pub provider: String,
    pub provider_track_id: String,
    pub canonical_url: String,
    pub title: String,
    pub artists: Vec<String>,
    #[serde(default)]
    pub artist_refs: Vec<LikeArtistRef>,
    #[serde(default)]
    pub source: Option<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeArtistRef {
    pub provider: String,
    #[serde(default)]
    pub id: Option<String>,
    pub name: String,
}

/// `GET /api/v1/liked-songs`: the whole set and the revision it belongs to.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeSnapshot {
    pub revision: i64,
    pub tracks: Vec<LikeTrack>,
}

/// `GET /api/v1/liked-songs/{provider}/{trackId}`. A missing like is 200 with `liked: false`, not an error.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeEntry {
    pub liked: bool,
    #[serde(default)]
    pub track: Option<LikeTrack>,
}

/// The PUT body: metadata only, since the key is in the path.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeTrackBody<'a> {
    pub canonical_url: &'a str,
    pub title: &'a str,
    pub artists: &'a [String],
    pub artist_refs: &'a [LikeArtistRef],
    pub source: Option<&'a str>,
    pub artwork_url: Option<&'a str>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LikeMergeResult {
    pub revision: i64,
    pub added: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LoginResponse {
    pub token: String,
    pub expires_at: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionResponse {
    pub user_id: String,
    pub expires_at: String,
}

/// Shape of `GET /api/v1/shared/{token}`. Only the fields the share screen shows are read.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedPlaylist {
    pub name: String,
    pub description: Option<String>,
    pub entries: Vec<SharedEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SharedEntry {
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: String,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCreateRequest {
    pub name: String,
    pub description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareView {
    pub id: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// Not Debug: the token must never reach a log through `{:?}`.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreatedShare {
    pub id: String,
    pub token: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistCreated {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: i64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Playlist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub version: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistUpdateRequest {
    pub expected_version: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Outer None omits the field (unchanged); Some(None) sends null (clears).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<Option<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub id: String,
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: Option<String>,
    pub artists: Option<Vec<String>>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub position: i64,
    pub created_at: String,
}

#[derive(Debug, Deserialize)]
pub struct EntryAdded {
    #[serde(flatten)]
    pub entry: Entry,
    pub version: i64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EntryAddRequest {
    pub expected_version: i64,
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: Option<String>,
    pub artists: Option<Vec<String>>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ReorderRequest<'a> {
    expected_version: i64,
    entry_ids: &'a [String],
}

#[derive(Debug, Deserialize)]
pub struct ReorderResponse {
    pub version: i64,
}

#[derive(Deserialize)]
struct ErrorBody {
    code: String,
    #[serde(default)]
    message: String,
    #[serde(default)]
    details: Value,
}

#[derive(Deserialize)]
struct Envelope {
    error: ErrorBody,
}

/// Maps the backend envelope to a variant by its code; unknown codes fall back to the HTTP status.
fn map_error(status: u16, retry_after: Option<u64>, body: &[u8]) -> SyncError {
    let (code, message, details) = match serde_json::from_slice::<Envelope>(body) {
        Ok(env) => (env.error.code, env.error.message, env.error.details),
        Err(_) => (String::new(), String::new(), Value::Null),
    };
    let kind = match code.as_str() {
        "UNAUTHENTICATED" => 401,
        "FORBIDDEN" => 403,
        "VALIDATION_FAILED" => 400,
        "PLAYLIST_NOT_FOUND" => 404,
        "VERSION_CONFLICT" => 409,
        "RATE_LIMITED" => 429,
        _ => status,
    };
    let message = if message.is_empty() {
        "request failed".to_string()
    } else {
        message
    };
    match kind {
        401 => SyncError::Unauthenticated(message),
        403 => SyncError::Forbidden(message),
        400 => SyncError::Validation(message),
        404 => SyncError::NotFound(message),
        409 => SyncError::Conflict {
            message,
            current_version: details.get("currentVersion").and_then(Value::as_i64),
        },
        429 => SyncError::RateLimited {
            message,
            retry_after,
        },
        _ => SyncError::Internal(message),
    }
}

fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, SyncError> {
    serde_json::from_slice(bytes)
        .map_err(|_| SyncError::Internal("unexpected response from backend".into()))
}

#[derive(Debug, Clone)]
pub struct ApiClient {
    base: String,
    http: reqwest::Client,
    /// Cloudflare Turnstile tokens for the next register/login calls, in order. A token is single use, so each
    /// call takes one. Empty: the call is sent without one (the server then must have the antibot check off).
    antibot: Arc<StdMutex<VecDeque<String>>>,
}

/// Bounds every backend call so a stalled connection cannot hold the sync engine indefinitely.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

fn build_http(connect: Duration, total: Duration) -> Result<reqwest::Client, SyncError> {
    reqwest::Client::builder()
        .connect_timeout(connect)
        .timeout(total)
        .build()
        .map_err(|_| SyncError::Internal("http client unavailable".into()))
}

/// IDs placed in a request path must be plain identifiers. A `/`, `.`, `?`, `#` or `%` could change
/// the route the bearer token is sent to, so such IDs are rejected before any request is made.
fn path_id(id: &str) -> Result<&str, SyncError> {
    let plain = !id.is_empty()
        && id.len() <= 128
        && id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_');
    if plain {
        Ok(id)
    } else {
        Err(SyncError::Internal("invalid id in request path".into()))
    }
}

impl ApiClient {
    pub fn new(base_url: &str) -> Result<Self, SyncError> {
        let base = parse_base_url(base_url).map_err(SyncError::Settings)?;
        Ok(Self {
            base,
            http: build_http(CONNECT_TIMEOUT, REQUEST_TIMEOUT)?,
            antibot: Arc::default(),
        })
    }

    /// Hands the next register/login calls their antibot tokens, replacing any left over.
    pub fn set_antibot(&self, tokens: Vec<String>) {
        *self.antibot.lock().unwrap_or_else(|e| e.into_inner()) = tokens.into();
    }

    fn next_antibot(&self) -> Option<String> {
        self.antibot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .pop_front()
    }

    fn url(&self, path: &str) -> String {
        format!("{}{path}", self.base)
    }

    /// Sends a request; returns the body on 2xx, otherwise the mapped error.
    async fn send(&self, req: reqwest::RequestBuilder) -> Result<Vec<u8>, SyncError> {
        let res = req.send().await.map_err(|_| SyncError::Network)?;
        let status = res.status().as_u16();
        let retry_after = res
            .headers()
            .get(reqwest::header::RETRY_AFTER)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.trim().parse::<u64>().ok());
        let body = res.bytes().await.map_err(|_| SyncError::Network)?.to_vec();
        if (200..300).contains(&status) {
            Ok(body)
        } else {
            Err(map_error(status, retry_after, &body))
        }
    }

    /// Creates an account. The backend returns no session: the caller logs in afterwards.
    pub async fn register(&self, email: &str, password: &str) -> Result<(), SyncError> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            password: &'a str,
            #[serde(rename = "turnstileToken", skip_serializing_if = "Option::is_none")]
            turnstile_token: Option<String>,
        }
        let req = self
            .http
            .post(self.url("/api/v1/auth/register"))
            .json(&Body {
                email,
                password,
                turnstile_token: self.next_antibot(),
            });
        self.send(req).await.map(|_| ())
    }

    pub async fn login(&self, email: &str, password: &str) -> Result<LoginResponse, SyncError> {
        #[derive(Serialize)]
        struct Body<'a> {
            email: &'a str,
            password: &'a str,
            #[serde(rename = "turnstileToken", skip_serializing_if = "Option::is_none")]
            turnstile_token: Option<String>,
        }
        let req = self.http.post(self.url("/api/v1/auth/login")).json(&Body {
            email,
            password,
            turnstile_token: self.next_antibot(),
        });
        decode(&self.send(req).await?)
    }

    pub async fn logout(&self, token: &str) -> Result<(), SyncError> {
        let req = self
            .http
            .post(self.url("/api/v1/auth/logout"))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    pub async fn get_session(&self, token: &str) -> Result<SessionResponse, SyncError> {
        let req = self
            .http
            .get(self.url("/api/v1/auth/session"))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    /// Public read of a shared playlist. No session token is sent: the share token is the only credential.
    pub async fn get_shared(&self, token: &str) -> Result<SharedPlaylist, SyncError> {
        let token = path_id(token)?;
        let req = self
            .http
            .get(self.url(&format!("/api/v1/shared/{token}")))
            .header(reqwest::header::ACCEPT, "application/json");
        decode(&self.send(req).await?)
    }

    /// No session token: telemetry is anonymous.
    pub async fn post_telemetry(&self, batch: &TelemetryBatch<'_>) -> Result<(), SyncError> {
        let req = self.http.post(self.url("/api/v1/telemetry")).json(batch);
        self.send(req).await.map(|_| ())
    }

    pub async fn delete_telemetry(&self, install_id: &str) -> Result<(), SyncError> {
        let req = self
            .http
            .delete(self.url(&format!("/api/v1/telemetry/{}", path_id(install_id)?)));
        self.send(req).await.map(|_| ())
    }

    /// Replaces this device's stats snapshot for the signed-in user.
    pub async fn put_stats(
        &self,
        token: &str,
        device_id: &str,
        tracks: &[StatsRow],
    ) -> Result<(), SyncError> {
        #[derive(Serialize)]
        struct Body<'a> {
            tracks: &'a [StatsRow],
        }
        let req = self
            .http
            .put(self.url(&format!("/api/v1/stats/{}", path_id(device_id)?)))
            .bearer_auth(token)
            .json(&Body { tracks });
        self.send(req).await.map(|_| ())
    }

    /// The user's stats summed over every device.
    pub async fn get_stats(&self, token: &str) -> Result<Vec<StatsRow>, SyncError> {
        let req = self.http.get(self.url("/api/v1/stats")).bearer_auth(token);
        Ok(decode::<StatsList>(&self.send(req).await?)?.tracks)
    }

    pub async fn delete_stats(&self, token: &str) -> Result<(), SyncError> {
        let req = self
            .http
            .delete(self.url("/api/v1/stats"))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    pub async fn list_likes(&self, token: &str) -> Result<LikeSnapshot, SyncError> {
        let req = self
            .http
            .get(self.url("/api/v1/liked-songs"))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn get_like(
        &self,
        token: &str,
        provider: &str,
        track_id: &str,
    ) -> Result<LikeEntry, SyncError> {
        let provider = path_id(provider)?;
        let track_id = path_id(track_id)?;
        let req = self
            .http
            .get(self.url(&format!("/api/v1/liked-songs/{provider}/{track_id}")))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn put_like(
        &self,
        token: &str,
        provider: &str,
        track_id: &str,
        body: &LikeTrackBody<'_>,
    ) -> Result<(), SyncError> {
        let provider = path_id(provider)?;
        let track_id = path_id(track_id)?;
        let req = self
            .http
            .put(self.url(&format!("/api/v1/liked-songs/{provider}/{track_id}")))
            .bearer_auth(token)
            .json(body);
        self.send(req).await.map(|_| ())
    }

    pub async fn delete_like(
        &self,
        token: &str,
        provider: &str,
        track_id: &str,
    ) -> Result<(), SyncError> {
        let provider = path_id(provider)?;
        let track_id = path_id(track_id)?;
        let req = self
            .http
            .delete(self.url(&format!("/api/v1/liked-songs/{provider}/{track_id}")))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    pub async fn merge_likes(
        &self,
        token: &str,
        tracks: &[LikeTrack],
    ) -> Result<LikeMergeResult, SyncError> {
        #[derive(Serialize)]
        struct Body<'a> {
            tracks: &'a [LikeTrack],
        }
        let req = self
            .http
            .post(self.url("/api/v1/liked-songs/merge"))
            .bearer_auth(token)
            .json(&Body { tracks });
        decode(&self.send(req).await?)
    }

    /// Creates a read-only share link. The token is returned once and never logged or stored by the client.
    pub async fn create_share(&self, token: &str, id: &str) -> Result<CreatedShare, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .post(self.url(&format!("/api/v1/playlists/{id}/shares")))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn list_shares(&self, token: &str, id: &str) -> Result<Vec<ShareView>, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .get(self.url(&format!("/api/v1/playlists/{id}/shares")))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn revoke_share(&self, token: &str, id: &str, share: &str) -> Result<(), SyncError> {
        let id = path_id(id)?;
        let share = path_id(share)?;
        let req = self
            .http
            .delete(self.url(&format!("/api/v1/playlists/{id}/shares/{share}")))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    /// The public URL a recipient opens; contains the share token, so it is shown to the user and never logged.
    pub fn share_url(&self, share_token: &str) -> String {
        self.url(&format!("/api/v1/shared/{share_token}"))
    }

    pub async fn list_playlists(&self, token: &str) -> Result<Vec<Playlist>, SyncError> {
        let req = self
            .http
            .get(self.url("/api/v1/playlists"))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn create_playlist(
        &self,
        token: &str,
        body: &PlaylistCreateRequest,
    ) -> Result<PlaylistCreated, SyncError> {
        let req = self
            .http
            .post(self.url("/api/v1/playlists"))
            .bearer_auth(token)
            .json(body);
        decode(&self.send(req).await?)
    }

    pub async fn get_playlist(&self, token: &str, id: &str) -> Result<Playlist, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .get(self.url(&format!("/api/v1/playlists/{id}")))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn update_playlist(
        &self,
        token: &str,
        id: &str,
        body: &PlaylistUpdateRequest,
    ) -> Result<Playlist, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .patch(self.url(&format!("/api/v1/playlists/{id}")))
            .bearer_auth(token)
            .json(body);
        decode(&self.send(req).await?)
    }

    pub async fn delete_playlist(
        &self,
        token: &str,
        id: &str,
        expected_version: i64,
    ) -> Result<(), SyncError> {
        let id = path_id(id)?;
        // expectedVersion is an integer, so a manual query string needs no encoding.
        let req = self
            .http
            .delete(self.url(&format!(
                "/api/v1/playlists/{id}?expectedVersion={expected_version}"
            )))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    pub async fn list_entries(&self, token: &str, id: &str) -> Result<Vec<Entry>, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .get(self.url(&format!("/api/v1/playlists/{id}/entries")))
            .bearer_auth(token);
        decode(&self.send(req).await?)
    }

    pub async fn add_entry(
        &self,
        token: &str,
        id: &str,
        body: &EntryAddRequest,
    ) -> Result<EntryAdded, SyncError> {
        let id = path_id(id)?;
        // Mirrors backend rule R-S2: Spotify entries carry ids only. Checked before any request.
        if body.provider == Provider::Spotify
            && (body.title.is_some()
                || body.artists.is_some()
                || body.artwork_url.is_some()
                || body.duration_ms.is_some())
        {
            return Err(SyncError::Local(
                "Spotify entries store ids only; title, artists, artwork and duration must be omitted",
            ));
        }
        let req = self
            .http
            .post(self.url(&format!("/api/v1/playlists/{id}/entries")))
            .bearer_auth(token)
            .json(body);
        decode(&self.send(req).await?)
    }

    pub async fn remove_entry(
        &self,
        token: &str,
        id: &str,
        entry_id: &str,
        expected_version: i64,
    ) -> Result<(), SyncError> {
        let id = path_id(id)?;
        let entry_id = path_id(entry_id)?;
        let req = self
            .http
            .delete(self.url(&format!(
                "/api/v1/playlists/{id}/entries/{entry_id}?expectedVersion={expected_version}"
            )))
            .bearer_auth(token);
        self.send(req).await.map(|_| ())
    }

    pub async fn reorder_entries(
        &self,
        token: &str,
        id: &str,
        expected_version: i64,
        entry_ids: &[String],
    ) -> Result<ReorderResponse, SyncError> {
        let id = path_id(id)?;
        let req = self
            .http
            .put(self.url(&format!("/api/v1/playlists/{id}/order")))
            .bearer_auth(token)
            .json(&ReorderRequest {
                expected_version,
                entry_ids,
            });
        decode(&self.send(req).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const TOKEN: &str = "SENTINEL-SESSION-TOKEN-4d1e";
    const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    const ENTRY: &str = "6f1d2c3a-0000-4000-8000-000000000002";
    const ENTRY2: &str = "6f1d2c3a-0000-4000-8000-000000000003";

    #[derive(Clone)]
    struct Recorded {
        method: String,
        path: String,
        auth: Option<String>,
        body: String,
    }

    struct Stub {
        base: String,
        seen: Arc<Mutex<Vec<Recorded>>>,
    }

    impl Stub {
        fn count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }

        fn last(&self) -> Recorded {
            self.seen
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("no request recorded")
        }
    }

    /// Serves `response` (raw HTTP/1.1) to every connection on 127.0.0.1:0 and records each request.
    async fn stub(response: String) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let raw = read_request(&mut sock).await;
                log.lock().unwrap().push(parse_request(&raw));
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        Stub { base, seen }
    }

    async fn read_request(sock: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = sock.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn parse_request(raw: &str) -> Recorded {
        let (head, body) = raw.split_once("\r\n\r\n").unwrap_or((raw, ""));
        let mut lines = head.lines();
        let mut start = lines.next().unwrap_or("").split(' ');
        let method = start.next().unwrap_or("").to_string();
        let path = start.next().unwrap_or("").to_string();
        let auth = lines.find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("authorization")
                .then(|| v.trim().to_string())
        });
        Recorded {
            method,
            path,
            auth,
            body: body.to_string(),
        }
    }

    fn raw(status: &str, headers: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n{body}",
            body.len()
        )
    }

    fn client(base: &str) -> ApiClient {
        ApiClient::new(base).unwrap()
    }

    /// Accepts connections and never answers, so only a client timeout can end the request.
    async fn stalled() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move {
            let mut held = Vec::new();
            while let Ok((sock, _)) = listener.accept().await {
                held.push(sock);
            }
        });
        base
    }

    #[tokio::test]
    async fn stalled_backend_fails_with_network_error_at_the_request_timeout() {
        let base = stalled().await;
        let api = ApiClient {
            base: base.clone(),
            http: build_http(Duration::from_millis(200), Duration::from_millis(300)).unwrap(),
            antibot: Arc::default(),
        };
        let started = std::time::Instant::now();
        let err = api.get_session(TOKEN).await.unwrap_err();
        assert!(matches!(err, SyncError::Network), "got {err:?}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    fn youtube_req() -> EntryAddRequest {
        EntryAddRequest {
            expected_version: 2,
            provider: Provider::YouTube,
            provider_track_id: "abc123".into(),
            provider_url: "https://www.youtube.com/watch?v=abc123".into(),
            title: Some("Song".into()),
            artists: Some(vec!["Band".into()]),
            artwork_url: Some("https://img.example.test/a.jpg".into()),
            duration_ms: Some(215_000),
        }
    }

    fn spotify_req() -> EntryAddRequest {
        EntryAddRequest {
            expected_version: 2,
            provider: Provider::Spotify,
            provider_track_id: "4uLU6hMCjMI75M1A2tAKUQ".into(),
            provider_url: "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tAKUQ".into(),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
        }
    }

    fn json(text: &str) -> serde_json::Value {
        serde_json::from_str(text).unwrap()
    }

    #[tokio::test]
    async fn login_posts_credentials_and_returns_token() {
        let body = format!(r#"{{"token":"{TOKEN}","expiresAt":"2026-11-18T00:00:00Z"}}"#);
        let s = stub(raw("200 OK", "", &body)).await;
        let res = client(&s.base)
            .login("a@example.test", "correct horse battery")
            .await
            .unwrap();
        assert_eq!(res.token, TOKEN);
        assert_eq!(res.expires_at, "2026-11-18T00:00:00Z");
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/v1/auth/login");
        assert_eq!(req.auth, None);
        assert_eq!(
            json(&req.body),
            serde_json::json!({"email": "a@example.test", "password": "correct horse battery"})
        );
    }

    #[tokio::test]
    async fn antibot_tokens_are_sent_once_each_in_order_and_never_reused() {
        let body = format!(r#"{{"token":"{TOKEN}","expiresAt":"2026-11-18T00:00:00Z"}}"#);
        let s = stub(raw("200 OK", "", &body)).await;
        let api = client(&s.base);
        api.set_antibot(vec!["first".into(), "second".into()]);
        api.login("a@example.test", "pw").await.unwrap();
        assert_eq!(json(&s.last().body)["turnstileToken"], "first");
        api.login("a@example.test", "pw").await.unwrap();
        assert_eq!(json(&s.last().body)["turnstileToken"], "second");
        api.login("a@example.test", "pw").await.unwrap();
        assert!(json(&s.last().body).get("turnstileToken").is_none());
    }

    #[tokio::test]
    async fn register_posts_credentials_without_a_token() {
        let s = stub(raw(
            "201 Created",
            "",
            r#"{"id":"5c8a0f2e-3d7b-4e1a-9f6c-2b1d0e4a7c33"}"#,
        ))
        .await;
        client(&s.base)
            .register("a@example.test", "correct horse battery")
            .await
            .unwrap();
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/v1/auth/register");
        assert_eq!(req.auth, None);
        assert_eq!(
            json(&req.body),
            serde_json::json!({"email": "a@example.test", "password": "correct horse battery"})
        );
    }

    #[tokio::test]
    async fn create_share_posts_with_bearer_and_reads_token() {
        let body = r#"{"id":"5c8a0f2e-3d7b-4e1a-9f6c-2b1d0e4a7c33","token":"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQ","createdAt":"2026-10-23T00:00:00Z"}"#;
        let s = stub(raw("201 Created", "", body)).await;
        let created = client(&s.base).create_share(TOKEN, PLAYLIST).await.unwrap();
        assert_eq!(created.token.len(), 43);
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, format!("/api/v1/playlists/{PLAYLIST}/shares"));
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
    }

    #[tokio::test]
    async fn revoke_share_deletes_that_share_only() {
        let s = stub(raw("204 No Content", "", "")).await;
        client(&s.base)
            .revoke_share(TOKEN, PLAYLIST, ENTRY)
            .await
            .unwrap();
        let req = s.last();
        assert_eq!(req.method, "DELETE");
        assert_eq!(
            req.path,
            format!("/api/v1/playlists/{PLAYLIST}/shares/{ENTRY}")
        );
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
    }

    #[tokio::test]
    async fn share_url_points_at_the_shared_endpoint() {
        let c = client("https://backend.example.test");
        assert_eq!(
            c.share_url("tok"),
            "https://backend.example.test/api/v1/shared/tok"
        );
    }

    #[tokio::test]
    async fn add_youtube_entry_sends_bearer_and_metadata() {
        let body = format!(
            r#"{{"id":"{ENTRY}","provider":"YouTube","providerTrackId":"abc123","providerUrl":"https://www.youtube.com/watch?v=abc123","title":"Song","artists":["Band"],"artworkUrl":"https://img.example.test/a.jpg","durationMs":215000,"position":0,"createdAt":"2026-10-19T00:00:00Z","version":3}}"#
        );
        let s = stub(raw("201 Created", "", &body)).await;
        let added = client(&s.base)
            .add_entry(TOKEN, PLAYLIST, &youtube_req())
            .await
            .unwrap();
        assert_eq!(added.version, 3);
        assert_eq!(added.entry.id, ENTRY);
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, format!("/api/v1/playlists/{PLAYLIST}/entries"));
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
        assert_eq!(
            json(&req.body),
            serde_json::json!({
                "expectedVersion": 2,
                "provider": "YouTube",
                "providerTrackId": "abc123",
                "providerUrl": "https://www.youtube.com/watch?v=abc123",
                "title": "Song",
                "artists": ["Band"],
                "artworkUrl": "https://img.example.test/a.jpg",
                "durationMs": 215000
            })
        );
    }

    #[tokio::test]
    async fn add_spotify_entry_sends_ids_only() {
        let body = format!(
            r#"{{"id":"{ENTRY}","provider":"Spotify","providerTrackId":"4uLU6hMCjMI75M1A2tAKUQ","providerUrl":"https://open.spotify.com/track/4uLU6hMCjMI75M1A2tAKUQ","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z","version":3}}"#
        );
        let s = stub(raw("201 Created", "", &body)).await;
        client(&s.base)
            .add_entry(TOKEN, PLAYLIST, &spotify_req())
            .await
            .unwrap();
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, format!("/api/v1/playlists/{PLAYLIST}/entries"));
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
        assert_eq!(
            json(&req.body),
            serde_json::json!({
                "expectedVersion": 2,
                "provider": "Spotify",
                "providerTrackId": "4uLU6hMCjMI75M1A2tAKUQ",
                "providerUrl": "https://open.spotify.com/track/4uLU6hMCjMI75M1A2tAKUQ",
                "title": null,
                "artists": null,
                "artworkUrl": null,
                "durationMs": null
            })
        );
    }

    #[tokio::test]
    async fn spotify_metadata_is_refused_before_any_request() {
        let cases: [fn(&mut EntryAddRequest); 4] = [
            |r| r.title = Some("Song".into()),
            |r| r.artists = Some(vec!["Band".into()]),
            |r| r.artwork_url = Some("https://img.example.test/a.jpg".into()),
            |r| r.duration_ms = Some(1000),
        ];
        for set in cases {
            let s = stub(raw("201 Created", "", "{}")).await;
            let mut req = spotify_req();
            set(&mut req);
            let err = client(&s.base)
                .add_entry(TOKEN, PLAYLIST, &req)
                .await
                .unwrap_err();
            assert!(matches!(err, SyncError::Local(_)), "{err:?}");
            assert_eq!(s.count(), 0, "a request was sent for Spotify metadata");
        }
    }

    #[tokio::test]
    async fn reorder_sends_put_with_expected_version_and_ids() {
        let s = stub(raw("200 OK", "", r#"{"version":8}"#)).await;
        let res = client(&s.base)
            .reorder_entries(TOKEN, PLAYLIST, 7, &[ENTRY.into(), ENTRY2.into()])
            .await
            .unwrap();
        assert_eq!(res.version, 8);
        let req = s.last();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.path, format!("/api/v1/playlists/{PLAYLIST}/order"));
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
        assert_eq!(
            json(&req.body),
            serde_json::json!({"expectedVersion": 7, "entryIds": [ENTRY, ENTRY2]})
        );
    }

    async fn error_for(status: &str, headers: &str, body: &str) -> SyncError {
        let s = stub(raw(status, headers, body)).await;
        client(&s.base).get_session(TOKEN).await.unwrap_err()
    }

    #[tokio::test]
    async fn status_401_maps_to_unauthenticated() {
        let body = r#"{"error":{"code":"UNAUTHENTICATED","message":"Authentication required."}}"#;
        let err = error_for("401 Unauthorized", "", body).await;
        assert!(matches!(err, SyncError::Unauthenticated(_)), "{err:?}");
    }

    #[tokio::test]
    async fn status_403_maps_to_forbidden() {
        let body = r#"{"error":{"code":"FORBIDDEN","message":"Forbidden."}}"#;
        let err = error_for("403 Forbidden", "", body).await;
        assert!(matches!(err, SyncError::Forbidden(_)), "{err:?}");
    }

    #[tokio::test]
    async fn status_400_maps_to_validation() {
        let body = r#"{"error":{"code":"VALIDATION_FAILED","message":"invalid request body"}}"#;
        let err = error_for("400 Bad Request", "", body).await;
        assert!(matches!(err, SyncError::Validation(_)), "{err:?}");
    }

    #[tokio::test]
    async fn status_404_maps_to_not_found() {
        let body = r#"{"error":{"code":"PLAYLIST_NOT_FOUND","message":"Playlist not found."}}"#;
        let err = error_for("404 Not Found", "", body).await;
        assert!(matches!(err, SyncError::NotFound(_)), "{err:?}");
    }

    #[tokio::test]
    async fn status_409_maps_to_conflict_with_current_version() {
        let body = r#"{"error":{"code":"VERSION_CONFLICT","message":"Playlist was changed.","details":{"currentVersion":7}}}"#;
        let err = error_for("409 Conflict", "", body).await;
        assert!(
            matches!(
                err,
                SyncError::Conflict {
                    current_version: Some(7),
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn status_429_maps_to_rate_limited_with_retry_after() {
        let body = r#"{"error":{"code":"RATE_LIMITED","message":"Too many requests."}}"#;
        let err = error_for("429 Too Many Requests", "Retry-After: 12\r\n", body).await;
        assert!(
            matches!(
                err,
                SyncError::RateLimited {
                    retry_after: Some(12),
                    ..
                }
            ),
            "{err:?}"
        );
    }

    #[tokio::test]
    async fn status_500_maps_to_internal() {
        let body = r#"{"error":{"code":"INTERNAL_ERROR","message":"Internal error."}}"#;
        let err = error_for("500 Internal Server Error", "", body).await;
        assert!(matches!(err, SyncError::Internal(_)), "{err:?}");
    }

    #[tokio::test]
    async fn connection_refused_maps_to_network() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let err = client(&format!("http://127.0.0.1:{port}"))
            .get_session(TOKEN)
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Network), "{err:?}");
    }

    #[tokio::test]
    async fn traversal_ids_are_refused_before_any_request() {
        // Unreachable port: a request would fail with Network, so Internal proves no request was sent.
        let c = client("http://127.0.0.1:9");
        assert!(matches!(
            c.get_playlist(TOKEN, "../../admin").await,
            Err(SyncError::Internal(_))
        ));
        assert!(matches!(
            c.remove_entry(TOKEN, PLAYLIST, "..%2Fx", 1).await,
            Err(SyncError::Internal(_))
        ));
        assert!(path_id(PLAYLIST).is_ok());
        assert!(path_id(ENTRY).is_ok());
    }

    #[tokio::test]
    async fn likes_are_listed_read_and_written_with_the_key_in_the_path() {
        let snapshot = r#"{"revision":4,"tracks":[{"provider":"YouTube","providerTrackId":"r-G2pKr2_n8","canonicalUrl":"https://www.youtube.com/watch?v=r-G2pKr2_n8","title":"BU","artists":["A"],"artistRefs":[{"provider":"YouTube","id":"UCuAXFkgsw1L7xaCfnd5JJOw","name":"A"}],"source":"YouTube Music","artworkUrl":null,"durationMs":166000}]}"#;
        let s = stub(raw("200 OK", "", snapshot)).await;
        let listed = client(&s.base).list_likes(TOKEN).await.unwrap();
        assert_eq!(listed.revision, 4);
        assert_eq!(listed.tracks[0].title, "BU");
        assert_eq!(listed.tracks[0].source.as_deref(), Some("YouTube Music"));
        let req = s.last();
        assert_eq!(req.method, "GET");
        assert_eq!(req.path, "/api/v1/liked-songs");
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));

        let s = stub(raw("200 OK", "", r#"{"liked":false}"#)).await;
        let entry = client(&s.base)
            .get_like(TOKEN, "YouTube", "r-G2pKr2_n8")
            .await
            .unwrap();
        assert!(!entry.liked);
        assert!(entry.track.is_none());
        assert_eq!(s.last().path, "/api/v1/liked-songs/YouTube/r-G2pKr2_n8");

        let s = stub(raw("204 No Content", "", "")).await;
        let artists = ["A".to_string()];
        let refs = vec![LikeArtistRef {
            provider: "YouTube".into(),
            id: Some("UCuAXFkgsw1L7xaCfnd5JJOw".into()),
            name: "A".into(),
        }];
        client(&s.base)
            .put_like(
                TOKEN,
                "YouTube",
                "r-G2pKr2_n8",
                &LikeTrackBody {
                    canonical_url: "https://www.youtube.com/watch?v=r-G2pKr2_n8",
                    title: "BU",
                    artists: &artists,
                    artist_refs: &refs,
                    source: Some("YouTube Music"),
                    artwork_url: None,
                    duration_ms: Some(166_000),
                },
            )
            .await
            .unwrap();
        let req = s.last();
        assert_eq!(req.method, "PUT");
        assert_eq!(req.path, "/api/v1/liked-songs/YouTube/r-G2pKr2_n8");
        assert_eq!(req.auth, Some(format!("Bearer {TOKEN}")));
        let body = json(&req.body);
        assert_eq!(body["title"], "BU");
        assert!(body.get("provider").is_none());
        assert!(body.get("providerTrackId").is_none());

        let s = stub(raw("204 No Content", "", "")).await;
        client(&s.base)
            .delete_like(TOKEN, "YouTube", "r-G2pKr2_n8")
            .await
            .unwrap();
        assert_eq!(s.last().method, "DELETE");
    }

    #[tokio::test]
    async fn merge_posts_the_tracks_and_reads_the_revision_and_count() {
        let s = stub(raw("200 OK", "", r#"{"revision":9,"added":2}"#)).await;
        let tracks = vec![LikeTrack {
            provider: "YouTube".into(),
            provider_track_id: "r-G2pKr2_n8".into(),
            canonical_url: "https://www.youtube.com/watch?v=r-G2pKr2_n8".into(),
            title: "BU".into(),
            artists: vec!["A".into()],
            artist_refs: vec![],
            source: None,
            artwork_url: None,
            duration_ms: None,
        }];
        let merged = client(&s.base).merge_likes(TOKEN, &tracks).await.unwrap();
        assert_eq!(merged.revision, 9);
        assert_eq!(merged.added, 2);
        let req = s.last();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/v1/liked-songs/merge");
        assert_eq!(
            json(&req.body)["tracks"][0]["providerTrackId"],
            "r-G2pKr2_n8"
        );
    }

    #[tokio::test]
    async fn a_missing_like_is_not_an_error_and_a_bad_provider_never_reaches_the_network() {
        let s = stub(raw("200 OK", "", r#"{"liked":false}"#)).await;
        assert!(
            !client(&s.base)
                .get_like(TOKEN, "YouTube", "r-G2pKr2_n8")
                .await
                .unwrap()
                .liked
        );
        // Unreachable port: a request would fail with Network, so Internal proves no request was sent.
        let c = client("http://127.0.0.1:9");
        assert!(matches!(
            c.get_like(TOKEN, "YouTube Music", "id").await,
            Err(SyncError::Internal(_))
        ));
    }

    #[test]
    fn new_rejects_invalid_base_url() {
        assert!(matches!(
            ApiClient::new("ftp://example.test"),
            Err(SyncError::Settings(_))
        ));
    }
}
