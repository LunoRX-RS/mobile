//! In-memory per-playlist offline write queue (spec 4.3, decision Q2). Persisted through persist.rs.

use std::collections::{HashMap, HashSet, VecDeque};

use super::api::{ApiClient, EntryAddRequest, SyncError};
use super::local_store::LocalEntry;

/// Q2: 500 writes per playlist. A full queue rejects the newest write; nothing is evicted.
pub const CAPACITY: usize = 500;

/// Each write carries the expectedVersion it was made against (inside `EntryAddRequest` for adds).
#[derive(Debug)]
pub enum WriteOp {
    AddEntry(EntryAddRequest),
    /// `index` and `entry` let a discard put the removed entry back where it was.
    RemoveEntry {
        entry_id: String,
        expected_version: i64,
        index: usize,
        entry: LocalEntry,
        /// Set after a network or server error: the server may have applied the removal, so a discard re-reads the server.
        attempted: bool,
    },
    Reorder {
        entry_ids: Vec<String>,
        expected_version: i64,
    },
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    #[error(
        "offline queue is full for this playlist (500 writes); the newest write was not queued"
    )]
    Full,
}

#[derive(Debug, Default)]
pub struct OfflineQueue {
    queues: HashMap<String, VecDeque<WriteOp>>,
    /// Playlists whose replay hit VERSION_CONFLICT and have not been resolved yet.
    conflicted: HashSet<String>,
    /// Bumped whenever queued writes are dropped. A request sent before a drop must not pop a later write.
    epoch: u64,
}

impl WriteOp {
    /// Owned copy for a request that is sent while the write stays queued.
    fn duplicate(&self) -> WriteOp {
        match self {
            WriteOp::AddEntry(b) => WriteOp::AddEntry(EntryAddRequest {
                expected_version: b.expected_version,
                provider: b.provider,
                provider_track_id: b.provider_track_id.clone(),
                provider_url: b.provider_url.clone(),
                title: b.title.clone(),
                artists: b.artists.clone(),
                artwork_url: b.artwork_url.clone(),
                duration_ms: b.duration_ms,
            }),
            WriteOp::RemoveEntry {
                entry_id,
                expected_version,
                index,
                entry,
                attempted,
            } => WriteOp::RemoveEntry {
                entry_id: entry_id.clone(),
                expected_version: *expected_version,
                index: *index,
                entry: entry.clone(),
                attempted: *attempted,
            },
            WriteOp::Reorder {
                entry_ids,
                expected_version,
            } => WriteOp::Reorder {
                entry_ids: entry_ids.clone(),
                expected_version: *expected_version,
            },
        }
    }

    /// Returns the server entry id for an accepted add; removes and reorders have none.
    pub async fn send(
        &self,
        api: &ApiClient,
        token: &str,
        playlist_id: &str,
    ) -> Result<Option<String>, SyncError> {
        match self {
            WriteOp::AddEntry(body) => api
                .add_entry(token, playlist_id, body)
                .await
                .map(|added| Some(added.entry.id)),
            WriteOp::RemoveEntry {
                entry_id,
                expected_version,
                ..
            } => api
                .remove_entry(token, playlist_id, entry_id, *expected_version)
                .await
                .map(|()| None),
            WriteOp::Reorder {
                entry_ids,
                expected_version,
            } => api
                .reorder_entries(token, playlist_id, *expected_version, entry_ids)
                .await
                .map(|_| None),
        }
    }
}

impl OfflineQueue {
    /// Restore path for persist::restore: takes the snapshot's queues and conflict flags as they were saved.
    pub fn from_parts(
        queues: HashMap<String, VecDeque<WriteOp>>,
        conflicted: HashSet<String>,
    ) -> Self {
        Self {
            queues,
            conflicted,
            epoch: 0,
        }
    }

    /// Total queued writes across all playlists.
    pub fn pending(&self) -> usize {
        self.queues.values().map(VecDeque::len).sum()
    }

    /// Drops every queued write of every playlist and every conflict flag. Returns the dropped writes with their
    /// remote playlist id, oldest first within each playlist, so the store can undo them.
    pub fn discard_all(&mut self) -> Vec<(String, WriteOp)> {
        self.epoch += 1;
        self.conflicted.clear();
        self.queues
            .drain()
            .flat_map(|(remote, q)| q.into_iter().map(move |op| (remote.clone(), op)))
            .collect()
    }

    /// Local only: no request is made. Full queues reject the new write and keep all existing ones.
    pub fn push(&mut self, playlist_id: &str, op: WriteOp) -> Result<(), QueueError> {
        let q = self.queues.entry(playlist_id.to_string()).or_default();
        if q.len() >= CAPACITY {
            return Err(QueueError::Full);
        }
        q.push_back(op);
        Ok(())
    }

    pub fn queued(&self, playlist_id: &str) -> Vec<&WriteOp> {
        self.queues
            .get(playlist_id)
            .map(|q| q.iter().collect())
            .unwrap_or_default()
    }

    pub fn conflict_pending(&self, playlist_id: &str) -> bool {
        self.conflicted.contains(playlist_id)
    }

    /// Read-only views for persist::capture.
    pub fn queues(&self) -> &HashMap<String, VecDeque<WriteOp>> {
        &self.queues
    }

    pub fn conflicted(&self) -> &HashSet<String> {
        &self.conflicted
    }

    /// Rewrites expectedVersion on every queued write of this playlist. Each replayed write bumps the server
    /// version, so the chain starts at `server_version` and steps by one.
    pub fn rebase(&mut self, playlist_id: &str, server_version: i64) {
        let Some(q) = self.queues.get_mut(playlist_id) else {
            return;
        };
        for (i, op) in q.iter_mut().enumerate() {
            let v = server_version + i as i64;
            match op {
                WriteOp::AddEntry(body) => body.expected_version = v,
                WriteOp::RemoveEntry {
                    expected_version, ..
                }
                | WriteOp::Reorder {
                    expected_version, ..
                } => *expected_version = v,
            }
        }
    }

    /// Flags a playlist whose upload hit VERSION_CONFLICT, so resolve_conflict handles it like a replay conflict.
    pub fn mark_conflict(&mut self, playlist_id: &str) {
        self.conflicted.insert(playlist_id.to_string());
    }

    /// Drops every queued write for this playlist and clears its conflict flag.
    pub fn discard(&mut self, playlist_id: &str) {
        self.epoch += 1;
        self.queues.remove(playlist_id);
        self.conflicted.remove(playlist_id);
    }

    /// Token for the write `front_op` returns; pass it to `finish_front` or `fail_front`.
    pub fn ticket(&self) -> u64 {
        self.epoch
    }

    /// Copy of the oldest queued write, to send with the queue unlocked. The write stays queued until `finish_front`.
    pub fn front_op(&self, playlist_id: &str) -> Option<WriteOp> {
        self.queues
            .get(playlist_id)
            .and_then(|q| q.front())
            .map(WriteOp::duplicate)
    }

    /// The write from `front_op` was accepted: removes it. False when the queue was dropped since `ticket`,
    /// so the write is not removed; the caller must re-read the server.
    pub fn finish_front(&mut self, playlist_id: &str, ticket: u64) -> bool {
        if ticket != self.epoch {
            return false;
        }
        let Some(q) = self.queues.get_mut(playlist_id) else {
            return false;
        };
        if q.pop_front().is_none() {
            return false;
        }
        if q.is_empty() {
            self.conflicted.remove(playlist_id);
        }
        true
    }

    /// Called when a replay of this playlist ran to the end: its conflict flag is settled.
    pub fn complete_replay(&mut self, playlist_id: &str) {
        self.conflicted.remove(playlist_id);
    }

    /// The write from `front_op` failed: keeps it queued and records a conflict or a possibly applied removal.
    pub fn fail_front(&mut self, playlist_id: &str, ticket: u64, err: &SyncError) {
        if ticket != self.epoch {
            return;
        }
        if matches!(err, SyncError::Conflict { .. }) {
            self.conflicted.insert(playlist_id.to_string());
        }
        // A network error or 5xx (Internal) may follow a removal the server applied: record the attempt, keep the write queued.
        if let (
            SyncError::Network | SyncError::Internal(_),
            Some(WriteOp::RemoveEntry { attempted, .. }),
        ) = (
            err,
            self.queues.get_mut(playlist_id).and_then(|q| q.front_mut()),
        ) {
            *attempted = true;
        }
    }

    /// Sequential replay: sends oldest-first, popping each accepted write. Any error keeps that write and every
    /// later one queued. `created` receives the server id of each accepted add.
    pub async fn replay(
        &mut self,
        api: &ApiClient,
        token: &str,
        playlist_id: &str,
        created: &mut Vec<String>,
    ) -> Result<(), SyncError> {
        while let Some(op) = self.front_op(playlist_id) {
            let ticket = self.ticket();
            match op.send(api, token, playlist_id).await {
                Ok(id) => {
                    created.extend(id);
                    self.finish_front(playlist_id, ticket);
                }
                Err(err) => {
                    self.fail_front(playlist_id, ticket, &err);
                    return Err(err);
                }
            }
        }
        self.complete_replay(playlist_id);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::api::{ApiClient, EntryAddRequest, Provider, SyncError};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const SENTINEL: &str = "SENTINEL-QUEUE-TOKEN-9a2f";
    const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    const ENTRY_A: &str = "6f1d2c3a-0000-4000-8000-000000000002";
    const ENTRY_B: &str = "6f1d2c3a-0000-4000-8000-000000000003";
    const CONFLICT: &str = r#"{"error":{"code":"VERSION_CONFLICT","message":"Playlist was changed.","details":{"currentVersion":9}}}"#;
    const INTERNAL: &str = r#"{"error":{"code":"INTERNAL_ERROR","message":"Internal error."}}"#;

    struct Recorded {
        line: String,
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
        let head = raw.split_once("\r\n\r\n").map_or(raw, |(h, _)| h);
        Recorded {
            line: head.lines().next().unwrap_or("").to_string(),
        }
    }

    fn raw(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
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

    fn add_op(track: &str, expected_version: i64) -> WriteOp {
        WriteOp::AddEntry(EntryAddRequest {
            expected_version,
            provider: Provider::Spotify,
            provider_track_id: track.into(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
        })
    }

    fn remove_op(entry_id: &str, expected_version: i64) -> WriteOp {
        WriteOp::RemoveEntry {
            entry_id: entry_id.into(),
            expected_version,
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

    fn reorder_op(expected_version: i64) -> WriteOp {
        WriteOp::Reorder {
            entry_ids: vec![ENTRY_A.into(), ENTRY_B.into()],
            expected_version,
        }
    }

    /// Track id for adds, entry id for removes, "reorder" for reorders.
    fn labels(q: &OfflineQueue, playlist_id: &str) -> Vec<String> {
        q.queued(playlist_id)
            .into_iter()
            .map(|op| match op {
                WriteOp::AddEntry(b) => b.provider_track_id.clone(),
                WriteOp::RemoveEntry { entry_id, .. } => entry_id.clone(),
                WriteOp::Reorder { .. } => "reorder".into(),
            })
            .collect()
    }

    #[test]
    fn push_accepts_500_and_rejects_501st_keeping_existing_intact() {
        let mut q = OfflineQueue::default();
        for i in 0..CAPACITY {
            q.push(PLAYLIST, add_op(&format!("track-{i}"), 1)).unwrap();
        }
        let expected: Vec<String> = (0..CAPACITY).map(|i| format!("track-{i}")).collect();
        assert_eq!(labels(&q, PLAYLIST), expected);
        assert_eq!(
            q.push(PLAYLIST, add_op("track-new", 1)),
            Err(QueueError::Full)
        );
        assert_eq!(labels(&q, PLAYLIST), expected);
    }

    #[tokio::test]
    async fn push_makes_zero_requests() {
        let s = stub(vec![raw("201 Created", "{}")]).await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, add_op("track-a", 1)).unwrap();
        q.push(PLAYLIST, remove_op(ENTRY_A, 1)).unwrap();
        q.push(PLAYLIST, reorder_op(1)).unwrap();
        assert_eq!(s.count(), 0);
        assert_eq!(q.queued(PLAYLIST).len(), 3);
        // Each write keeps the expectedVersion it was made against.
        assert!(matches!(
            q.queued(PLAYLIST)[1],
            WriteOp::RemoveEntry {
                expected_version: 1,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn network_error_on_remove_keeps_it_queued_and_marks_attempted() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let api = client(&format!("http://127.0.0.1:{port}"));
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 1)).unwrap();
        let err = q
            .replay(&api, SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Network), "{err:?}");
        assert_eq!(q.queued(PLAYLIST).len(), 1, "the removal stays queued");
        assert!(matches!(
            q.queued(PLAYLIST)[0],
            WriteOp::RemoveEntry {
                attempted: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn server_error_on_remove_keeps_it_queued_and_marks_attempted() {
        let s = stub(vec![raw("500 Internal Server Error", INTERNAL)]).await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 1)).unwrap();
        let err = q
            .replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Internal(_)), "{err:?}");
        assert_eq!(q.queued(PLAYLIST).len(), 1, "the removal stays queued");
        assert!(matches!(
            q.queued(PLAYLIST)[0],
            WriteOp::RemoveEntry {
                attempted: true,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn replay_sends_in_order_and_removes_each_sent_write() {
        let s = stub(vec![
            raw("204 No Content", ""),
            added("track-a", 8),
            raw("200 OK", r#"{"version":9}"#),
        ])
        .await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        q.push(PLAYLIST, add_op("track-a", 7)).unwrap();
        q.push(PLAYLIST, reorder_op(7)).unwrap();
        q.replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap();
        assert!(q.queued(PLAYLIST).is_empty());
        assert_eq!(
            s.lines(),
            vec![
                format!(
                    "DELETE /api/v1/playlists/{PLAYLIST}/entries/{ENTRY_A}?expectedVersion=7 HTTP/1.1"
                ),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
                format!("PUT /api/v1/playlists/{PLAYLIST}/order HTTP/1.1"),
            ],
            "each write sent exactly once, oldest first"
        );
    }

    #[tokio::test]
    async fn replay_stops_at_conflict_and_keeps_that_write_and_later_ones() {
        let s = stub(vec![
            raw("204 No Content", ""),
            raw("409 Conflict", CONFLICT),
            raw("200 OK", r#"{"version":9}"#),
        ])
        .await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        q.push(PLAYLIST, add_op("track-a", 7)).unwrap();
        q.push(PLAYLIST, reorder_op(7)).unwrap();
        let err = q
            .replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(
            matches!(
                err,
                SyncError::Conflict {
                    current_version: Some(9),
                    ..
                }
            ),
            "{err:?}"
        );
        // The remove succeeded and was dropped; the add (conflicting) and the reorder stay queued.
        assert_eq!(labels(&q, PLAYLIST), vec!["track-a", "reorder"]);
        assert_eq!(s.count(), 2, "reorder must not be sent after the conflict");
    }

    #[tokio::test]
    async fn replay_network_refused_keeps_all_writes() {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, add_op("track-a", 7)).unwrap();
        q.push(PLAYLIST, add_op("track-b", 7)).unwrap();
        let err = q
            .replay(
                &client(&format!("http://127.0.0.1:{port}")),
                SENTINEL,
                PLAYLIST,
                &mut Vec::new(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Network), "{err:?}");
        assert_eq!(labels(&q, PLAYLIST), vec!["track-a", "track-b"]);
    }

    #[tokio::test]
    async fn failed_replay_removes_only_the_write_that_succeeded() {
        let s = stub(vec![
            raw("204 No Content", ""),
            raw("500 Internal Server Error", INTERNAL),
        ])
        .await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        q.push(PLAYLIST, remove_op(ENTRY_B, 7)).unwrap();
        let err = q
            .replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap_err();
        assert!(matches!(err, SyncError::Internal(_)), "{err:?}");
        assert_eq!(labels(&q, PLAYLIST), vec![ENTRY_B]);
        assert_eq!(s.count(), 2);
    }

    #[tokio::test]
    async fn token_never_appears_in_debug_or_errors() {
        let s = stub(vec![raw("409 Conflict", CONFLICT)]).await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        let err = q
            .replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap_err();
        let full = QueueError::Full;
        let out = format!("{q:?} {err:?} {err} {full:?} {full}");
        assert!(!out.contains(SENTINEL), "{out}");
    }

    #[tokio::test]
    async fn conflict_sets_pending_flag() {
        let s = stub(vec![raw("409 Conflict", CONFLICT)]).await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        assert!(!q.conflict_pending(PLAYLIST));
        assert!(
            q.replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
                .await
                .is_err()
        );
        assert!(q.conflict_pending(PLAYLIST));
        assert!(!q.conflict_pending("other"));
    }

    #[tokio::test]
    async fn successful_replay_clears_pending_flag() {
        let s = stub(vec![
            raw("409 Conflict", CONFLICT),
            raw("204 No Content", ""),
        ])
        .await;
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        assert!(
            q.replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
                .await
                .is_err()
        );
        assert!(q.conflict_pending(PLAYLIST));
        q.replay(&client(&s.base), SENTINEL, PLAYLIST, &mut Vec::new())
            .await
            .unwrap();
        assert!(!q.conflict_pending(PLAYLIST));
    }

    #[test]
    fn rebase_rewrites_every_queued_write_of_that_playlist_only() {
        fn version(op: &WriteOp) -> i64 {
            match op {
                WriteOp::AddEntry(b) => b.expected_version,
                WriteOp::RemoveEntry {
                    expected_version, ..
                }
                | WriteOp::Reorder {
                    expected_version, ..
                } => *expected_version,
            }
        }
        let mut q = OfflineQueue::default();
        q.push(PLAYLIST, add_op("track-a", 7)).unwrap();
        q.push(PLAYLIST, remove_op(ENTRY_A, 7)).unwrap();
        q.push(PLAYLIST, reorder_op(7)).unwrap();
        q.push("other", remove_op(ENTRY_B, 7)).unwrap();
        q.rebase(PLAYLIST, 9);
        let versions: Vec<i64> = q.queued(PLAYLIST).into_iter().map(version).collect();
        // Each replayed write bumps the server version, so the chain continues from it.
        assert_eq!(versions, vec![9, 10, 11]);
        assert_eq!(version(q.queued("other")[0]), 7);
    }

    #[test]
    fn random_interleavings_keep_per_playlist_order_and_cap() {
        // Fixed-seed LCG: deterministic without adding a rand dependency.
        let mut seed: u64 = 42;
        let mut next = move || {
            seed = seed
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        let playlists = ["p0", "p1", "p2"];
        let mut q = OfflineQueue::default();
        let mut model: Vec<Vec<String>> = vec![Vec::new(); playlists.len()];
        for step in 0..2000 {
            let p = next() % playlists.len();
            let label = format!("track-{step}");
            let res = q.push(playlists[p], add_op(&label, 1));
            if model[p].len() == CAPACITY {
                assert_eq!(res, Err(QueueError::Full));
            } else {
                res.unwrap();
                model[p].push(label);
            }
        }
        for (p, pl) in playlists.iter().enumerate() {
            assert_eq!(labels(&q, pl), model[p]);
        }
    }
}
