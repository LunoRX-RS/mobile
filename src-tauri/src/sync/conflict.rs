//! Whole-playlist conflict resolution (spec 4.3). Nothing is written unless the caller passes a Resolution.

use super::api::{ApiClient, Entry, Playlist, SyncError};
use super::queue::OfflineQueue;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
    KeepServer,
    KeepMine,
}

/// `entries` is set only for KeepServer, the one case where the caller replaces its local copy.
#[derive(Debug)]
pub struct ServerPlaylist {
    pub playlist: Playlist,
    pub entries: Option<Vec<Entry>>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConflictError {
    #[error("no conflict is pending for this playlist")]
    NoConflictPending,
    #[error(transparent)]
    Sync(#[from] SyncError),
}

/// Gated on a pending conflict: makes no request unless one is pending. `created` receives the server ids of
/// replayed adds (see `OfflineQueue::replay`).
pub async fn resolve(
    api: &ApiClient,
    token: &str,
    queue: &mut OfflineQueue,
    playlist_id: &str,
    resolution: Resolution,
    created: &mut Vec<String>,
) -> Result<ServerPlaylist, ConflictError> {
    if !queue.conflict_pending(playlist_id) {
        return Err(ConflictError::NoConflictPending);
    }
    let playlist = api.get_playlist(token, playlist_id).await?;
    match resolution {
        Resolution::KeepServer => {
            // Fetch before discarding so a failed fetch leaves the queue and flag intact.
            let entries = api.list_entries(token, playlist_id).await?;
            queue.discard(playlist_id);
            Ok(ServerPlaylist {
                playlist,
                entries: Some(entries),
            })
        }
        Resolution::KeepMine => {
            // Each replayed write bumps the server version once. A replay that succeeded is complete, so if the
            // re-read fails the version is the pre-replay one plus that count; a stale version would conflict.
            let replayed = queue.queued(playlist_id).len() as i64;
            queue.rebase(playlist_id, playlist.version);
            queue.replay(api, token, playlist_id, created).await?;
            let playlist = match api.get_playlist(token, playlist_id).await {
                Ok(fresh) => fresh,
                Err(_) => Playlist {
                    version: playlist.version + replayed,
                    ..playlist
                },
            };
            Ok(ServerPlaylist {
                playlist,
                entries: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::api::{EntryAddRequest, Provider, SyncError};
    use crate::sync::local_store::LocalEntry;
    use crate::sync::queue::WriteOp;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const SENTINEL: &str = "SENTINEL-CONFLICT-TOKEN-7c3b";
    const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    const ENTRY_A: &str = "6f1d2c3a-0000-4000-8000-000000000002";
    const ENTRY_B: &str = "6f1d2c3a-0000-4000-8000-000000000003";
    const CONFLICT: &str = r#"{"error":{"code":"VERSION_CONFLICT","message":"Playlist was changed.","details":{"currentVersion":9}}}"#;
    const NOT_FOUND: &str =
        r#"{"error":{"code":"PLAYLIST_NOT_FOUND","message":"Playlist not found."}}"#;

    struct Recorded {
        line: String,
        body: serde_json::Value,
    }

    struct Stub {
        base: String,
        seen: Arc<Mutex<Vec<Recorded>>>,
    }

    impl Stub {
        fn count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }

        fn lines(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.line.clone())
                .collect()
        }

        fn body(&self, i: usize) -> serde_json::Value {
            self.seen.lock().unwrap()[i].body.clone()
        }
    }

    /// Answers request i with responses[i] (the last response repeats) and records each request.
    async fn stub(responses: Vec<String>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            let mut i = 0;
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let raw = read_request(&mut sock).await;
                log.lock().unwrap().push(parse_request(&raw));
                let res = &responses[i.min(responses.len() - 1)];
                i += 1;
                let _ = sock.write_all(res.as_bytes()).await;
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
        Recorded {
            line: head.lines().next().unwrap_or("").to_string(),
            body: serde_json::from_str(body).unwrap_or(serde_json::Value::Null),
        }
    }

    fn raw(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn ok(body: &str) -> String {
        raw("200 OK", body)
    }

    fn no_content() -> String {
        raw("204 No Content", "")
    }

    fn conflict() -> String {
        raw("409 Conflict", CONFLICT)
    }

    fn playlist_json(version: i64) -> String {
        format!(
            r#"{{"id":"{PLAYLIST}","name":"Mix","description":null,"version":{version},"createdAt":"2026-10-19T00:00:00Z","updatedAt":"2026-10-19T00:00:00Z"}}"#
        )
    }

    fn entries_json() -> String {
        format!(
            r#"[{{"id":"{ENTRY_B}","provider":"Spotify","providerTrackId":"track-s","providerUrl":"https://open.spotify.com/track/track-s","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z"}}]"#
        )
    }

    fn added(track: &str, version: i64) -> String {
        raw(
            "201 Created",
            &format!(
                r#"{{"id":"e-{track}","provider":"Spotify","providerTrackId":"{track}","providerUrl":"https://open.spotify.com/track/{track}","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z","version":{version}}}"#
            ),
        )
    }

    fn client(base: &str) -> ApiClient {
        ApiClient::new(base).unwrap()
    }

    fn add_op(track: &str, v: i64) -> WriteOp {
        WriteOp::AddEntry(EntryAddRequest {
            expected_version: v,
            provider: Provider::Spotify,
            provider_track_id: track.into(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
        })
    }

    fn remove_op(entry_id: &str, v: i64) -> WriteOp {
        WriteOp::RemoveEntry {
            entry_id: entry_id.into(),
            expected_version: v,
            index: 0,
            attempted: false,
            entry: LocalEntry {
                provider: Provider::Spotify,
                provider_track_id: "track-removed".into(),
                provider_url: "https://open.spotify.com/track/track-removed".into(),
                title: None,
                artists: None,
                artwork_url: None,
                duration_ms: None,
                remote_entry_id: Some(entry_id.into()),
            },
        }
    }

    fn reorder_op(v: i64) -> WriteOp {
        WriteOp::Reorder {
            entry_ids: vec![ENTRY_A.into(), ENTRY_B.into()],
            expected_version: v,
        }
    }

    /// Queues remove A, add track-a and reorder at version 7, then replays into a 409 (first response).
    async fn conflicted_queue(api: &ApiClient) -> OfflineQueue {
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        q.push(PLAYLIST, add_op("track-a", 7)).unwrap();
        q.push(PLAYLIST, reorder_op(7)).unwrap();
        assert!(
            q.replay(api, SENTINEL, PLAYLIST, &mut Vec::new())
                .await
                .is_err()
        );
        assert!(q.conflict_pending(PLAYLIST));
        q
    }

    #[tokio::test]
    async fn no_conflict_pending_makes_zero_requests_and_errors() {
        let s = stub(vec![ok(&playlist_json(9))]).await;
        let api = client(&s.base);
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        for resolution in [Resolution::KeepServer, Resolution::KeepMine] {
            let err = resolve(
                &api,
                SENTINEL,
                &mut q,
                PLAYLIST,
                resolution,
                &mut Vec::new(),
            )
            .await
            .unwrap_err();
            assert!(matches!(err, ConflictError::NoConflictPending), "{err:?}");
        }
        assert_eq!(s.count(), 0);
        assert_eq!(q.queued(PLAYLIST).len(), 1);
    }

    #[tokio::test]
    async fn keep_server_sends_only_gets_and_clears_the_queue() {
        let s = stub(vec![conflict(), ok(&playlist_json(9)), ok(&entries_json())]).await;
        let api = client(&s.base);
        let mut q = conflicted_queue(&api).await;
        let server = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepServer,
            &mut Vec::new(),
        )
        .await
        .unwrap();
        assert_eq!(server.playlist.version, 9);
        assert_eq!(server.entries.as_ref().map(Vec::len), Some(1));
        assert_eq!(
            s.lines(),
            vec![
                format!(
                    "DELETE /api/v1/playlists/{PLAYLIST}/entries/{ENTRY_A}?expectedVersion=7 HTTP/1.1"
                ),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
                format!("GET /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ],
            "only the setup write and two GETs; no write after the resolution"
        );
        assert!(q.queued(PLAYLIST).is_empty());
        assert!(!q.conflict_pending(PLAYLIST));
    }

    #[tokio::test]
    async fn keep_mine_rebases_then_resends_with_server_version() {
        let s = stub(vec![
            conflict(),
            ok(&playlist_json(9)),
            no_content(),
            added("track-a", 10),
            ok(r#"{"version":10}"#),
            ok(&playlist_json(12)),
        ])
        .await;
        let api = client(&s.base);
        let mut q = conflicted_queue(&api).await;
        let server = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepMine,
            &mut Vec::new(),
        )
        .await
        .unwrap();
        // The version is read again after the replay, so it is the server's current one, not the pre-replay 9.
        assert_eq!(server.playlist.version, 12);
        assert!(server.entries.is_none());
        assert_eq!(
            s.lines(),
            vec![
                format!(
                    "DELETE /api/v1/playlists/{PLAYLIST}/entries/{ENTRY_A}?expectedVersion=7 HTTP/1.1"
                ),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
                format!(
                    "DELETE /api/v1/playlists/{PLAYLIST}/entries/{ENTRY_A}?expectedVersion=9 HTTP/1.1"
                ),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
                format!("PUT /api/v1/playlists/{PLAYLIST}/order HTTP/1.1"),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
            ]
        );
        assert_eq!(s.body(3)["expectedVersion"], 10);
        assert_eq!(
            s.body(4),
            serde_json::json!({"expectedVersion": 11, "entryIds": [ENTRY_A, ENTRY_B]})
        );
        assert!(q.queued(PLAYLIST).is_empty());
        assert!(!q.conflict_pending(PLAYLIST));
    }

    #[tokio::test]
    async fn keep_mine_stops_at_a_further_conflict_and_keeps_the_rest_queued() {
        let s = stub(vec![
            conflict(),
            ok(&playlist_json(9)),
            no_content(),
            conflict(),
        ])
        .await;
        let api = client(&s.base);
        let mut q = conflicted_queue(&api).await;
        let err = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepMine,
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        assert!(
            matches!(
                err,
                ConflictError::Sync(SyncError::Conflict {
                    current_version: Some(9),
                    ..
                })
            ),
            "{err:?}"
        );
        assert_eq!(
            s.count(),
            4,
            "the reorder must not be sent after the second conflict"
        );
        assert!(q.conflict_pending(PLAYLIST));
        let rest = q.queued(PLAYLIST);
        assert_eq!(rest.len(), 2);
        assert!(matches!(rest[0], WriteOp::AddEntry(b) if b.expected_version == 10));
        assert!(matches!(
            rest[1],
            WriteOp::Reorder {
                expected_version: 11,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn token_never_appears_in_results_or_errors() {
        let s = stub(vec![
            conflict(),
            raw("404 Not Found", NOT_FOUND),
            ok(&playlist_json(9)),
            ok(&entries_json()),
        ])
        .await;
        let api = client(&s.base);
        let mut q = conflicted_queue(&api).await;
        let e1 = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepServer,
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        let r2 = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepServer,
            &mut Vec::new(),
        )
        .await
        .unwrap();
        let e3 = resolve(
            &api,
            SENTINEL,
            &mut q,
            PLAYLIST,
            Resolution::KeepServer,
            &mut Vec::new(),
        )
        .await
        .unwrap_err();
        let out = format!(
            "{e1:?} {e1} {r2:?} {e3:?} {e3} {:?} {:?}",
            Resolution::KeepMine,
            Resolution::KeepServer
        );
        assert!(matches!(e1, ConflictError::Sync(SyncError::NotFound(_))));
        assert!(!out.contains(SENTINEL), "{out}");
    }
}
