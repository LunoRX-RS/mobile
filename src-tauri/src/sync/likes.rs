//! Liked-songs sync. The account on the backend holds the like set and a monotonic revision; the local file is a cache
//! of the last snapshot plus the changes this device has not delivered yet (the outbox). This module holds the rules
//! and the socket loop; `commands::likes` supplies the Tauri-backed local store and starts it.
//!
//! Signed-out likes stay in the plain `liked-songs.json`. The first account that signs in claims that file once, so a
//! second account never inherits another person's likes.
use std::collections::{BTreeMap, HashSet};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use tokio::sync::{Notify, mpsc, watch};

use crate::core::domain::ProviderId;
use crate::core::library::SavedTrack;
use crate::core::search::hit::SearchSource;
use crate::sync::api::{ApiClient, LikeEntry, LikeSnapshot, LikeTrack, LikeTrackBody, SyncError};
use crate::sync::credentials::TokenStore;
use crate::sync::remote::{self, LinkStatus};

/// The backend's cap; the local library enforces the same number.
pub const CAP: usize = 5000;
const CHUNK_TRACKS: usize = 500;
const CHUNK_BYTES: usize = 1024 * 1024;
const FRAME_QUEUE: usize = 256;
const FLUSH_EVERY: Duration = Duration::from_secs(30);
const WAIT_SIGNED_OUT: Duration = Duration::from_secs(30);
const WAIT_RETRY: Duration = Duration::from_secs(10);
const ANON_MARKER: &str = ".anonymous-claimed";

/// How a backend call failed. The outbox drops permanent failures and waits on the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendError {
    /// No token, or the backend refused it. Nothing is dropped; wait for a sign-in.
    SignedOut,
    /// Network, timeout, 429 or 5xx: keep the outbox and try again.
    Offline,
    /// A permanent 4xx: retrying cannot help, so the entry is dropped.
    Rejected,
}

/// The account's file could not be opened; the current one is still in use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unavailable;

/// The local like store. Everything here is synchronous: the engine only calls it between awaits.
pub trait Local: Send + Sync + 'static {
    fn list(&self) -> Vec<SavedTrack>;
    fn replace(&self, tracks: Vec<SavedTrack>);
    fn upsert(&self, track: SavedTrack);
    fn remove(&self, provider: &ProviderId, id: &str);
    /// Opens another account's file. `Err` means it could not be opened and the current one is still in use.
    fn switch(&self, path: &Path) -> Result<(), Unavailable>;
}

/// The backend calls the engine needs. The real implementation reads the session token per call; tests use a fake.
pub trait Backend: Send + Sync + 'static {
    fn session(&self) -> impl std::future::Future<Output = Result<String, BackendError>> + Send;
    fn snapshot(
        &self,
    ) -> impl std::future::Future<Output = Result<LikeSnapshot, BackendError>> + Send;
    fn get(
        &self,
        provider: &str,
        id: &str,
    ) -> impl std::future::Future<Output = Result<LikeEntry, BackendError>> + Send;
    fn put(
        &self,
        track: &LikeTrack,
    ) -> impl std::future::Future<Output = Result<(), BackendError>> + Send;
    fn delete(
        &self,
        provider: &str,
        id: &str,
    ) -> impl std::future::Future<Output = Result<(), BackendError>> + Send;
    fn merge(
        &self,
        tracks: &[LikeTrack],
    ) -> impl std::future::Future<Output = Result<(), BackendError>> + Send;
}

/// `ApiClient` plus the token store. The token is fetched for each call and never kept here.
pub struct HttpBackend {
    api: ApiClient,
    tokens: Arc<dyn TokenStore + Send + Sync>,
}

impl HttpBackend {
    pub fn new(api: ApiClient, tokens: Arc<dyn TokenStore + Send + Sync>) -> Self {
        Self { api, tokens }
    }

    fn token(&self) -> Result<String, BackendError> {
        self.tokens
            .get()
            .map_err(|_| BackendError::Offline)?
            .ok_or(BackendError::SignedOut)
    }
}

fn permanent(err: SyncError) -> BackendError {
    match err {
        SyncError::Unauthenticated(_) => BackendError::SignedOut,
        SyncError::Validation(_)
        | SyncError::NotFound(_)
        | SyncError::Conflict { .. }
        | SyncError::Settings(_)
        | SyncError::Local(_) => BackendError::Rejected,
        SyncError::Forbidden(_) | SyncError::RateLimited { .. } | SyncError::Internal(_) => {
            BackendError::Offline
        }
        SyncError::Network => BackendError::Offline,
    }
}

impl Backend for HttpBackend {
    async fn session(&self) -> Result<String, BackendError> {
        let token = self.token()?;
        self.api
            .get_session(&token)
            .await
            .map(|s| s.user_id)
            .map_err(permanent)
    }

    async fn snapshot(&self) -> Result<LikeSnapshot, BackendError> {
        let token = self.token()?;
        self.api.list_likes(&token).await.map_err(permanent)
    }

    async fn get(&self, provider: &str, id: &str) -> Result<LikeEntry, BackendError> {
        let token = self.token()?;
        self.api
            .get_like(&token, provider, id)
            .await
            .map_err(permanent)
    }

    async fn put(&self, track: &LikeTrack) -> Result<(), BackendError> {
        let token = self.token()?;
        let body = LikeTrackBody {
            canonical_url: &track.canonical_url,
            title: &track.title,
            artists: &track.artists,
            artist_refs: &track.artist_refs,
            source: track.source.as_deref(),
            artwork_url: track.artwork_url.as_deref(),
            duration_ms: track.duration_ms,
        };
        self.api
            .put_like(&token, &track.provider, &track.provider_track_id, &body)
            .await
            .map_err(permanent)
    }

    async fn delete(&self, provider: &str, id: &str) -> Result<(), BackendError> {
        let token = self.token()?;
        self.api
            .delete_like(&token, provider, id)
            .await
            .map_err(permanent)
    }

    async fn merge(&self, tracks: &[LikeTrack]) -> Result<(), BackendError> {
        let token = self.token()?;
        self.api
            .merge_likes(&token, tracks)
            .await
            .map(|_| ())
            .map_err(permanent)
    }
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct SyncState {
    pub revision: i64,
    pub initialized: bool,
}

#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Outbox {
    pending: BTreeMap<String, Pending>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pending {
    liked: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    track: Option<SavedTrack>,
    /// Bumped on every toggle, so a completing send settles only the version it delivered.
    at: u64,
}

/// One account's file locations. The directory is `likes/<userId>/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    pub user_id: String,
    pub library: PathBuf,
    outbox: PathBuf,
    state: PathBuf,
}

pub fn account_at(root: &Path, user_id: &str) -> Account {
    let dir = root.join("likes").join(user_id);
    Account {
        user_id: user_id.to_owned(),
        library: dir.join("liked-songs.json"),
        outbox: dir.join("outbox.json"),
        state: dir.join("sync.json"),
    }
}

/// The file the app opens before anyone signs in.
pub fn anonymous_path(root: &Path) -> PathBuf {
    root.join("liked-songs.json")
}

/// Copies the signed-out likes into the first account's file, once per installation. Later accounts never inherit them.
/// Returns whether likes were copied.
pub fn claim_anonymous(root: &Path, account: &Account) -> bool {
    let marker = root.join("likes").join(ANON_MARKER);
    if marker.exists() {
        return false;
    }
    let legacy = anonymous_path(root);
    let mut claimed = false;
    if !account.library.exists()
        && let Ok(bytes) = std::fs::read(&legacy)
        && bytes.len() <= 8 * 1024 * 1024
    {
        claimed = write_atomic(&account.library, &bytes).is_ok();
    }
    // The marker is written even when there was nothing to copy: a later account must not inherit the file.
    let _ = write_atomic(&marker, b"1");
    claimed
}

fn write_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent"))?;
    std::fs::create_dir_all(parent)?;
    let part = path.with_extension(format!("{:032x}.part", rand::random::<u128>()));
    let result = (|| {
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        std::fs::rename(&part, path)
    })();
    let _ = std::fs::remove_file(part);
    result
}

fn load<T: DeserializeOwned + Default>(path: &Path, max: usize) -> T {
    std::fs::read(path)
        .ok()
        .filter(|bytes| bytes.len() <= max)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

fn save<T: Serialize>(path: &Path, value: &T) {
    if let Ok(bytes) = serde_json::to_vec(value) {
        let _ = write_atomic(path, &bytes);
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// State shared with `commands::likes`: the current account, its revision, the outbox, and the wake-ups.
pub struct Shared {
    root: PathBuf,
    account: Mutex<Option<Account>>,
    state: Mutex<SyncState>,
    outbox: Mutex<Outbox>,
    /// Held while a local like is written and while the account changes, so a toggle cannot land in the wrong outbox.
    pub gate: Mutex<()>,
    /// Sign-in or sign-out: the loop must re-read the token.
    pub kick: Notify,
    /// A like was queued: flush the outbox.
    pub flush: Notify,
}

impl Shared {
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            account: Mutex::new(None),
            state: Mutex::new(SyncState::default()),
            outbox: Mutex::new(Outbox::default()),
            gate: Mutex::new(()),
            kick: Notify::new(),
            flush: Notify::new(),
        }
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn account(&self) -> Option<Account> {
        self.account
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    fn enter(&self, account: Account) {
        let state = load(&account.state, 64 * 1024);
        let outbox = load(&account.outbox, 4 * 1024 * 1024);
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = state;
        *self.outbox.lock().unwrap_or_else(PoisonError::into_inner) = outbox;
        *self.account.lock().unwrap_or_else(PoisonError::into_inner) = Some(account);
    }

    fn leave(&self) {
        *self.account.lock().unwrap_or_else(PoisonError::into_inner) = None;
        *self.state.lock().unwrap_or_else(PoisonError::into_inner) = SyncState::default();
        *self.outbox.lock().unwrap_or_else(PoisonError::into_inner) = Outbox::default();
    }

    fn revision(&self) -> i64 {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .revision
    }

    fn initialized(&self) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .initialized
    }

    /// Records a revision the local list now matches and persists it with the account.
    fn advance(&self, revision: i64) {
        let account = self.account();
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.revision = revision;
        state.initialized = true;
        if let Some(account) = &account {
            save(&account.state, &*state);
        }
    }

    /// Queues one toggle for the current account. False when signed out: the local file still holds the like.
    pub fn note(&self, track: &SavedTrack, liked: bool) -> bool {
        let Some(account) = self.account() else {
            return false;
        };
        let key = key_of(&track.provider, &track.provider_track_id);
        let mut outbox = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
        outbox.pending.insert(
            key,
            Pending {
                liked,
                track: liked.then(|| track.clone()),
                at: now_ms(),
            },
        );
        save(&account.outbox, &*outbox);
        true
    }

    fn pending(&self, key: &str) -> Option<Pending> {
        self.outbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
            .get(key)
            .cloned()
    }

    fn outbox(&self) -> Outbox {
        self.outbox
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Removes an entry only when it is still the version that was sent.
    fn settle(&self, key: &str, sent: &Pending) {
        let Some(account) = self.account() else {
            return;
        };
        let mut outbox = self.outbox.lock().unwrap_or_else(PoisonError::into_inner);
        if outbox
            .pending
            .get(key)
            .is_some_and(|p| p.liked == sent.liked && p.at == sent.at)
        {
            outbox.pending.remove(key);
            save(&account.outbox, &*outbox);
        }
    }
}

fn key_of(provider: &ProviderId, id: &str) -> String {
    format!("{}:{id}", provider.as_str())
}

fn split_key(key: &str) -> Option<(&str, &str)> {
    key.split_once(':')
}

fn source_str(source: SearchSource) -> &'static str {
    match source {
        SearchSource::YouTubeMusic => "YouTube Music",
        SearchSource::YouTube => "YouTube",
    }
}

fn wire(track: &SavedTrack) -> LikeTrack {
    LikeTrack {
        provider: track.provider.as_str().to_owned(),
        provider_track_id: track.provider_track_id.clone(),
        canonical_url: track.canonical_url.clone(),
        title: track.title.clone(),
        artists: track.artists.clone(),
        artist_refs: track
            .artist_refs
            .iter()
            .map(|a| crate::sync::api::LikeArtistRef {
                provider: a.provider.as_str().to_owned(),
                id: a.id.clone(),
                name: a.name.clone(),
            })
            .collect(),
        source: track.source.map(|s| source_str(s).to_owned()),
        artwork_url: track.artwork_url.clone(),
        duration_ms: track.duration_ms.and_then(|d| i64::try_from(d).ok()),
    }
}

/// Server record to local record. Unknown providers or sources are dropped, and the local validators have the final say,
/// so a buggy or hostile server cannot poison the file's deserialization.
fn saved(track: &LikeTrack) -> Option<SavedTrack> {
    let provider: ProviderId = track.provider.parse().ok()?;
    let source = track.source.as_deref().and_then(|s| match s {
        "YouTube Music" => Some(SearchSource::YouTubeMusic),
        "YouTube" => Some(SearchSource::YouTube),
        _ => None,
    });
    let mut saved = SavedTrack {
        source,
        provider: provider.clone(),
        provider_track_id: track.provider_track_id.clone(),
        canonical_url: track.canonical_url.clone(),
        title: track.title.clone(),
        artists: track.artists.clone(),
        artist_refs: track
            .artist_refs
            .iter()
            .filter_map(|a| {
                Some(crate::core::search::hit::ArtistRef {
                    provider: a.provider.parse().ok()?,
                    id: a.id.clone(),
                    name: a.name.clone(),
                })
            })
            .collect(),
        artwork_url: track.artwork_url.clone(),
        duration_ms: track.duration_ms.and_then(|d| u64::try_from(d).ok()),
    };
    saved.validate().ok()?;
    Some(saved)
}

/// What an inbound frame asks for, given the revision the local list matches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reaction {
    Ignore,
    Apply(Event),
    Snapshot,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Liked {
        revision: i64,
        provider: String,
        track_id: String,
    },
    Unliked {
        revision: i64,
        provider: String,
        track_id: String,
    },
}

#[derive(Deserialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Frame {
    Liked {
        revision: i64,
        provider: String,
        provider_track_id: String,
    },
    Unliked {
        revision: i64,
        provider: String,
        provider_track_id: String,
    },
    Reset {
        revision: i64,
    },
    Hello {
        revision: i64,
    },
    Tick {
        revision: i64,
    },
}

/// The revision rules. An event exactly one ahead applies; older ones are echoes; anything else (a gap, a differing
/// hello or tick, a reset) pulls the snapshot, which heals a lost notification.
pub fn reaction(revision: i64, text: &str) -> Reaction {
    let Ok(frame) = serde_json::from_str::<Frame>(text) else {
        return Reaction::Ignore;
    };
    match frame {
        Frame::Liked {
            revision: r,
            provider,
            provider_track_id,
        } => step(
            revision,
            r,
            Event::Liked {
                revision: r,
                provider,
                track_id: provider_track_id,
            },
        ),
        Frame::Unliked {
            revision: r,
            provider,
            provider_track_id,
        } => step(
            revision,
            r,
            Event::Unliked {
                revision: r,
                provider,
                track_id: provider_track_id,
            },
        ),
        Frame::Reset { revision: r } => {
            if r > revision {
                Reaction::Snapshot
            } else {
                Reaction::Ignore
            }
        }
        Frame::Hello { revision: r } | Frame::Tick { revision: r } => {
            if r == revision {
                Reaction::Ignore
            } else {
                Reaction::Snapshot
            }
        }
    }
}

fn step(revision: i64, incoming: i64, event: Event) -> Reaction {
    if incoming == revision + 1 {
        Reaction::Apply(event)
    } else if incoming <= revision {
        Reaction::Ignore
    } else {
        Reaction::Snapshot
    }
}

/// Splits the local list into merge requests below both the backend's body and track limits.
fn chunks(tracks: &[SavedTrack]) -> Vec<Vec<SavedTrack>> {
    let mut out = Vec::new();
    let mut current: Vec<SavedTrack> = Vec::new();
    let mut bytes = 2;
    for track in tracks {
        let size = serde_json::to_vec(&wire(track))
            .map(|b| b.len() + 1)
            .unwrap_or(CHUNK_BYTES + 1);
        if !current.is_empty() && (current.len() >= CHUNK_TRACKS || bytes + size > CHUNK_BYTES) {
            out.push(std::mem::take(&mut current));
            bytes = 2;
        }
        if size > CHUNK_BYTES {
            continue;
        }
        current.push(track.clone());
        bytes += size;
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// Puts the unsent intent back on top of a server list: pending likes first (in key order), pending unlikes removed.
fn overlay(mut tracks: Vec<SavedTrack>, outbox: &Outbox) -> Vec<SavedTrack> {
    for (key, pending) in &outbox.pending {
        let Some((provider, id)) = split_key(key) else {
            continue;
        };
        let Ok(provider) = provider.parse::<ProviderId>() else {
            continue;
        };
        tracks.retain(|t| t.provider != provider || t.provider_track_id != id);
        if pending.liked
            && let Some(track) = pending.track.clone()
        {
            tracks.insert(0, track);
        }
    }
    let mut seen = HashSet::new();
    tracks.retain(|t| seen.insert((t.provider.clone(), t.provider_track_id.clone())));
    tracks.truncate(CAP);
    tracks
}

struct Session<L: Local, B: Backend> {
    shared: Arc<Shared>,
    local: Arc<L>,
    backend: Arc<B>,
    on_change: Arc<dyn Fn() + Send + Sync>,
}

impl<L: Local, B: Backend> Session<L, B> {
    fn changed(&self) {
        (self.on_change)();
    }

    /// Uploads the local list the first time this account syncs, then adopts the server snapshot.
    async fn first_sync(&self) -> Result<(), BackendError> {
        if !self.shared.initialized() {
            for chunk in chunks(&self.local.list()) {
                let wire: Vec<LikeTrack> = chunk.iter().map(wire).collect();
                self.backend.merge(&wire).await?;
            }
        }
        self.pull().await
    }

    /// Replaces the local list with the server's and re-applies unsent intent.
    async fn pull(&self) -> Result<(), BackendError> {
        let snapshot = self.backend.snapshot().await?;
        let tracks = snapshot.tracks.iter().filter_map(saved).take(CAP).collect();
        let tracks = overlay(tracks, &self.shared.outbox());
        self.local.replace(tracks);
        self.shared.advance(snapshot.revision);
        self.changed();
        Ok(())
    }

    async fn flush(&self) {
        loop {
            let Some((key, pending)) = self.shared.outbox().pending.into_iter().next() else {
                return;
            };
            let Some((provider, id)) = split_key(&key) else {
                self.shared.settle(&key, &pending);
                continue;
            };
            let result = if pending.liked {
                let track = pending
                    .track
                    .clone()
                    .or_else(|| find(&self.local.list(), provider, id));
                match track {
                    Some(track) => self.backend.put(&wire(&track)).await,
                    // The local list no longer holds the track, so there is no metadata to send.
                    None => {
                        self.shared.settle(&key, &pending);
                        continue;
                    }
                }
            } else {
                self.backend.delete(provider, id).await
            };
            match result {
                Ok(()) | Err(BackendError::Rejected) => self.shared.settle(&key, &pending),
                Err(_) => return,
            }
        }
    }

    async fn handle_frame(&self, text: &str) {
        match reaction(self.shared.revision(), text) {
            Reaction::Ignore => {}
            Reaction::Snapshot => {
                let _ = self.pull().await;
            }
            Reaction::Apply(Event::Liked {
                revision,
                provider,
                track_id,
            }) => {
                let Ok(provider_id) = provider.parse::<ProviderId>() else {
                    self.shared.advance(revision);
                    return;
                };
                let key = key_of(&provider_id, &track_id);
                if self.shared.pending(&key).is_none() {
                    match self.backend.get(&provider, &track_id).await {
                        Ok(entry) => {
                            if let Some(track) = entry.track.as_ref().and_then(saved)
                                && entry.liked
                            {
                                self.local.upsert(track);
                            } else {
                                self.local.remove(&provider_id, &track_id);
                            }
                            self.changed();
                        }
                        // Do not advance: the next tick sees the gap and pulls the snapshot.
                        Err(_) => return,
                    }
                }
                self.shared.advance(revision);
            }
            Reaction::Apply(Event::Unliked {
                revision,
                provider,
                track_id,
            }) => {
                let Ok(provider_id) = provider.parse::<ProviderId>() else {
                    self.shared.advance(revision);
                    return;
                };
                let key = key_of(&provider_id, &track_id);
                if self.shared.pending(&key).is_none() {
                    self.local.remove(&provider_id, &track_id);
                    self.changed();
                }
                self.shared.advance(revision);
            }
        }
    }
}

fn find(tracks: &[SavedTrack], provider: &str, id: &str) -> Option<SavedTrack> {
    let provider: ProviderId = provider.parse().ok()?;
    tracks
        .iter()
        .find(|t| t.provider == provider && t.provider_track_id == id)
        .cloned()
}

/// Opens the signed-out file and forgets the account. Returns whether anything changed.
fn use_anonymous<L: Local>(shared: &Shared, local: &L) -> bool {
    let _gate = shared.gate.lock().unwrap_or_else(PoisonError::into_inner);
    if shared.account().is_none() {
        return false;
    }
    if local.switch(&anonymous_path(shared.root())).is_err() {
        return false;
    }
    shared.leave();
    true
}

fn use_account<L: Local>(shared: &Shared, local: &L, user: &str) -> Option<Account> {
    let _gate = shared.gate.lock().unwrap_or_else(PoisonError::into_inner);
    let account = account_at(shared.root(), user);
    claim_anonymous(shared.root(), &account);
    if local.switch(&account.library).is_err() {
        return None;
    }
    shared.enter(account.clone());
    Some(account)
}

async fn wait(shared: &Shared, at_most: Duration) {
    let _ = tokio::time::timeout(at_most, shared.kick.notified()).await;
}

/// Keeps this device in step with the account: merge on first sync, flush the outbox, listen on the socket and heal
/// with a snapshot whenever a revision does not line up. Runs until the task is dropped.
pub async fn run<L: Local, B: Backend>(
    shared: Arc<Shared>,
    local: Arc<L>,
    backend: Arc<B>,
    tokens: Arc<dyn TokenStore + Send + Sync>,
    base_url: String,
    on_change: Arc<dyn Fn() + Send + Sync>,
) {
    loop {
        if tokens.get().ok().flatten().is_none() {
            if use_anonymous(&shared, local.as_ref()) {
                on_change();
            }
            wait(&shared, WAIT_SIGNED_OUT).await;
            continue;
        }
        let user = match backend.session().await {
            Ok(user) => user,
            Err(BackendError::SignedOut) => {
                if use_anonymous(&shared, local.as_ref()) {
                    on_change();
                }
                wait(&shared, WAIT_SIGNED_OUT).await;
                continue;
            }
            Err(_) => {
                wait(&shared, WAIT_RETRY).await;
                continue;
            }
        };
        let current = shared.account().map(|a| a.user_id);
        let account = if current.as_deref() == Some(user.as_str()) {
            shared.account()
        } else {
            None
        };
        let account = match account.or_else(|| use_account(&shared, local.as_ref(), &user)) {
            Some(account) => account,
            None => {
                wait(&shared, WAIT_RETRY).await;
                continue;
            }
        };
        if current.as_deref() != Some(account.user_id.as_str()) {
            on_change();
        }
        let session = Session {
            shared: shared.clone(),
            local: local.clone(),
            backend: backend.clone(),
            on_change: on_change.clone(),
        };
        if session.first_sync().await.is_err() {
            wait(&shared, WAIT_RETRY).await;
            continue;
        }
        session.flush().await;

        let Some(url) = remote::likes_socket_url(&base_url) else {
            wait(&shared, WAIT_RETRY).await;
            continue;
        };
        let (keep_open, outbound) = mpsc::channel::<String>(1);
        let _keep_open = keep_open;
        let (frames_tx, mut frames) = mpsc::channel::<String>(FRAME_QUEUE);
        let (backlog_tx, mut backlog) = mpsc::unbounded_channel::<()>();
        let (status_tx, mut status) = watch::channel(LinkStatus::Connecting);
        let mut link = pin!(remote::run(
            url,
            &*tokens,
            outbound,
            move |text| {
                if frames_tx.try_send(text).is_err() {
                    // The engine is behind; one snapshot catches up on everything queued.
                    let _ = backlog_tx.send(());
                }
            },
            move |s| {
                let _ = status_tx.send(s);
            },
        ));
        let mut ticker = tokio::time::interval(FLUSH_EVERY);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut stop = false;
        while !stop {
            tokio::select! {
                _ = link.as_mut() => break,
                _ = shared.kick.notified() => stop = true,
                _ = shared.flush.notified() => session.flush().await,
                backlog = backlog.recv() => {
                    if backlog.is_some() {
                        let _ = session.pull().await;
                    }
                }
                frame = frames.recv() => {
                    match frame {
                        Some(frame) => session.handle_frame(&frame).await,
                        None => stop = true,
                    }
                }
                changed = status.changed() => {
                    if changed.is_err() {
                        break;
                    }
                    let now = *status.borrow_and_update();
                    match now {
                        LinkStatus::Online => session.flush().await,
                        LinkStatus::SignedOut => stop = true,
                        _ => {}
                    }
                }
                _ = ticker.tick() => session.flush().await,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(id: &str) -> SavedTrack {
        SavedTrack {
            source: Some(SearchSource::YouTubeMusic),
            provider: ProviderId::YouTube,
            provider_track_id: id.into(),
            canonical_url: format!("https://www.youtube.com/watch?v={id}"),
            title: format!("T{id}"),
            artists: vec!["A".into()],
            artist_refs: Vec::new(),
            artwork_url: None,
            duration_ms: Some(1000),
        }
    }

    fn root() -> PathBuf {
        std::env::temp_dir().join(format!("lunor-likes-sync-{:032x}", rand::random::<u128>()))
    }

    #[derive(Default)]
    struct FakeLocal {
        tracks: Mutex<Vec<SavedTrack>>,
    }

    impl Local for FakeLocal {
        fn list(&self) -> Vec<SavedTrack> {
            self.tracks.lock().unwrap().clone()
        }
        fn replace(&self, tracks: Vec<SavedTrack>) {
            *self.tracks.lock().unwrap() = tracks;
        }
        fn upsert(&self, track: SavedTrack) {
            let mut tracks = self.tracks.lock().unwrap();
            tracks.retain(|t| {
                t.provider != track.provider || t.provider_track_id != track.provider_track_id
            });
            tracks.insert(0, track);
        }
        fn remove(&self, provider: &ProviderId, id: &str) {
            self.tracks
                .lock()
                .unwrap()
                .retain(|t| t.provider != *provider || t.provider_track_id != id);
        }
        fn switch(&self, _: &Path) -> Result<(), Unavailable> {
            Ok(())
        }
    }

    struct FakeBackend {
        snapshot: Mutex<LikeSnapshot>,
        merged: Mutex<Vec<Vec<LikeTrack>>>,
        put: Mutex<Vec<String>>,
        deleted: Mutex<Vec<String>>,
        fail_put: bool,
    }

    impl Default for FakeBackend {
        fn default() -> Self {
            Self {
                snapshot: Mutex::new(LikeSnapshot {
                    revision: 0,
                    tracks: Vec::new(),
                }),
                merged: Mutex::new(Vec::new()),
                put: Mutex::new(Vec::new()),
                deleted: Mutex::new(Vec::new()),
                fail_put: false,
            }
        }
    }

    impl Backend for FakeBackend {
        async fn session(&self) -> Result<String, BackendError> {
            Ok("user".into())
        }
        async fn snapshot(&self) -> Result<LikeSnapshot, BackendError> {
            Ok(self.snapshot.lock().unwrap().clone())
        }
        async fn get(&self, _: &str, _: &str) -> Result<LikeEntry, BackendError> {
            Ok(LikeEntry {
                liked: false,
                track: None,
            })
        }
        async fn put(&self, track: &LikeTrack) -> Result<(), BackendError> {
            if self.fail_put {
                return Err(BackendError::Offline);
            }
            self.put
                .lock()
                .unwrap()
                .push(track.provider_track_id.clone());
            Ok(())
        }
        async fn delete(&self, _: &str, id: &str) -> Result<(), BackendError> {
            self.deleted.lock().unwrap().push(id.into());
            Ok(())
        }
        async fn merge(&self, tracks: &[LikeTrack]) -> Result<(), BackendError> {
            self.merged.lock().unwrap().push(tracks.to_vec());
            Ok(())
        }
    }

    fn session(
        shared: Arc<Shared>,
        local: Arc<FakeLocal>,
        backend: Arc<FakeBackend>,
    ) -> Session<FakeLocal, FakeBackend> {
        shared.enter(account_at(shared.root(), "user"));
        Session {
            shared,
            local,
            backend,
            on_change: Arc::new(|| {}),
        }
    }

    #[test]
    fn events_apply_only_from_the_next_revision_and_gaps_pull_a_snapshot() {
        let liked = |revision: i64| {
            format!(
                r#"{{"type":"liked","revision":{revision},"provider":"YouTube","providerTrackId":"abc"}}"#
            )
        };
        assert!(matches!(reaction(4, &liked(5)), Reaction::Apply(_)));
        assert_eq!(reaction(5, &liked(5)), Reaction::Ignore);
        assert_eq!(reaction(5, &liked(4)), Reaction::Ignore);
        assert_eq!(reaction(5, &liked(7)), Reaction::Snapshot);
        assert_eq!(
            reaction(5, r#"{"type":"tick","revision":6}"#),
            Reaction::Snapshot
        );
        assert_eq!(
            reaction(5, r#"{"type":"hello","revision":5}"#),
            Reaction::Ignore
        );
        assert_eq!(
            reaction(5, r#"{"type":"reset","revision":6}"#),
            Reaction::Snapshot
        );
        assert_eq!(reaction(5, "not json"), Reaction::Ignore);
    }

    #[test]
    fn the_outbox_keeps_the_latest_toggle_and_a_send_settles_its_own_version() {
        let dir = root();
        let shared = Shared::new(dir.clone());
        shared.enter(account_at(&dir, "user"));
        assert!(shared.note(&track("abc"), true));
        assert!(shared.note(&track("abc"), false));
        let key = "YouTube:abc";
        assert!(!shared.pending(key).unwrap().liked);
        let sent = shared.pending(key).unwrap();
        // The user flips it again while the first send is in flight.
        assert!(shared.note(&track("abc"), true));
        shared.settle(key, &sent);
        assert!(shared.pending(key).unwrap().liked);
        shared.settle(key, &shared.pending(key).unwrap());
        assert!(shared.pending(key).is_none());
        let anonymous = Shared::new(root());
        assert!(
            !anonymous.note(&track("abc"), true),
            "signed out is a no-op"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_snapshot_replace_puts_the_unsent_intent_back_on_top() {
        let outbox = Outbox {
            pending: BTreeMap::from([
                (
                    "YouTube:ccc".to_owned(),
                    Pending {
                        liked: true,
                        track: Some(track("ccc")),
                        at: 1,
                    },
                ),
                (
                    "YouTube:bbb".to_owned(),
                    Pending {
                        liked: false,
                        track: None,
                        at: 2,
                    },
                ),
            ]),
        };
        let server = vec![track("aaa"), track("bbb")];
        let merged = overlay(server, &outbox);
        assert_eq!(
            merged
                .iter()
                .map(|t| t.provider_track_id.as_str())
                .collect::<Vec<_>>(),
            vec!["ccc", "aaa"]
        );
    }

    #[tokio::test]
    async fn a_pending_outbox_entry_survives_a_snapshot_pull() {
        let dir = root();
        let shared = Arc::new(Shared::new(dir.clone()));
        let local = Arc::new(FakeLocal::default());
        let backend = Arc::new(FakeBackend {
            snapshot: Mutex::new(LikeSnapshot {
                revision: 7,
                tracks: vec![wire(&track("aaaaaaaaaaa"))],
            }),
            ..Default::default()
        });
        let s = session(shared.clone(), local.clone(), backend);
        assert!(s.shared.note(&track("abcdefghijk"), true));
        s.pull().await.unwrap();
        assert_eq!(shared.revision(), 7);
        assert_eq!(
            local
                .list()
                .iter()
                .map(|t| t.provider_track_id.clone())
                .collect::<Vec<_>>(),
            vec!["abcdefghijk", "aaaaaaaaaaa"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn the_first_sync_uploads_in_chunks_then_adopts_the_snapshot() {
        let dir = root();
        let shared = Arc::new(Shared::new(dir.clone()));
        let local = Arc::new(FakeLocal::default());
        let tracks = (0..1201)
            .map(|i| {
                let mut t = track(&format!("{i:011}"));
                t.canonical_url =
                    format!("https://www.youtube.com/watch?v={}", t.provider_track_id);
                t
            })
            .collect();
        local.replace(tracks);
        let backend = Arc::new(FakeBackend {
            snapshot: Mutex::new(LikeSnapshot {
                revision: 3,
                tracks: vec![wire(&track("aaaaaaaaaaa"))],
            }),
            ..Default::default()
        });
        let s = session(shared.clone(), local.clone(), backend.clone());
        s.first_sync().await.unwrap();
        let merged = backend.merged.lock().unwrap();
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().all(|c| c.len() <= CHUNK_TRACKS));
        assert_eq!(merged.iter().map(Vec::len).sum::<usize>(), 1201);
        assert_eq!(
            local
                .list()
                .iter()
                .map(|t| t.provider_track_id.clone())
                .collect::<Vec<_>>(),
            vec!["aaaaaaaaaaa"]
        );
        assert_eq!(shared.revision(), 3);
        assert!(shared.initialized());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_queued_like_is_flushed_once_and_a_failed_send_stays_queued() {
        let dir = root();
        let shared = Arc::new(Shared::new(dir.clone()));
        let local = Arc::new(FakeLocal::default());
        shared.enter(account_at(&dir, "user"));
        shared.note(&track("abc"), true);
        let failing = Arc::new(FakeBackend {
            fail_put: true,
            ..Default::default()
        });
        let s = session(shared.clone(), local.clone(), failing);
        s.flush().await;
        assert!(
            shared.pending("YouTube:abc").is_some(),
            "kept for the next pass"
        );
        let backend = Arc::new(FakeBackend::default());
        let s = Session {
            shared: shared.clone(),
            local,
            backend: backend.clone(),
            on_change: Arc::new(|| {}),
        };
        s.flush().await;
        assert!(shared.pending("YouTube:abc").is_none());
        assert_eq!(*backend.put.lock().unwrap(), vec!["abc"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn the_first_account_claims_the_anonymous_file_once() {
        let dir = root();
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(anonymous_path(&dir), b"[]").unwrap();
        let a = account_at(&dir, "a");
        let b = account_at(&dir, "b");
        assert!(claim_anonymous(&dir, &a));
        assert!(a.library.exists());
        assert!(!claim_anonymous(&dir, &b));
        assert!(!b.library.exists());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn account_paths_are_scoped_and_the_anonymous_path_is_the_legacy_one() {
        let dir = Path::new("/tmp/root");
        assert_eq!(anonymous_path(dir), Path::new("/tmp/root/liked-songs.json"));
        let a = account_at(dir, "user-1");
        assert_eq!(
            a.library,
            Path::new("/tmp/root/likes/user-1/liked-songs.json")
        );
    }

    #[test]
    fn an_unknown_server_source_becomes_none_and_a_bad_track_is_dropped() {
        let mut wire = wire(&track("abcdefghijk"));
        wire.source = Some("SoundCloud".into());
        assert_eq!(saved(&wire).unwrap().source, None);
        wire.provider_track_id = "bad id".into();
        assert!(saved(&wire).is_none());
    }
}
