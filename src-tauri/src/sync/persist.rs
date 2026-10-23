//! On-disk sync snapshot: local playlists, queued writes and conflict flags as JSON.
//! Never holds a session token, password or Authorization value. The caller passes the path.

use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use super::api::{EntryAddRequest, Provider};
use super::local_store::{LocalEntry, LocalPlaylist, LocalStore};
use super::queue::{OfflineQueue, WriteOp};

#[derive(Debug, thiserror::Error)]
pub enum PersistError {
    #[error("sync snapshot file could not be read or written: {0}")]
    Io(#[from] io::Error),
    // The serde message can echo file contents, so it is dropped on purpose.
    #[error("sync snapshot file is corrupt; it was left untouched")]
    Corrupt,
}

#[derive(Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncSnapshot {
    pub playlists: Vec<SnapshotPlaylist>,
    /// Queued writes keyed by the queue's playlist id, oldest first.
    pub queues: BTreeMap<String, Vec<SnapshotWrite>>,
    pub conflicted: BTreeSet<String>,
    /// Remote ids to re-read from the server on the next refresh; absent in older snapshots.
    #[serde(default)]
    pub refresh: BTreeSet<String>,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotPlaylist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub entries: Vec<SnapshotEntry>,
    pub remote_id: Option<String>,
    pub uploaded: usize,
    /// Queued adds not yet accepted by the server; restored so the entries stay pending.
    #[serde(default)]
    pub queued: usize,
    /// expectedVersion for the next add; without it a restored partial upload would send a stale version.
    #[serde(default)]
    pub version: i64,
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: Option<String>,
    pub artists: Option<Vec<String>>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
    pub remote_entry_id: Option<String>,
}

/// Mirrors `queue::WriteOp`; the live type has no serde derives.
#[derive(Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op")]
pub enum SnapshotWrite {
    AddEntry {
        expected_version: i64,
        entry: SnapshotEntry,
    },
    RemoveEntry {
        entry_id: String,
        expected_version: i64,
        index: usize,
        entry: SnapshotEntry,
        /// Absent in older snapshots, where the removal was never marked attempted.
        #[serde(default)]
        attempted: bool,
    },
    Reorder {
        entry_ids: Vec<String>,
        expected_version: i64,
    },
}

impl SnapshotEntry {
    fn from_local(e: &LocalEntry) -> Self {
        Self {
            provider: e.provider,
            provider_track_id: e.provider_track_id.clone(),
            provider_url: e.provider_url.clone(),
            title: e.title.clone(),
            artists: e.artists.clone(),
            artwork_url: e.artwork_url.clone(),
            duration_ms: e.duration_ms,
            remote_entry_id: e.remote_entry_id.clone(),
        }
    }

    fn into_local(self) -> LocalEntry {
        LocalEntry {
            provider: self.provider,
            provider_track_id: self.provider_track_id,
            provider_url: self.provider_url,
            title: self.title,
            artists: self.artists,
            artwork_url: self.artwork_url,
            duration_ms: self.duration_ms,
            remote_entry_id: self.remote_entry_id,
        }
    }
}

impl SnapshotWrite {
    fn into_op(self) -> WriteOp {
        match self {
            Self::AddEntry {
                expected_version,
                entry,
            } => WriteOp::AddEntry(EntryAddRequest {
                expected_version,
                provider: entry.provider,
                provider_track_id: entry.provider_track_id,
                provider_url: entry.provider_url,
                title: entry.title,
                artists: entry.artists,
                artwork_url: entry.artwork_url,
                duration_ms: entry.duration_ms,
            }),
            Self::RemoveEntry {
                entry_id,
                expected_version,
                index,
                entry,
                attempted,
            } => WriteOp::RemoveEntry {
                entry_id,
                expected_version,
                index,
                entry: entry.into_local(),
                attempted,
            },
            Self::Reorder {
                entry_ids,
                expected_version,
            } => WriteOp::Reorder {
                entry_ids,
                expected_version,
            },
        }
    }

    fn from_op(op: &WriteOp) -> Self {
        match op {
            WriteOp::AddEntry(b) => Self::AddEntry {
                expected_version: b.expected_version,
                entry: SnapshotEntry {
                    provider: b.provider,
                    provider_track_id: b.provider_track_id.clone(),
                    provider_url: b.provider_url.clone(),
                    title: b.title.clone(),
                    artists: b.artists.clone(),
                    artwork_url: b.artwork_url.clone(),
                    duration_ms: b.duration_ms,
                    remote_entry_id: None,
                },
            },
            WriteOp::RemoveEntry {
                entry_id,
                expected_version,
                index,
                entry,
                attempted,
            } => Self::RemoveEntry {
                entry_id: entry_id.clone(),
                expected_version: *expected_version,
                index: *index,
                entry: SnapshotEntry::from_local(entry),
                attempted: *attempted,
            },
            WriteOp::Reorder {
                entry_ids,
                expected_version,
            } => Self::Reorder {
                entry_ids: entry_ids.clone(),
                expected_version: *expected_version,
            },
        }
    }
}

pub fn capture(store: &LocalStore, queue: &OfflineQueue) -> SyncSnapshot {
    SyncSnapshot {
        playlists: store
            .playlists()
            .iter()
            .map(|p| SnapshotPlaylist {
                id: p.id.clone(),
                name: p.name.clone(),
                description: p.description.clone(),
                entries: p.entries.iter().map(SnapshotEntry::from_local).collect(),
                remote_id: p.remote_id.clone(),
                uploaded: p.uploaded,
                queued: p.queued,
                version: p.version,
            })
            .collect(),
        queues: queue
            .queues()
            .iter()
            .filter(|(_, q)| !q.is_empty())
            .map(|(id, q)| (id.clone(), q.iter().map(SnapshotWrite::from_op).collect()))
            .collect(),
        conflicted: queue.conflicted().iter().cloned().collect(),
        refresh: store.refresh_marks().into_iter().collect(),
    }
}

/// Rebuilds live state from a loaded snapshot. Every persisted field is restored, so capture(restore(s)) == s.
pub fn restore(snapshot: SyncSnapshot) -> (LocalStore, OfflineQueue) {
    let playlists = snapshot
        .playlists
        .into_iter()
        .map(|p| LocalPlaylist {
            id: p.id,
            name: p.name,
            description: p.description,
            entries: p.entries.into_iter().map(|e| e.into_local()).collect(),
            remote_id: p.remote_id,
            uploaded: p.uploaded,
            queued: p.queued,
            version: p.version,
        })
        .collect();
    let queues = snapshot
        .queues
        .into_iter()
        .map(|(id, writes)| (id, writes.into_iter().map(|w| w.into_op()).collect()))
        .collect();
    let conflicted = snapshot.conflicted.into_iter().collect();
    let mut store = LocalStore::from_playlists(playlists);
    store.set_refresh_marks(snapshot.refresh);
    (store, OfflineQueue::from_parts(queues, conflicted))
}

/// Atomic: the bytes land in a sibling temp file, are fsynced, then renamed over the target.
pub fn save(path: &Path, snapshot: &SyncSnapshot) -> Result<(), PersistError> {
    let bytes = serde_json::to_vec(snapshot).map_err(io::Error::other)?;
    write_atomic(path, &bytes, || Ok(()))
}

/// A missing file is an empty snapshot. A corrupt file is an error and is never deleted or rewritten.
pub fn load(path: &Path) -> Result<SyncSnapshot, PersistError> {
    match fs::read(path) {
        Ok(bytes) => {
            let snapshot: SyncSnapshot =
                serde_json::from_slice(&bytes).map_err(|_| PersistError::Corrupt)?;
            // drain and adopt_created index by uploaded and queued, so an overrun would panic later.
            if snapshot
                .playlists
                .iter()
                .any(|p| p.uploaded > p.entries.len() || p.queued > p.entries.len() - p.uploaded)
            {
                return Err(PersistError::Corrupt);
            }
            Ok(snapshot)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(SyncSnapshot::default()),
        Err(e) => Err(e.into()),
    }
}

/// `before_rename` is a test seam: an error there must leave the target untouched.
fn write_atomic(
    path: &Path,
    bytes: &[u8],
    before_rename: impl FnOnce() -> io::Result<()>,
) -> Result<(), PersistError> {
    let name = path.file_name().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "snapshot path has no file name",
        )
    })?;
    let mut tmp_name = name.to_os_string();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    let result = write_temp(&tmp, bytes)
        .and_then(|()| before_rename())
        .and_then(|()| fs::rename(&tmp, path));
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result.map_err(PersistError::Io)
}

fn write_temp(tmp: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut file = File::create(tmp)?;
    // Playlist names and track URLs are private: owner-only, also when a stale temp file already exists.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::api::{ApiClient, EntryAddRequest};
    use std::fs;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SENTINEL: &str = "SENTINEL-PERSIST-TOKEN-4b1e";
    const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    const ENTRY_A: &str = "6f1d2c3a-0000-4000-8000-000000000002";
    const INTERNAL: &str = r#"{"error":{"code":"INTERNAL_ERROR","message":"Internal error."}}"#;
    const CONFLICT: &str = r#"{"error":{"code":"VERSION_CONFLICT","message":"Playlist was changed.","details":{"currentVersion":9}}}"#;

    /// Temp directory removed on drop. The name is unique per process and per call.
    struct TempDir(std::path::PathBuf);

    static NEXT: AtomicUsize = AtomicUsize::new(0);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir = std::env::temp_dir()
                .join(format!("lunor-persist-{tag}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn file(&self) -> std::path::PathBuf {
            self.0.join("sync-snapshot.json")
        }

        fn temp_file(&self) -> std::path::PathBuf {
            self.0.join("sync-snapshot.json.tmp")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// Answers requests in order (the last response repeats). The thread stays blocked after the test ends.
    fn serve(responses: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            for i in 0usize.. {
                let Ok((mut sock, _)) = listener.accept() else {
                    return;
                };
                read_request(&mut sock);
                let res = &responses[i.min(responses.len() - 1)];
                let _ = sock.write_all(res.as_bytes());
            }
        });
        base
    }

    fn read_request(sock: &mut TcpStream) {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = sock.read(&mut chunk).unwrap_or(0);
            if n == 0 {
                return;
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
                    return;
                }
            }
        }
    }

    fn raw(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    fn created() -> String {
        raw(
            "201 Created",
            &format!(r#"{{"id":"{PLAYLIST}","name":"Mix","description":"notes","version":1}}"#),
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

    fn spotify(track: &str) -> LocalEntry {
        LocalEntry {
            provider: Provider::Spotify,
            provider_track_id: track.into(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
            remote_entry_id: None,
        }
    }

    /// Builds real state through the public API: one playlist with a partial upload (remote id set,
    /// one entry sent), and one queued playlist whose replay hit a conflict. SENTINEL is the session token.
    async fn fixture() -> (LocalStore, OfflineQueue) {
        let base = serve(vec![
            created(),
            added("track-a", 2),
            raw("500 Internal Server Error", INTERNAL),
            raw("409 Conflict", CONFLICT),
        ]);
        let api = ApiClient::new(&base).unwrap();
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", Some("notes")).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        store.add_local_entry(&id, spotify("track-b")).unwrap();
        assert!(store.upload(&api, SENTINEL, &id).await.is_err());
        let mut queue = OfflineQueue::default();
        queue
            .push(
                PLAYLIST,
                WriteOp::RemoveEntry {
                    entry_id: ENTRY_A.into(),
                    expected_version: 2,
                    index: 0,
                    entry: spotify("track-a"),
                    attempted: false,
                },
            )
            .unwrap();
        queue
            .push(
                PLAYLIST,
                WriteOp::AddEntry(EntryAddRequest {
                    expected_version: 2,
                    provider: Provider::Spotify,
                    provider_track_id: "track-c".into(),
                    provider_url: "https://open.spotify.com/track/track-c".into(),
                    title: None,
                    artists: None,
                    artwork_url: None,
                    duration_ms: None,
                }),
            )
            .unwrap();
        assert!(
            queue
                .replay(&api, SENTINEL, PLAYLIST, &mut Vec::new())
                .await
                .is_err()
        );
        (store, queue)
    }

    #[tokio::test]
    async fn capture_keeps_remote_id_uploaded_count_queued_versions_and_conflicts() {
        let (store, queue) = fixture().await;
        let snap = capture(&store, &queue);
        let pl = &snap.playlists[0];
        assert_eq!(pl.name, "Mix");
        assert_eq!(pl.entries.len(), 2);
        assert_eq!(pl.remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(pl.uploaded, 1);
        let writes = &snap.queues[PLAYLIST];
        assert_eq!(writes.len(), 2);
        assert!(matches!(
            &writes[0],
            SnapshotWrite::RemoveEntry {
                expected_version: 2,
                ..
            }
        ));
        assert!(matches!(
            &writes[1],
            SnapshotWrite::AddEntry {
                expected_version: 2,
                ..
            }
        ));
        assert!(snap.conflicted.contains(PLAYLIST));
    }

    #[tokio::test]
    async fn round_trip_preserves_playlists_queues_and_conflicts() {
        let dir = TempDir::new("roundtrip");
        let (store, queue) = fixture().await;
        let snap = capture(&store, &queue);
        save(&dir.file(), &snap).unwrap();
        assert_eq!(load(&dir.file()).unwrap(), snap);
    }

    #[tokio::test]
    async fn snapshot_json_never_contains_session_token() {
        let (store, queue) = fixture().await;
        let json = serde_json::to_string(&capture(&store, &queue)).unwrap();
        assert!(!json.contains(SENTINEL), "{json}");
    }

    #[tokio::test]
    async fn saved_file_never_contains_session_token() {
        let dir = TempDir::new("nosecret");
        let (store, queue) = fixture().await;
        save(&dir.file(), &capture(&store, &queue)).unwrap();
        let text = String::from_utf8_lossy(&fs::read(dir.file()).unwrap()).into_owned();
        assert!(!text.contains(SENTINEL), "{text}");
    }

    #[cfg(unix)]
    #[test]
    fn temp_snapshot_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new("mode");
        let tmp = dir.0.join("snapshot.json.tmp");
        fs::write(&tmp, b"stale").unwrap();
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o644)).unwrap();
        write_temp(&tmp, b"{}").unwrap();
        let mode = fs::metadata(&tmp).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    #[test]
    fn missing_file_loads_empty_snapshot() {
        let dir = TempDir::new("missing");
        assert_eq!(load(&dir.file()).unwrap(), SyncSnapshot::default());
    }

    #[test]
    fn corrupt_file_errors_without_touching_bytes_or_leaking_contents() {
        // Syntax error, and a valid-JSON type mismatch (serde would echo the string value).
        let cases: [&[u8]; 2] = [
            br#"{"playlists": "SECRET-CONTENT-XYZ""#,
            br#"{"playlists":"SECRET-CONTENT-XYZ","queues":{},"conflicted":[]}"#,
        ];
        for bytes in cases {
            let dir = TempDir::new("corrupt");
            fs::write(dir.file(), bytes).unwrap();
            let err = load(&dir.file()).unwrap_err();
            assert!(matches!(err, PersistError::Corrupt), "{err:?}");
            let shown = format!("{err} {err:?}");
            assert!(!shown.contains("XYZ"), "{shown}");
            assert_eq!(fs::read(dir.file()).unwrap(), bytes);
        }
    }

    #[tokio::test]
    async fn failed_write_before_rename_keeps_previous_file() {
        let dir = TempDir::new("prerename");
        let (store, queue) = fixture().await;
        save(&dir.file(), &capture(&store, &queue)).unwrap();
        let previous = fs::read(dir.file()).unwrap();
        let err = write_atomic(&dir.file(), b"{\"playlists\":[]}", || {
            Err(io::Error::other("forced failure"))
        })
        .unwrap_err();
        assert!(matches!(err, PersistError::Io(_)), "{err:?}");
        assert_eq!(fs::read(dir.file()).unwrap(), previous);
        assert!(!dir.temp_file().exists());
    }

    #[test]
    fn pre_change_snapshot_loads_with_no_marks_and_unattempted_removes() {
        // Shape saved before `refresh` and `attempted` existed: it must still load, and its remove must still be accepted.
        let json = format!(
            r#"{{"playlists":[{{"id":"local-1","name":"Mix","description":null,"entries":[],"remote_id":"{PLAYLIST}","uploaded":0}}],"queues":{{"{PLAYLIST}":[{{"op":"RemoveEntry","entry_id":"{ENTRY_A}","expected_version":2,"index":0,"entry":{{"provider":"Spotify","provider_track_id":"track-a","provider_url":"https://open.spotify.com/track/track-a","title":null,"artists":null,"artwork_url":null,"duration_ms":null,"remote_entry_id":null}}}}]}},"conflicted":[]}}"#
        );
        let dir = TempDir::new("pre-change");
        fs::write(dir.file(), json).unwrap();
        let snap = load(&dir.file()).unwrap();
        assert!(snap.refresh.is_empty());
        let (store, queue) = restore(snap);
        assert!(store.refresh_marks().is_empty());
        assert!(matches!(
            queue.queued(PLAYLIST)[0],
            WriteOp::RemoveEntry {
                attempted: false,
                ..
            }
        ));
    }

    #[test]
    fn refresh_marks_and_attempted_removes_survive_save_and_restore() {
        let dir = TempDir::new("marks");
        let mut store = LocalStore::from_playlists(vec![LocalPlaylist {
            id: "local-1".into(),
            name: "Mix".into(),
            description: None,
            entries: vec![],
            remote_id: Some(PLAYLIST.into()),
            uploaded: 0,
            queued: 0,
            version: 1,
        }]);
        let attempted_remove = || WriteOp::RemoveEntry {
            entry_id: ENTRY_A.into(),
            expected_version: 1,
            index: 0,
            entry: spotify("track-a"),
            attempted: true,
        };
        // A discarded attempted removal leaves a refresh mark; a queued attempted removal keeps its flag.
        store.discard_queued(vec![(PLAYLIST.into(), attempted_remove())]);
        let mut queue = OfflineQueue::default();
        queue.push(PLAYLIST, attempted_remove()).unwrap();
        save(&dir.file(), &capture(&store, &queue)).unwrap();

        let (restored, restored_queue) = restore(load(&dir.file()).unwrap());
        assert_eq!(restored.refresh_marks(), [PLAYLIST.to_string()]);
        assert!(matches!(
            restored_queue.queued(PLAYLIST)[0],
            WriteOp::RemoveEntry {
                attempted: true,
                ..
            }
        ));
    }

    #[test]
    fn inconsistent_playlist_counts_are_corrupt_and_file_untouched() {
        // uploaded + queued past entries.len(), uploaded past it, and a usize-overflowing pair: each would panic later.
        let cases = [
            r#"{"playlists":[{"id":"local-1","name":"Mix","description":null,"entries":[],"remote_id":"x","uploaded":1,"queued":1}],"queues":{},"conflicted":[]}"#,
            r#"{"playlists":[{"id":"local-1","name":"Mix","description":null,"entries":[],"remote_id":"x","uploaded":2}],"queues":{},"conflicted":[]}"#,
            r#"{"playlists":[{"id":"local-1","name":"Mix","description":null,"entries":[],"remote_id":"x","uploaded":18446744073709551615,"queued":1}],"queues":{},"conflicted":[]}"#,
        ];
        for json in cases {
            let dir = TempDir::new("inconsistent");
            fs::write(dir.file(), json).unwrap();
            let err = load(&dir.file()).unwrap_err();
            assert!(matches!(err, PersistError::Corrupt), "{err:?}");
            assert_eq!(fs::read(dir.file()).unwrap(), json.as_bytes());
        }
    }

    #[tokio::test]
    async fn saving_twice_replaces_content() {
        let dir = TempDir::new("twice");
        let (store, queue) = fixture().await;
        save(&dir.file(), &capture(&store, &queue)).unwrap();
        save(&dir.file(), &SyncSnapshot::default()).unwrap();
        assert_eq!(load(&dir.file()).unwrap(), SyncSnapshot::default());
        assert_eq!(
            fs::read(dir.file()).unwrap(),
            serde_json::to_vec(&SyncSnapshot::default()).unwrap()
        );
        assert!(!dir.temp_file().exists());
    }
}
