//! In-memory local playlists. Only `LocalStore::upload` touches the backend; persisted through persist.rs.

use std::collections::HashSet;

#[cfg(test)]
use super::api::{ApiClient, PlaylistCreateRequest};
use super::api::{Entry, EntryAddRequest, Provider, SyncError};
use super::queue::WriteOp;

#[derive(Debug, Clone)]
pub struct LocalEntry {
    pub provider: Provider,
    pub provider_track_id: String,
    pub provider_url: String,
    pub title: Option<String>,
    pub artists: Option<Vec<String>>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
    /// Server entry id, known once the entry is on the server. A removal needs it.
    pub remote_entry_id: Option<String>,
}

impl LocalEntry {
    /// A server entry as the local copy keeps it, with its server id.
    pub fn from_server(e: Entry) -> Self {
        Self {
            provider: e.provider,
            provider_track_id: e.provider_track_id,
            provider_url: e.provider_url,
            title: e.title,
            artists: e.artists,
            artwork_url: e.artwork_url,
            duration_ms: e.duration_ms,
            remote_entry_id: Some(e.id),
        }
    }
}

/// Mirrors backend rule R-S2: Spotify entries carry ids only.
fn validate_entry(entry: &LocalEntry) -> Result<(), SyncError> {
    if entry.provider == Provider::Spotify
        && (entry.title.is_some()
            || entry.artists.is_some()
            || entry.artwork_url.is_some()
            || entry.duration_ms.is_some())
    {
        return Err(SyncError::Local(
            "Spotify entries store ids only; title, artists, artwork and duration must be omitted",
        ));
    }
    Ok(())
}

#[derive(Debug)]
pub struct LocalPlaylist {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub entries: Vec<LocalEntry>,
    /// Set as soon as the remote create succeeds, so a retry resumes instead of creating a second playlist.
    pub remote_id: Option<String>,
    /// Entries the backend has accepted, in order; a retry resumes adding from this index.
    pub uploaded: usize,
    /// Queued adds right after `uploaded`: sent through the offline queue, not yet accepted by the server.
    pub queued: usize,
    /// expectedVersion for the next add, taken from the last successful response.
    pub(crate) version: i64,
}

/// One planned request of an upload. `Create` and `Add` are sent with the engine released.
pub enum UploadStep {
    Create {
        name: String,
        description: Option<String>,
    },
    Add {
        remote: String,
        body: EntryAddRequest,
    },
    Done {
        remote: String,
    },
}

#[derive(Debug, Default)]
pub struct LocalStore {
    playlists: Vec<LocalPlaylist>,
    /// Remote ids whose removal was sent with an unknown outcome; the next refresh re-reads them.
    refresh: HashSet<String>,
}

/// The first lines of a description written by a YouTube import. The marker is the last line of a description, after
/// any text the user wrote; a later import finds its playlist again by it.
pub const MUSIC_MARKER: &str = "Imported from YouTube Music [";
pub const YOUTUBE_MARKER: &str = "Imported from YouTube [";

/// The playlist id inside an import marker line, if `line` is one.
pub fn marker_id(line: &str) -> Option<&str> {
    line.strip_prefix(MUSIC_MARKER)
        .or_else(|| line.strip_prefix(YOUTUBE_MARKER))
        .and_then(|rest| rest.strip_suffix(']'))
}

/// Splits a description into the text the user wrote and its import marker line, if it ends with one.
pub fn split_note(description: &str) -> (&str, Option<&str>) {
    let (text, last) = description.rsplit_once('\n').unwrap_or(("", description));
    if marker_id(last).is_some() {
        (text.trim_end(), Some(last))
    } else {
        (description, None)
    }
}

/// The user's text with the marker (if any) kept as the last line.
pub fn join_note(text: Option<&str>, marker: Option<&str>) -> Option<String> {
    match (text.filter(|t| !t.is_empty()), marker) {
        (Some(t), Some(m)) => Some(format!("{t}\n{m}")),
        (Some(t), None) => Some(t.to_string()),
        (None, Some(m)) => Some(m.to_string()),
        (None, None) => None,
    }
}

/// Same limits as backend/src/playlists/mod.rs (name 1..=200 after trim, description <= 2000).
const NAME_MAX: usize = 200;
const DESCRIPTION_MAX: usize = 2000;

/// Std-only UUID-v4-shaped id. Each RandomState is seeded from OS randomness, so hashing an empty input yields a fresh 64-bit value.
fn new_local_id() -> String {
    use std::hash::{BuildHasher, Hasher, RandomState};
    let mut b = [0u8; 16];
    for chunk in b.chunks_mut(8) {
        let r = RandomState::new().build_hasher().finish().to_le_bytes();
        chunk.copy_from_slice(&r[..chunk.len()]);
    }
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

impl LocalStore {
    /// Restore path for persist::restore: takes the snapshot's playlists as they were saved.
    pub fn from_playlists(playlists: Vec<LocalPlaylist>) -> Self {
        Self {
            playlists,
            ..Self::default()
        }
    }

    /// Read-only view for persist::capture.
    pub fn playlists(&self) -> &[LocalPlaylist] {
        &self.playlists
    }

    /// Remote ids marked for a server re-read (see `discard_queued`).
    pub fn refresh_marks(&self) -> Vec<String> {
        self.refresh.iter().cloned().collect()
    }

    /// Restore path for persist::restore: re-adds the marks a previous run had not yet cleared.
    pub fn set_refresh_marks(&mut self, marks: impl IntoIterator<Item = String>) {
        self.refresh.extend(marks);
    }

    /// True while the local copy holds entries the server has not accepted. A refresh must not replace it.
    pub fn has_unsent_tail(&self, remote_id: &str) -> bool {
        self.playlists.iter().any(|p| {
            p.remote_id.as_deref() == Some(remote_id)
                && (p.entries.len() > p.uploaded
                    || p.entries.iter().any(|e| e.remote_entry_id.is_none()))
        })
    }

    fn find(&self, local_id: &str) -> Result<&LocalPlaylist, SyncError> {
        self.playlists
            .iter()
            .find(|p| p.id == local_id)
            .ok_or(SyncError::Local("playlist not found"))
    }

    fn find_mut(&mut self, local_id: &str) -> Result<&mut LocalPlaylist, SyncError> {
        self.playlists
            .iter_mut()
            .find(|p| p.id == local_id)
            .ok_or(SyncError::Local("playlist not found"))
    }

    pub fn create_local(
        &mut self,
        name: &str,
        description: Option<&str>,
    ) -> Result<String, SyncError> {
        let name = name.trim();
        if !(1..=NAME_MAX).contains(&name.chars().count()) {
            return Err(SyncError::Local("name must be 1 to 200 characters"));
        }
        if description.is_some_and(|d| d.chars().count() > DESCRIPTION_MAX) {
            return Err(SyncError::Local(
                "description must be at most 2000 characters",
            ));
        }
        let id = new_local_id();
        self.playlists.push(LocalPlaylist {
            id: id.clone(),
            name: name.to_string(),
            description: description.map(str::to_string),
            entries: Vec::new(),
            remote_id: None,
            uploaded: 0,
            queued: 0,
            version: 0,
        });
        Ok(id)
    }

    /// Forgets a playlist on this device. For an uploaded one the caller deletes it on the server first
    /// (`engine::delete_playlist`), so the two cannot drift.
    pub fn delete_local(&mut self, local_id: &str) -> Result<(), SyncError> {
        let index = self
            .playlists
            .iter()
            .position(|p| p.id == local_id)
            .ok_or(SyncError::Local("playlist not found"))?;
        let removed = self.playlists.remove(index);
        // A mark for the deleted remote playlist would make every later refresh ask the server for it again.
        if let Some(remote) = removed.remote_id {
            self.refresh.remove(&remote);
        }
        Ok(())
    }

    /// Sign-out: drops every remote binding so the next account re-creates the playlists. Content is kept.
    pub fn unbind_all_remote(&mut self) {
        // Marks hold the old account's remote ids; the next token must not re-read them.
        self.refresh.clear();
        for p in &mut self.playlists {
            p.remote_id = None;
            p.uploaded = 0;
            p.queued = 0;
            // Server entry ids belong to the old account's playlist.
            for e in &mut p.entries {
                e.remote_entry_id = None;
            }
        }
    }

    pub fn add_local_entry(&mut self, local_id: &str, entry: LocalEntry) -> Result<(), SyncError> {
        validate_entry(&entry)?;
        self.find_mut(local_id)?.entries.push(entry);
        Ok(())
    }

    /// Uploaded playlists send adds through the queue (see engine). None: not uploaded yet, or an earlier
    /// upload is partial, and then the entry joins that upload's unsent tail, which goes out in order.
    /// Queued adds are the entries right after `uploaded`; `queued` counts them.
    pub fn queued_add(
        &self,
        local_id: &str,
        entry: &LocalEntry,
    ) -> Result<Option<(String, WriteOp)>, SyncError> {
        validate_entry(entry)?;
        let pl = self.find(local_id)?;
        match &pl.remote_id {
            Some(remote) if pl.uploaded + pl.queued == pl.entries.len() => Ok(Some((
                remote.clone(),
                WriteOp::AddEntry(EntryAddRequest {
                    expected_version: pl.version,
                    provider: entry.provider,
                    provider_track_id: entry.provider_track_id.clone(),
                    provider_url: entry.provider_url.clone(),
                    title: entry.title.clone(),
                    artists: entry.artists.clone(),
                    artwork_url: entry.artwork_url.clone(),
                    duration_ms: entry.duration_ms,
                }),
            ))),
            _ => Ok(None),
        }
    }

    /// Call after the queue accepted the write from `queued_add`. The entry is pending, not uploaded, until
    /// replay (`adopt_created`) gets the server's accept. The projected version moves with the queued write.
    pub fn add_queued(&mut self, local_id: &str, entry: LocalEntry) -> Result<(), SyncError> {
        let pl = self.find_mut(local_id)?;
        pl.entries.push(entry);
        pl.queued += 1;
        pl.version += 1;
        Ok(())
    }

    /// Undoes the writes `OfflineQueue::discard_all` dropped: queued adds leave the list, removed entries go back
    /// to their recorded index, and the projected version rewinds by one per dropped write.
    pub fn discard_queued(&mut self, dropped: Vec<(String, WriteOp)>) {
        for pl in &mut self.playlists {
            pl.entries.drain(pl.uploaded..pl.uploaded + pl.queued);
            pl.queued = 0;
        }
        // Newest first: each removal's index is relative to the list as it was just before that removal.
        for (remote, op) in dropped.into_iter().rev() {
            let Some(pl) = self
                .playlists
                .iter_mut()
                .find(|p| p.remote_id.as_deref() == Some(remote.as_str()))
            else {
                continue;
            };
            pl.version -= 1;
            if let WriteOp::RemoveEntry {
                index,
                entry,
                attempted,
                ..
            } = op
            {
                // An attempted removal may already be applied on the server: reinserting it would
                // duplicate it, so the playlist is re-read from the server instead.
                if attempted {
                    self.refresh.insert(remote);
                } else {
                    pl.entries.insert(index.min(pl.entries.len()), entry);
                    pl.uploaded += 1;
                }
            }
        }
    }

    /// Remote-bound entries the server holds are removed through the queue. None: local-only removal.
    pub fn queued_remove(
        &self,
        local_id: &str,
        index: usize,
    ) -> Result<Option<(String, WriteOp)>, SyncError> {
        let pl = self.find(local_id)?;
        let entry = pl
            .entries
            .get(index)
            .ok_or(SyncError::Local("entry index is out of range"))?;
        match &pl.remote_id {
            // Queued adds sit after `uploaded` and have no server id yet, so they fail the same way.
            Some(remote) if index < pl.uploaded + pl.queued => {
                let entry_id = entry.remote_entry_id.clone().ok_or(SyncError::Local(
                    "entry is not on the server yet; remove it after the next sync",
                ))?;
                Ok(Some((
                    remote.clone(),
                    WriteOp::RemoveEntry {
                        entry_id,
                        expected_version: pl.version,
                        index,
                        entry: entry.clone(),
                        attempted: false,
                    },
                )))
            }
            _ => Ok(None),
        }
    }

    /// Renames a playlist that is not on the server. An uploaded one is refused: the server has no rename yet, and the
    /// two names would drift apart.
    pub fn rename_local(&mut self, local_id: &str, name: &str) -> Result<(), SyncError> {
        let name = name.trim();
        if !(1..=NAME_MAX).contains(&name.chars().count()) {
            return Err(SyncError::Local("name must be 1 to 200 characters"));
        }
        let pl = self.find_mut(local_id)?;
        if pl.remote_id.is_some() {
            return Err(SyncError::Local(
                "an uploaded playlist cannot be renamed yet",
            ));
        }
        pl.name = name.to_string();
        Ok(())
    }

    /// Sets or clears the text of a playlist's description; an import marker on its last line is kept. `synced` is for
    /// an uploaded playlist whose server copy already took the change; otherwise an uploaded one is refused.
    pub fn set_description(
        &mut self,
        local_id: &str,
        text: Option<&str>,
        synced: bool,
    ) -> Result<(), SyncError> {
        let text = text.map(str::trim).filter(|d| !d.is_empty());
        if text.is_some_and(|d| d.chars().count() > DESCRIPTION_MAX || marker_id(d).is_some()) {
            return Err(SyncError::Local(
                "description must be at most 2000 characters",
            ));
        }
        let pl = self.find_mut(local_id)?;
        if pl.remote_id.is_some() && !synced {
            return Err(SyncError::Local(
                "an uploaded playlist's description is changed on the server first",
            ));
        }
        let marker = pl
            .description
            .as_deref()
            .and_then(|d| split_note(d).1.map(str::to_string));
        pl.description = join_note(text, marker.as_deref());
        Ok(())
    }

    /// The text part of a playlist's description (without an import marker), for showing and for the server copy.
    pub fn description_text(&self, local_id: &str) -> Option<String> {
        self.find(local_id)
            .ok()
            .and_then(|p| {
                p.description
                    .as_deref()
                    .map(|d| split_note(d).0.to_string())
            })
            .filter(|t| !t.is_empty())
    }

    /// Moves entries[from] so it ends up at `to`. Only for a playlist that is not on the server, since the server
    /// keeps its own order.
    pub fn move_entry(&mut self, local_id: &str, from: usize, to: usize) -> Result<(), SyncError> {
        let pl = self.find_mut(local_id)?;
        if pl.remote_id.is_some() {
            return Err(SyncError::Local(
                "an uploaded playlist cannot be reordered yet",
            ));
        }
        if from >= pl.entries.len() || to >= pl.entries.len() {
            return Err(SyncError::Local("entry index is out of range"));
        }
        let entry = pl.entries.remove(from);
        pl.entries.insert(to, entry);
        Ok(())
    }

    /// Drops entries[index]. Removing a server-held entry also moves the uploaded count and projected version.
    pub fn remove_entry_at(&mut self, local_id: &str, index: usize) -> Result<(), SyncError> {
        let pl = self.find_mut(local_id)?;
        if index >= pl.entries.len() {
            return Err(SyncError::Local("entry index is out of range"));
        }
        if index >= pl.uploaded && index < pl.uploaded + pl.queued {
            return Err(SyncError::Local(
                "entry is not on the server yet; remove it after the next sync",
            ));
        }
        pl.entries.remove(index);
        if index < pl.uploaded {
            pl.uploaded -= 1;
            pl.version += 1;
        }
        Ok(())
    }

    /// Each id is an accepted replayed add. Replay runs oldest-first, so the add is the oldest queued entry:
    /// it moves from the queued range into the server-held prefix.
    pub fn adopt_created(&mut self, remote_id: &str, ids: &[String]) {
        let Some(pl) = self
            .playlists
            .iter_mut()
            .find(|p| p.remote_id.as_deref() == Some(remote_id))
        else {
            return;
        };
        for id in ids {
            if pl.queued == 0 {
                break;
            }
            pl.entries[pl.uploaded].remote_entry_id = Some(id.clone());
            pl.uploaded += 1;
            pl.queued -= 1;
        }
    }

    /// Takes the server's version for the playlist with this remote id. With `entries` (KeepServer) it also
    /// replaces the local copy and marks it fully uploaded. Returns the local id when one matches.
    pub fn adopt_server(
        &mut self,
        remote_id: &str,
        version: i64,
        entries: Option<Vec<LocalEntry>>,
    ) -> Option<String> {
        let pl = self
            .playlists
            .iter_mut()
            .find(|p| p.remote_id.as_deref() == Some(remote_id))?;
        pl.version = version;
        if let Some(entries) = entries {
            pl.uploaded = entries.len();
            pl.queued = 0;
            pl.entries = entries;
            self.refresh.remove(remote_id);
        }
        Some(pl.id.clone())
    }

    /// Next request for an upload, planned under the engine. The engine is released while the request runs; the
    /// `upload_created` / `upload_added` call then records its result. Resumes from `uploaded`.
    pub fn upload_step(&self, local_id: &str) -> Result<UploadStep, SyncError> {
        let pl = self.find(local_id)?;
        let Some(remote) = pl.remote_id.clone() else {
            return Ok(UploadStep::Create {
                name: pl.name.clone(),
                // The import marker is for this device's importer; the server copy shows only the user's text.
                description: pl
                    .description
                    .as_deref()
                    .map(|d| split_note(d).0.to_string())
                    .filter(|t| !t.is_empty()),
            });
        };
        // Queued adds (the last `queued` entries) are sent by the queue, never here.
        if pl.uploaded >= pl.entries.len() - pl.queued {
            return Ok(UploadStep::Done { remote });
        }
        let e = &pl.entries[pl.uploaded];
        Ok(UploadStep::Add {
            remote,
            body: EntryAddRequest {
                expected_version: pl.version,
                provider: e.provider,
                provider_track_id: e.provider_track_id.clone(),
                provider_url: e.provider_url.clone(),
                title: e.title.clone(),
                artists: e.artists.clone(),
                artwork_url: e.artwork_url.clone(),
                duration_ms: e.duration_ms,
            },
        })
    }

    /// Records a created remote playlist. remote_id is set right away, so a failed add keeps it and the next upload resumes.
    pub fn upload_created(
        &mut self,
        local_id: &str,
        remote: String,
        version: i64,
    ) -> Result<(), SyncError> {
        let pl = self.find_mut(local_id)?;
        pl.remote_id = Some(remote);
        pl.version = version;
        Ok(())
    }

    /// Records an accepted add. The add was planned for `entries[uploaded]`; removals below that position keep
    /// it there, and removing that exact entry while the add is in flight is refused (see `is_upload_position`).
    pub fn upload_added(
        &mut self,
        local_id: &str,
        entry_id: String,
        version: i64,
    ) -> Result<(), SyncError> {
        let pl = self.find_mut(local_id)?;
        if pl.uploaded >= pl.entries.len() {
            return Err(SyncError::Local("upload position is out of range"));
        }
        pl.version = version;
        pl.entries[pl.uploaded].remote_entry_id = Some(entry_id);
        pl.uploaded += 1;
        Ok(())
    }

    /// True when `index` is the next entry an upload sends (the position an add in flight occupies).
    pub fn is_upload_position(&self, local_id: &str, index: usize) -> bool {
        self.find(local_id).is_ok_and(|pl| pl.uploaded == index)
    }

    /// Asks the next refresh to re-read this remote playlist (a write was accepted after its queue was dropped).
    pub fn mark_refresh(&mut self, remote_id: &str) {
        self.refresh.insert(remote_id.to_string());
    }

    /// Sequential upload: plans and sends each request in turn.
    #[cfg(test)]
    pub async fn upload(
        &mut self,
        api: &ApiClient,
        token: &str,
        local_id: &str,
    ) -> Result<String, SyncError> {
        loop {
            match self.upload_step(local_id)? {
                UploadStep::Create { name, description } => {
                    let created = api
                        .create_playlist(token, &PlaylistCreateRequest { name, description })
                        .await?;
                    self.upload_created(local_id, created.id, created.version)?;
                }
                UploadStep::Add { remote, body } => {
                    let added = api.add_entry(token, &remote, &body).await?;
                    self.upload_added(local_id, added.entry.id, added.version)?;
                }
                UploadStep::Done { remote } => return Ok(remote),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const SENTINEL: &str = "SENTINEL-LOCAL-TOKEN-7c3e";
    const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    const CREATE_LINE: &str = "POST /api/v1/playlists HTTP/1.1";
    const ENTRIES_LINE: &str =
        "POST /api/v1/playlists/6f1d2c3a-0000-4000-8000-000000000001/entries HTTP/1.1";

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

        fn log(&self) -> Vec<(String, serde_json::Value)> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|r| (r.line.clone(), r.body.clone()))
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

    fn created(id: &str, version: i64) -> String {
        raw(
            "201 Created",
            &format!(r#"{{"id":"{id}","name":"Mix","description":null,"version":{version}}}"#),
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

    const INTERNAL: &str = r#"{"error":{"code":"INTERNAL_ERROR","message":"Internal error."}}"#;

    fn client(base: &str) -> ApiClient {
        ApiClient::new(base).unwrap()
    }

    #[test]
    fn a_description_is_trimmed_cleared_bounded_and_keeps_an_import_marker() {
        let mut store = LocalStore::default();
        let id = store.create_local("P", None).unwrap();
        store
            .set_description(&id, Some("  my notes \n"), false)
            .unwrap();
        assert_eq!(
            store.playlists()[0].description.as_deref(),
            Some("my notes")
        );
        store.set_description(&id, Some("   "), false).unwrap();
        assert_eq!(store.playlists()[0].description, None);
        assert!(
            store
                .set_description(&id, Some(&"a".repeat(2001)), false)
                .is_err()
        );
        assert!(
            store
                .set_description(&id, Some(&"ก".repeat(2000)), false)
                .is_ok()
        );
        assert!(store.set_description("missing", Some("x"), false).is_err());
        // Text cannot pose as a marker.
        assert!(
            store
                .set_description(&id, Some("Imported from YouTube [x]"), false)
                .is_err()
        );

        // An imported playlist keeps its marker as the last line; the text part is what the user edits and what uploads.
        let marked = "Imported from YouTube Music [PL1]";
        let imp = store.create_local("Imp", Some(marked)).unwrap();
        assert_eq!(store.description_text(&imp), None);
        store
            .set_description(&imp, Some("road trip"), false)
            .unwrap();
        assert_eq!(
            store.playlists()[1].description.as_deref(),
            Some("road trip\nImported from YouTube Music [PL1]")
        );
        assert_eq!(store.description_text(&imp).as_deref(), Some("road trip"));
        store.set_description(&imp, None, false).unwrap();
        assert_eq!(store.playlists()[1].description.as_deref(), Some(marked));
        assert_eq!(marker_id(split_note("a\nb\n".trim_end()).0), None);
        assert_eq!(
            split_note("x\nImported from YouTube [WL]"),
            ("x", Some("Imported from YouTube [WL]"))
        );

        // An uploaded playlist is refused unless the server copy already changed.
        store.find_mut(&id).unwrap().remote_id = Some("r".into());
        assert!(store.set_description(&id, Some("y"), false).is_err());
        assert!(store.set_description(&id, Some("y"), true).is_ok());
    }

    #[test]
    fn delete_local_forgets_an_uploaded_playlist_once_the_server_copy_is_gone() {
        let mut store = LocalStore::default();
        let id = store.create_local("P", None).unwrap();
        store.find_mut(&id).unwrap().remote_id = Some("r".into());
        store.mark_refresh("r");
        store.delete_local(&id).unwrap();
        assert!(store.playlists().is_empty());
        assert!(store.refresh_marks().is_empty());
        assert!(store.delete_local(&id).is_err());
    }

    /// Spotify entry carrying ids only (the only shape the backend accepts for Spotify).
    #[test]
    fn a_local_playlist_can_be_renamed_and_reordered_but_an_uploaded_one_cannot() {
        let mut store = LocalStore::default();
        let id = store.create_local("Old", None).unwrap();
        for t in ["a", "b", "c"] {
            store.add_local_entry(&id, spotify(t)).unwrap();
        }
        store.rename_local(&id, "  New name ").unwrap();
        assert_eq!(store.playlists()[0].name, "New name");
        assert!(store.rename_local(&id, "   ").is_err());
        store.move_entry(&id, 0, 2).unwrap();
        let order: Vec<_> = store.playlists()[0]
            .entries
            .iter()
            .map(|e| e.provider_track_id.clone())
            .collect();
        assert_eq!(order, ["b", "c", "a"]);
        assert!(store.move_entry(&id, 3, 0).is_err());
        assert!(store.move_entry(&id, 0, 3).is_err());
        store.playlists[0].remote_id = Some("remote".into());
        assert!(store.rename_local(&id, "X").is_err());
        assert!(store.move_entry(&id, 0, 1).is_err());
        assert_eq!(store.playlists()[0].name, "New name");
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

    #[tokio::test]
    async fn create_and_add_while_signed_out_make_no_requests() {
        let s = stub(vec![raw("500 Internal Server Error", INTERNAL)]).await;
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", Some("notes")).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        store.add_local_entry(&id, spotify("track-b")).unwrap();
        assert_eq!(s.count(), 0);
        assert_eq!(store.playlists[0].entries.len(), 2);
        assert_eq!(store.playlists[0].remote_id, None);
    }

    #[tokio::test]
    async fn spotify_entry_with_metadata_is_refused_locally() {
        let cases: [fn(&mut LocalEntry); 4] = [
            |e| e.title = Some("Song".into()),
            |e| e.artists = Some(vec!["Band".into()]),
            |e| e.artwork_url = Some("https://img.example.test/a.jpg".into()),
            |e| e.duration_ms = Some(1000),
        ];
        let s = stub(vec![raw("201 Created", "{}")]).await;
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", None).unwrap();
        for set in cases {
            let mut entry = spotify("track-a");
            set(&mut entry);
            let err = store.add_local_entry(&id, entry).unwrap_err();
            assert!(matches!(err, SyncError::Local(_)), "{err:?}");
        }
        assert!(store.playlists[0].entries.is_empty());
        assert_eq!(s.count(), 0);
    }

    #[test]
    fn spotify_entry_with_only_ids_is_accepted() {
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", None).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        assert_eq!(store.playlists[0].entries.len(), 1);
    }

    #[test]
    fn name_limits_follow_backend_rules() {
        let mut store = LocalStore::default();
        assert!(matches!(
            store.create_local("   ", None),
            Err(SyncError::Local(_))
        ));
        assert!(matches!(
            store.create_local(&"a".repeat(201), None),
            Err(SyncError::Local(_))
        ));
        assert!(matches!(
            store.create_local("Mix", Some(&"d".repeat(2001))),
            Err(SyncError::Local(_))
        ));
        assert!(store.playlists.is_empty());
        store
            .create_local(&format!("  {}  ", "a".repeat(200)), None)
            .unwrap();
        store.create_local(" x ", None).unwrap();
        assert_eq!(store.playlists[0].name.chars().count(), 200);
        assert_eq!(store.playlists[1].name, "x");
    }

    #[tokio::test]
    async fn upload_creates_once_and_adds_entries_in_order() {
        let s = stub(vec![
            created(PLAYLIST, 1),
            added("track-a", 2),
            added("track-b", 3),
        ])
        .await;
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", Some("notes")).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        store.add_local_entry(&id, spotify("track-b")).unwrap();
        let remote = store.upload(&client(&s.base), SENTINEL, &id).await.unwrap();
        assert_eq!(remote, PLAYLIST);
        let log = s.log();
        assert_eq!(log.len(), 3);
        assert_eq!(log[0].0, CREATE_LINE);
        assert_eq!(log[0].1, json!({"name": "Mix", "description": "notes"}));
        assert_eq!(log[1].0, ENTRIES_LINE);
        assert_eq!(log[1].1["providerTrackId"], "track-a");
        assert_eq!(log[1].1["expectedVersion"], 1);
        assert_eq!(log[2].0, ENTRIES_LINE);
        assert_eq!(log[2].1["providerTrackId"], "track-b");
        assert_eq!(log[2].1["expectedVersion"], 2);
        assert_eq!(store.playlists[0].remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(store.playlists[0].uploaded, 2);
    }

    #[tokio::test]
    async fn upload_twice_returns_same_remote_id_with_zero_second_requests() {
        let s = stub(vec![created(PLAYLIST, 1), added("track-a", 2)]).await;
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", None).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        let api = client(&s.base);
        let first = store.upload(&api, SENTINEL, &id).await.unwrap();
        let after_first = s.count();
        let second = store.upload(&api, SENTINEL, &id).await.unwrap();
        assert_eq!(first, PLAYLIST);
        assert_eq!(second, PLAYLIST);
        assert_eq!(s.count(), after_first);
        assert_eq!(s.count(), 2);
    }

    #[tokio::test]
    async fn failed_entry_keeps_remote_id_and_retry_sends_only_remaining() {
        let s = stub(vec![
            created(PLAYLIST, 1),
            added("track-a", 2),
            raw("500 Internal Server Error", INTERNAL),
            added("track-b", 3),
        ])
        .await;
        let api = client(&s.base);
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", None).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        store.add_local_entry(&id, spotify("track-b")).unwrap();

        let err = store.upload(&api, SENTINEL, &id).await.unwrap_err();
        assert!(matches!(err, SyncError::Internal(_)), "{err:?}");
        assert_eq!(store.playlists[0].remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(store.playlists[0].uploaded, 1);
        assert_eq!(s.count(), 3);

        let remote = store.upload(&api, SENTINEL, &id).await.unwrap();
        assert_eq!(remote, PLAYLIST);
        let log = s.log();
        assert_eq!(log.len(), 4, "retry must send exactly one request");
        assert_eq!(log[3].0, ENTRIES_LINE);
        assert_eq!(log[3].1["providerTrackId"], "track-b");
        assert_eq!(log[3].1["expectedVersion"], 2);
        assert_eq!(
            log.iter().filter(|(line, _)| line == CREATE_LINE).count(),
            1
        );
        assert_eq!(store.playlists[0].uploaded, 2);
    }

    #[test]
    fn discarded_attempted_remove_is_not_reinserted_and_marks_playlist_for_refresh() {
        let mut store = LocalStore::from_playlists(vec![LocalPlaylist {
            id: "local-1".into(),
            name: "Mix".into(),
            description: None,
            entries: vec![LocalEntry {
                remote_entry_id: Some("entry-b".into()),
                ..spotify("track-b")
            }],
            remote_id: Some(PLAYLIST.into()),
            uploaded: 1,
            queued: 0,
            version: 3,
        }]);
        let op = WriteOp::RemoveEntry {
            entry_id: "entry-a".into(),
            expected_version: 2,
            index: 0,
            entry: spotify("track-a"),
            attempted: true,
        };
        store.discard_queued(vec![(PLAYLIST.into(), op)]);
        let pl = &store.playlists()[0];
        assert_eq!(
            pl.entries.len(),
            1,
            "the attempted removal is not reinserted"
        );
        assert_eq!(pl.uploaded, 1);
        assert_eq!(store.refresh_marks(), [PLAYLIST.to_string()]);
    }

    #[tokio::test]
    async fn token_never_appears_in_debug_or_errors() {
        let s = stub(vec![
            created(PLAYLIST, 1),
            raw("500 Internal Server Error", INTERNAL),
        ])
        .await;
        let mut store = LocalStore::default();
        let id = store.create_local("Mix", None).unwrap();
        store.add_local_entry(&id, spotify("track-a")).unwrap();
        let err = store
            .upload(&client(&s.base), SENTINEL, &id)
            .await
            .unwrap_err();
        let out = format!("{store:?} {err:?} {err}");
        assert!(!out.contains(SENTINEL), "{out}");
        assert!(
            s.log()
                .iter()
                .all(|(_, body)| !body.to_string().contains(SENTINEL))
        );
    }
}
