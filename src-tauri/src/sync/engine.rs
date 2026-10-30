//! Sync orchestrator: owns the API client, session token store, offline queue, local playlists and the snapshot path.
//! Only fixed categories leave this module. Backend messages and the session token never do.
//!
//! Network operations run as steps. A step holds the engine only while it reads or changes local state, and every
//! request runs with the engine released, so a local command never waits for a request in flight. Requests of
//! one playlist are ordered by the caller's network gate (one network operation at a time), and each step
//! re-checks local state before it writes a server result back.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex, PoisonError};

use serde::Serialize;

use super::api::{ApiClient, PlaylistCreateRequest, Provider, SyncError};
use super::auth::{self, AuthError, AuthState, SignedInUser};
use super::conflict::Resolution;
use super::credentials::TokenStore;
use super::local_store::{LocalEntry, LocalPlaylist, LocalStore, UploadStep};
use super::persist;
use super::queue::{OfflineQueue, QueueError};
use super::settings::{share_base, share_link};
use crate::commands::sync::{Checkout, SyncEngineState};

/// The only error strings the sync layer exposes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCategory {
    Validation,
    Unauthenticated,
    Conflict,
    RateLimited,
    Offline,
    Storage,
    QueueFull,
    Internal,
}

/// Queue rejections have one category; the queue's own message never leaves the sync layer.
impl From<QueueError> for ErrorCategory {
    fn from(_: QueueError) -> Self {
        ErrorCategory::QueueFull
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "state",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum SyncState {
    SignedOut,
    Syncing,
    Synced,
    Offline,
    RateLimited { retry_after_secs: Option<u64> },
    Conflict { playlist_id: String },
    Error { kind: ErrorCategory },
}

#[derive(Debug, PartialEq, Eq)]
pub struct SyncStatus {
    pub state: SyncState,
    pub pending_writes: usize,
    pub conflicts: Vec<String>,
}

pub struct SyncEngine {
    api: ApiClient,
    tokens: Arc<dyn TokenStore>,
    queue: OfflineQueue,
    store: LocalStore,
    path: PathBuf,
    state: SyncState,
    busy: Arc<StdMutex<Busy>>,
}

/// Operations in flight that local edits must respect. Set by network steps, read by local commands.
#[derive(Default)]
struct Busy {
    /// Sign-in or sign-out is in flight: writes for remote playlists are refused until it ends.
    account_switch: bool,
    /// Local ids whose upload has an add in flight. The add's entry sits at the upload position.
    adds: HashSet<String>,
}

enum Key {
    Account,
    Add(String),
}

/// Holds a busy mark. Dropping it clears the mark, including when the operation is cancelled mid-request.
struct Mark {
    busy: Arc<StdMutex<Busy>>,
    key: Key,
}

impl Mark {
    fn set(busy: &Arc<StdMutex<Busy>>, key: Key) -> Self {
        {
            let mut b = busy.lock().unwrap_or_else(PoisonError::into_inner);
            match &key {
                Key::Account => b.account_switch = true,
                Key::Add(id) => {
                    b.adds.insert(id.clone());
                }
            }
        }
        Self {
            busy: busy.clone(),
            key,
        }
    }
}

impl Drop for Mark {
    fn drop(&mut self) {
        let mut b = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        match &self.key {
            Key::Account => b.account_switch = false,
            Key::Add(id) => {
                b.adds.remove(id);
            }
        }
    }
}

/// Where an operation reads and writes the engine. A step holds the engine only inside its closure, never across a request.
pub(crate) enum Host<'a> {
    Shared(&'a SyncEngineState),
    Direct(&'a mut SyncEngine),
}

impl Host<'_> {
    async fn step<R>(
        &mut self,
        f: impl FnOnce(&mut SyncEngine) -> Result<R, ErrorCategory>,
    ) -> Result<R, ErrorCategory> {
        match self {
            Host::Shared(state) => {
                let mut co: Checkout<'_> = state.checkout().await?;
                f(co.engine())
            }
            Host::Direct(engine) => f(engine),
        }
    }
}

/// Maps a transport or backend error to its fixed category.
pub fn category(e: &SyncError) -> ErrorCategory {
    match e {
        SyncError::Settings(_)
        | SyncError::Validation(_)
        | SyncError::NotFound(_)
        | SyncError::Local(_) => ErrorCategory::Validation,
        SyncError::Network => ErrorCategory::Offline,
        SyncError::Unauthenticated(_) | SyncError::Forbidden(_) => ErrorCategory::Unauthenticated,
        SyncError::Conflict { .. } => ErrorCategory::Conflict,
        SyncError::RateLimited { .. } => ErrorCategory::RateLimited,
        SyncError::Internal(_) => ErrorCategory::Internal,
    }
}

impl SyncEngine {
    /// Loads the snapshot and restores playlists, queued writes and conflicts. A corrupt or unreadable file fails construction and is never written.
    pub fn new(
        api: ApiClient,
        tokens: Box<dyn TokenStore>,
        path: PathBuf,
    ) -> Result<Self, ErrorCategory> {
        let snapshot = persist::load(&path).map_err(|_| ErrorCategory::Storage)?;
        let (store, queue) = persist::restore(snapshot);
        let state = match tokens.get() {
            Ok(Some(_)) => SyncState::Syncing,
            _ => SyncState::SignedOut,
        };
        Ok(Self {
            api,
            tokens: Arc::from(tokens),
            queue,
            store,
            path,
            state,
            busy: Arc::new(StdMutex::new(Busy::default())),
        })
    }

    /// Discards every queued write and undoes it locally: adds leave the list, removes come back, versions rewind.
    /// Local only. Safe during an upload: the in-flight add sits at the upload position, which discard does not move.
    pub fn discard_pending(&mut self) -> Result<usize, ErrorCategory> {
        let dropped = self.drop_queued();
        self.commit()?;
        Ok(dropped)
    }

    /// Drops every queued write and undoes it locally: adds leave the list, removes come back, versions rewind.
    fn drop_queued(&mut self) -> usize {
        let dropped = self.queue.discard_all();
        let n = dropped.len();
        self.store.discard_queued(dropped);
        n
    }

    fn snapshot(&self) -> SyncStatus {
        let mut conflicts: Vec<String> = self.queue.conflicted().iter().cloned().collect();
        conflicts.sort();
        // Signed out wins: a conflict flag left by a rejected session cannot be resolved without a token.
        let state = match conflicts.first() {
            Some(first) if self.state != SyncState::SignedOut => SyncState::Conflict {
                playlist_id: first.clone(),
            },
            _ => self.state.clone(),
        };
        SyncStatus {
            state,
            pending_writes: self.queue.pending(),
            conflicts,
        }
    }

    pub fn create_local(
        &mut self,
        name: &str,
        description: Option<&str>,
    ) -> Result<String, ErrorCategory> {
        let id = self
            .store
            .create_local(name, description)
            .map_err(|e| category(&e))?;
        self.commit()?;
        Ok(id)
    }

    /// Refused while a sign-in or sign-out runs: a queued write would name remote ids of the account being replaced.
    fn refuse_during_account_switch(&self) -> Result<(), ErrorCategory> {
        let busy = self.busy.lock().unwrap_or_else(PoisonError::into_inner);
        if busy.account_switch {
            return Err(ErrorCategory::Validation);
        }
        Ok(())
    }

    /// Uploaded playlists send every add through the offline queue, online or not; `sync_status` or
    /// `flush_pending` sends it. Playlists not uploaded yet keep the entry local until the first upload.
    pub fn add_local_entry(
        &mut self,
        local_id: &str,
        mut entry: LocalEntry,
    ) -> Result<(), ErrorCategory> {
        // Backend requires an https providerUrl for YouTube; the UI leaves it empty when the id is known.
        if entry.provider == Provider::YouTube && entry.provider_url.trim().is_empty() {
            entry.provider_url = format!(
                "https://www.youtube.com/watch?v={}",
                entry.provider_track_id
            );
        }
        match self
            .store
            .queued_add(local_id, &entry)
            .map_err(|e| category(&e))?
        {
            Some((remote, op)) => {
                self.refuse_during_account_switch()?;
                self.queue.push(&remote, op)?;
                self.store
                    .add_queued(local_id, entry)
                    .map_err(|e| category(&e))?;
            }
            None => self
                .store
                .add_local_entry(local_id, entry)
                .map_err(|e| category(&e))?,
        }
        self.commit()
    }

    /// Same rule as `add_local_entry`: an entry the server holds is removed through the queue.
    /// The entry at the upload position is refused while its add is in flight: the server may take it.
    pub fn remove_local_entry(
        &mut self,
        local_id: &str,
        index: usize,
    ) -> Result<(), ErrorCategory> {
        let add_in_flight = self
            .busy
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .adds
            .contains(local_id);
        if add_in_flight && self.store.is_upload_position(local_id, index) {
            return Err(ErrorCategory::Conflict);
        }
        if let Some((remote, op)) = self
            .store
            .queued_remove(local_id, index)
            .map_err(|e| category(&e))?
        {
            self.refuse_during_account_switch()?;
            self.queue.push(&remote, op)?;
        }
        self.store
            .remove_entry_at(local_id, index)
            .map_err(|e| category(&e))?;
        self.commit()
    }

    pub fn rename_local(&mut self, local_id: &str, name: &str) -> Result<(), ErrorCategory> {
        self.store
            .rename_local(local_id, name)
            .map_err(|e| category(&e))?;
        self.commit()
    }

    pub fn move_entry(
        &mut self,
        local_id: &str,
        from: usize,
        to: usize,
    ) -> Result<(), ErrorCategory> {
        self.store
            .move_entry(local_id, from, to)
            .map_err(|e| category(&e))?;
        self.commit()
    }

    pub fn local_playlists(&self) -> &[LocalPlaylist] {
        self.store.playlists()
    }

    /// Queued writes for one remote playlist (the queue is keyed by remote id).
    pub fn queued_writes(&self, remote_id: &str) -> usize {
        self.queue.queued(remote_id).len()
    }

    /// Test seam: no command enqueues writes yet, so command tests seed the queue here.
    #[cfg(test)]
    pub(crate) fn queue_mut(&mut self) -> &mut OfflineQueue {
        &mut self.queue
    }

    fn token(&self) -> Result<String, ErrorCategory> {
        match self.tokens.get() {
            Ok(Some(token)) => Ok(token),
            Ok(None) => Err(ErrorCategory::Unauthenticated),
            Err(_) => Err(ErrorCategory::Storage),
        }
    }

    fn commit(&self) -> Result<(), ErrorCategory> {
        persist::save(&self.path, &persist::capture(&self.store, &self.queue))
            .map_err(|_| ErrorCategory::Storage)
    }

    /// Sets `state` from a server error and returns its category. Only server-reported failures reach here.
    fn record(&mut self, e: &SyncError, playlist_id: &str) -> ErrorCategory {
        self.state = match e {
            SyncError::Network => SyncState::Offline,
            SyncError::RateLimited { retry_after, .. } => SyncState::RateLimited {
                retry_after_secs: *retry_after,
            },
            SyncError::Conflict { .. } => SyncState::Conflict {
                playlist_id: playlist_id.to_string(),
            },
            other => SyncState::Error {
                kind: category(other),
            },
        };
        category(e)
    }
}

/// Records a request failure and persists what was done before it. Returns the category to report.
fn fail_sync(e: &mut SyncEngine, err: &SyncError, playlist_id: &str) -> ErrorCategory {
    let cat = e.record(err, playlist_id);
    // A failed save outranks the request error, as it did when the save ran after the request.
    match e.commit() {
        Err(storage) => storage,
        Ok(()) => cat,
    }
}

/// Upload failure: a VERSION_CONFLICT also flags the playlist so resolve_conflict can settle it.
fn fail_upload(e: &mut SyncEngine, err: &SyncError, remote: &str) -> ErrorCategory {
    if matches!(err, SyncError::Conflict { .. }) {
        e.queue.mark_conflict(remote);
    }
    fail_sync(e, err, remote)
}

fn auth_category(e: &AuthError) -> ErrorCategory {
    match e {
        AuthError::Sync(s) => category(s),
        AuthError::Store(_) => ErrorCategory::Storage,
    }
}

/// Queues the antibot tokens the next register/login calls send, replacing leftovers. An empty list clears them.
pub(crate) async fn set_antibot(
    h: &mut Host<'_>,
    tokens: Vec<String>,
) -> Result<(), ErrorCategory> {
    let api = h.step(|e| Ok(e.api.clone())).await?;
    api.set_antibot(tokens);
    Ok(())
}

/// Creates an account, then signs in with it. The backend answers a registration the same way whether or not the email
/// exists, so a wrong password on an existing account shows as the sign-in failure, not as "already registered".
pub(crate) async fn register(
    h: &mut Host<'_>,
    email: &str,
    password: &str,
) -> Result<SignedInUser, ErrorCategory> {
    let email = email.trim();
    // The backend's rule: 12 to 128 bytes. Checked here so a bad password never leaves the device.
    if email.is_empty() || !(12..=128).contains(&password.len()) {
        return Err(ErrorCategory::Validation);
    }
    let api = h.step(|e| Ok(e.api.clone())).await?;
    api.register(email, password)
        .await
        .map_err(|e| category(&e))?;
    sign_in(h, email, password).await
}

/// Signs in. Refused while writes are queued: they belong to the previous account and must not be replayed onto a new one.
pub(crate) async fn sign_in(
    h: &mut Host<'_>,
    email: &str,
    password: &str,
) -> Result<SignedInUser, ErrorCategory> {
    let (api, tokens, _account) = h
        .step(|e| {
            if email.trim().is_empty() || password.is_empty() || e.queue.pending() > 0 {
                return Err(ErrorCategory::Validation);
            }
            Ok((
                e.api.clone(),
                e.tokens.clone(),
                Mark::set(&e.busy, Key::Account),
            ))
        })
        .await?;
    let user = auth::sign_in(&api, tokens.as_ref(), email, password)
        .await
        .map_err(|e| auth_category(&e))?;
    h.step(|e| {
        // Remote ids belong to whichever account last bound them; a new sign-in must not inherit them.
        e.store.unbind_all_remote();
        if e.commit().is_err() {
            // The snapshot did not save: no token may stay stored while the sign-in is not Synced.
            // Only when the clear fails too can a token remain stored; the Error state reports that.
            e.state = match tokens.clear() {
                Ok(()) => SyncState::SignedOut,
                Err(_) => SyncState::Error {
                    kind: ErrorCategory::Storage,
                },
            };
            return Err(ErrorCategory::Storage);
        }
        e.state = SyncState::Synced;
        Ok(user)
    })
    .await
}

/// Refused (no request, token kept) while any write is queued. On success the queue is emptied and persisted.
/// Ok(warning): the local session is cleared either way; the warning is set when the server logout failed.
/// Remote bindings are cleared too: the playlists belong to this account and must be re-created for the next.
pub(crate) async fn sign_out(h: &mut Host<'_>) -> Result<Option<ErrorCategory>, ErrorCategory> {
    let (api, tokens, _account) = h
        .step(|e| {
            if e.queue.pending() > 0 {
                return Err(ErrorCategory::Validation);
            }
            Ok((
                e.api.clone(),
                e.tokens.clone(),
                Mark::set(&e.busy, Key::Account),
            ))
        })
        .await?;
    let out = auth::sign_out(&api, tokens.as_ref())
        .await
        .map_err(|e| auth_category(&e))?;
    h.step(|e| {
        e.state = SyncState::SignedOut;
        e.drop_queued();
        e.store.unbind_all_remote();
        e.commit()?;
        Ok(out.warning.as_ref().map(category))
    })
    .await
}

/// Re-checks the server while not Synced or Conflict, re-reads marked playlists, then replays queued writes.
/// Returns Err only when a step cannot reach local state (a startup failure); request errors go to `state`.
async fn refresh(h: &mut Host<'_>) -> Result<(), ErrorCategory> {
    let planned = h
        .step(|e| {
            let token = match e.tokens.get() {
                Ok(Some(token)) => token,
                Ok(None) => {
                    e.state = SyncState::SignedOut;
                    return Ok(None);
                }
                Err(_) => {
                    e.state = SyncState::Error {
                        kind: ErrorCategory::Storage,
                    };
                    return Ok(None);
                }
            };
            let restore = !matches!(e.state, SyncState::Synced | SyncState::Conflict { .. });
            Ok(Some((e.api.clone(), e.tokens.clone(), token, restore)))
        })
        .await?;
    let Some((api, tokens, token, restore)) = planned else {
        return Ok(());
    };

    if restore {
        match auth::restore_session(&api, tokens.as_ref()).await {
            Ok(AuthState::SignedIn(_)) => {
                h.step(|e| {
                    e.state = SyncState::Synced;
                    Ok(())
                })
                .await?;
            }
            // A rejected token is cleared. Queued writes need their remote bindings to be undone by discard_pending, so unbind only when nothing is queued.
            Ok(AuthState::SignedOut) => {
                return h
                    .step(|e| {
                        e.state = SyncState::SignedOut;
                        if e.queue.pending() == 0 {
                            e.store.unbind_all_remote();
                        }
                        if e.commit().is_err() {
                            e.state = SyncState::Error {
                                kind: ErrorCategory::Storage,
                            };
                        }
                        Ok(())
                    })
                    .await;
            }
            Err(AuthError::Sync(err)) => {
                return h
                    .step(|e| {
                        fail_sync(e, &err, "");
                        Ok(())
                    })
                    .await;
            }
            Err(AuthError::Store(_)) => {
                return h
                    .step(|e| {
                        e.state = SyncState::Error {
                            kind: ErrorCategory::Storage,
                        };
                        Ok(())
                    })
                    .await;
            }
        }
    }

    // A removal sent with an unknown outcome: re-read the server list before anything is replayed.
    let marks = h.step(|e| Ok(e.store.refresh_marks())).await?;
    let mut changed = false;
    for remote in marks {
        let skip = h
            .step(|e| Ok(!e.queue.queued(&remote).is_empty() || e.store.has_unsent_tail(&remote)))
            .await?;
        // The server copy would wipe an unsent tail: the mark stays until upload sends it.
        if skip {
            continue;
        }
        let fetched = match api.get_playlist(&token, &remote).await {
            Ok(pl) => api
                .list_entries(&token, &remote)
                .await
                .map(|es| (pl.version, es)),
            Err(err) => Err(err),
        };
        match fetched {
            Ok((version, entries)) => {
                let applied = h
                    .step(|e| {
                        // Re-checked here: a local edit made during the read makes the copy stale.
                        if !e.queue.queued(&remote).is_empty() || e.store.has_unsent_tail(&remote) {
                            return Ok(false);
                        }
                        let entries = entries.into_iter().map(LocalEntry::from_server).collect();
                        e.store.adopt_server(&remote, version, Some(entries));
                        Ok(true)
                    })
                    .await?;
                changed |= applied;
            }
            Err(err) => {
                h.step(|e| {
                    fail_sync(e, &err, &remote);
                    Ok(())
                })
                .await?;
            }
        }
    }
    if changed && h.step(|e| e.commit()).await.is_err() {
        h.step(|e| {
            e.state = SyncState::Error {
                kind: ErrorCategory::Storage,
            };
            Ok(())
        })
        .await?;
        return Ok(());
    }

    let (_, queued) = replay_all(h, &api, &token).await?;
    if queued && h.step(|e| e.commit()).await.is_err() {
        h.step(|e| {
            e.state = SyncState::Error {
                kind: ErrorCategory::Storage,
            };
            Ok(())
        })
        .await?;
    }
    Ok(())
}

/// Sync status for the UI: refreshes first, then reads the local snapshot.
pub(crate) async fn status(h: &mut Host<'_>) -> Result<SyncStatus, ErrorCategory> {
    refresh(h).await?;
    h.step(|e| Ok(e.snapshot())).await
}

/// Uploads this device's play counts, then returns the totals across all devices.
pub(crate) async fn stats_sync(
    h: &mut Host<'_>,
    device_id: &str,
    rows: &[super::api::StatsRow],
) -> Result<Vec<super::api::StatsRow>, ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    api.put_stats(&token, device_id, rows)
        .await
        .map_err(|e| category(&e))?;
    api.get_stats(&token).await.map_err(|e| category(&e))
}

/// Deletes every stats row the signed-in user has on the server.
pub(crate) async fn stats_clear_remote(h: &mut Host<'_>) -> Result<(), ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    api.delete_stats(&token).await.map_err(|e| category(&e))
}

/// Sets or clears a playlist's description text (an import marker is kept). A playlist that is only on this device changes
/// at once. An uploaded one changes on the server first, with its version, and only then here, so the two cannot drift.
/// It is refused while writes are waiting: they would move the version under this request.
pub(crate) async fn set_description(
    h: &mut Host<'_>,
    local_id: &str,
    text: Option<&str>,
) -> Result<(), ErrorCategory> {
    let text = text.map(str::trim).filter(|d| !d.is_empty());
    let target = h
        .step(|e| {
            let pl = e
                .store
                .playlists()
                .iter()
                .find(|p| p.id == local_id)
                .ok_or(ErrorCategory::Validation)?;
            let Some(remote) = pl.remote_id.clone() else {
                e.store
                    .set_description(local_id, text, false)
                    .map_err(|err| category(&err))?;
                e.commit()?;
                return Ok(None);
            };
            if e.queued_writes(&remote) > 0 || pl.queued > 0 {
                return Err(ErrorCategory::Validation);
            }
            Ok(Some((e.api.clone(), e.token()?, remote, pl.version)))
        })
        .await?;
    let Some((api, token, remote, version)) = target else {
        return Ok(());
    };
    // Checked here too, so a bad text never reaches the server.
    if text.is_some_and(|t| t.chars().count() > 2000 || super::local_store::marker_id(t).is_some())
    {
        return Err(ErrorCategory::Validation);
    }
    let updated = api
        .update_playlist(
            &token,
            &remote,
            &super::api::PlaylistUpdateRequest {
                expected_version: version,
                name: None,
                description: Some(text.map(str::to_string)),
            },
        )
        .await
        .map_err(|e| category(&e))?;
    h.step(|e| {
        e.store.adopt_server(&remote, updated.version, None);
        e.store
            .set_description(local_id, text, true)
            .map_err(|err| category(&err))?;
        e.commit()
    })
    .await
}

/// Deletes a playlist. A playlist that is only on this device goes at once. An uploaded one is deleted on the server
/// first, with its version, and only then here, so a failed request leaves both copies. A playlist the server no
/// longer has counts as deleted. Refused while writes are waiting: they would move the version under this request.
pub(crate) async fn delete_playlist(h: &mut Host<'_>, local_id: &str) -> Result<(), ErrorCategory> {
    let target = h
        .step(|e| {
            let pl = e
                .store
                .playlists()
                .iter()
                .find(|p| p.id == local_id)
                .ok_or(ErrorCategory::Validation)?;
            let Some(remote) = pl.remote_id.clone() else {
                e.store
                    .delete_local(local_id)
                    .map_err(|err| category(&err))?;
                e.commit()?;
                return Ok(None);
            };
            if e.queued_writes(&remote) > 0 || pl.queued > 0 {
                return Err(ErrorCategory::Validation);
            }
            Ok(Some((e.api.clone(), e.token()?, remote, pl.version)))
        })
        .await?;
    let Some((api, token, remote, version)) = target else {
        return Ok(());
    };
    match api.delete_playlist(&token, &remote, version).await {
        Ok(()) | Err(SyncError::NotFound(_)) => {}
        Err(e) => return Err(category(&e)),
    }
    h.step(|e| {
        e.store
            .delete_local(local_id)
            .map_err(|err| category(&err))?;
        // A local edit made during the request may have queued a write; it would outlive its playlist.
        e.queue.discard(&remote);
        e.commit()
    })
    .await
}

/// Where a local playlist stands against the server. Read-only: nothing is uploaded or changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteState {
    /// Never uploaded from this device.
    NotUploaded,
    /// Uploaded once, but the server no longer has it (deleted on another device or in the account).
    MissingOnServer,
    /// The server copy changed since this device last saw it.
    ServerNewer,
    /// This device has entries or edits the server has not received yet.
    LocalChanges,
    /// The server has everything this device has.
    InSync,
}

/// Pure comparison, so the rules are testable without a server. `server` maps remote id to its version.
pub(crate) fn remote_state(
    p: &LocalPlaylist,
    queued_writes: usize,
    server: &std::collections::HashMap<String, i64>,
) -> RemoteState {
    let Some(remote) = p.remote_id.as_deref() else {
        return RemoteState::NotUploaded;
    };
    let Some(&version) = server.get(remote) else {
        return RemoteState::MissingOnServer;
    };
    if version > p.version {
        RemoteState::ServerNewer
    } else if queued_writes > 0 || p.entries.len() > p.uploaded {
        RemoteState::LocalChanges
    } else {
        RemoteState::InSync
    }
}

/// Asks the server which playlists it holds and compares them with this device's. Uploads nothing: the user decides.
pub(crate) async fn check_remote(
    h: &mut Host<'_>,
) -> Result<Vec<(String, RemoteState)>, ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    let server: std::collections::HashMap<String, i64> = api
        .list_playlists(&token)
        .await
        .map_err(|e| category(&e))?
        .into_iter()
        .map(|p| (p.id, p.version))
        .collect();
    h.step(|e| {
        Ok(e.local_playlists()
            .iter()
            .map(|p| {
                let queued = p.remote_id.as_deref().map_or(0, |r| e.queued_writes(r));
                (p.id.clone(), remote_state(p, queued, &server))
            })
            .collect())
    })
    .await
}

/// Sends the unsent tail of a playlist, one request at a time. Returns the remote id.
pub(crate) async fn upload(h: &mut Host<'_>, local_id: &str) -> Result<String, ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    push_tail(h, &api, &token, local_id).await
}

/// A share link for a playlist that has reached the backend. The URL contains the token; callers show it, never log it.
pub(crate) struct ShareLink {
    pub id: String,
    pub url: String,
}

/// The remote id of a playlist that has been uploaded; a playlist that is only local cannot be shared.
fn remote_of(e: &SyncEngine, local_id: &str) -> Result<String, ErrorCategory> {
    e.store
        .playlists()
        .iter()
        .find(|p| p.id == local_id)
        .and_then(|p| p.remote_id.clone())
        .ok_or(ErrorCategory::Validation)
}

pub(crate) async fn share(h: &mut Host<'_>, local_id: &str) -> Result<ShareLink, ErrorCategory> {
    let (api, token, remote) = h
        .step(|e| Ok((e.api.clone(), e.token()?, remote_of(e, local_id)?)))
        .await?;
    let created = api
        .create_share(&token, &remote)
        .await
        .map_err(|e| category(&e))?;
    // The link is the website's page, not the API. A bad configured base is an internal error, not a wrong link.
    let url = share_link(&share_base(), &created.token).ok_or(ErrorCategory::Internal)?;
    // Both the token and the URL containing it are capabilities; whichever surfaces in free text must mask.
    let registry = crate::core::secrets::registry();
    registry.register(&created.token);
    registry.register(&url);
    Ok(ShareLink {
        id: created.id,
        url,
    })
}

pub(crate) async fn list_shares(
    h: &mut Host<'_>,
    local_id: &str,
) -> Result<Vec<(String, String, bool, Option<String>)>, ErrorCategory> {
    let (api, token, remote) = h
        .step(|e| Ok((e.api.clone(), e.token()?, remote_of(e, local_id)?)))
        .await?;
    let shares = api
        .list_shares(&token, &remote)
        .await
        .map_err(|e| category(&e))?;
    Ok(shares
        .into_iter()
        .map(|s| {
            let active = s.revoked_at.is_none();
            // The URL, rebuilt from the token the server keeps for the owner; only for a live link.
            let url = s
                .token
                .filter(|_| active)
                .and_then(|t| share_link(&share_base(), &t));
            (s.id, s.created_at, active, url)
        })
        .collect())
}

pub(crate) async fn revoke_share(
    h: &mut Host<'_>,
    local_id: &str,
    share: &str,
) -> Result<(), ErrorCategory> {
    let (api, token, remote) = h
        .step(|e| Ok((e.api.clone(), e.token()?, remote_of(e, local_id)?)))
        .await?;
    api.revoke_share(&token, &remote, share)
        .await
        .map_err(|e| category(&e))
}

/// Each request is planned under the engine, sent with it released, and its result recorded under the engine again.
/// Only the upload position can be affected by local edits while a request is in flight (see `remove_local_entry`).
async fn push_tail(
    h: &mut Host<'_>,
    api: &ApiClient,
    token: &str,
    local_id: &str,
) -> Result<String, ErrorCategory> {
    h.step(|e| {
        e.state = SyncState::Syncing;
        Ok(())
    })
    .await?;
    loop {
        let (step, _add) = h
            .step(|e| {
                let step = match e.store.upload_step(local_id) {
                    Ok(step) => step,
                    Err(err) => return Err(fail_sync(e, &err, "")),
                };
                let add = match &step {
                    UploadStep::Add { .. } => Some(Mark::set(&e.busy, Key::Add(local_id.into()))),
                    _ => None,
                };
                Ok((step, add))
            })
            .await?;
        match step {
            UploadStep::Done { remote } => {
                h.step(|e| {
                    e.state = SyncState::Synced;
                    Ok(())
                })
                .await?;
                return Ok(remote);
            }
            UploadStep::Create { name, description } => {
                match api
                    .create_playlist(token, &PlaylistCreateRequest { name, description })
                    .await
                {
                    Ok(created) => {
                        h.step(|e| {
                            e.store
                                .upload_created(local_id, created.id, created.version)
                                .map_err(|err| category(&err))?;
                            e.commit()
                        })
                        .await?
                    }
                    Err(err) => return Err(h.step(|e| Ok(fail_upload(e, &err, ""))).await?),
                }
            }
            UploadStep::Add { remote, body } => match api.add_entry(token, &remote, &body).await {
                Ok(added) => {
                    h.step(|e| {
                        e.store
                            .upload_added(local_id, added.entry.id, added.version)
                            .map_err(|err| category(&err))?;
                        e.commit()
                    })
                    .await?
                }
                Err(err) => {
                    return Err(h.step(|e| Ok(fail_upload(e, &err, &remote))).await?);
                }
            },
        }
    }
}

/// Sends one playlist's queued writes oldest-first, each with the engine released. Errors are recorded in `state`
/// and returned as their category; accepted writes are counted into `accepted`.
async fn replay_playlist(
    h: &mut Host<'_>,
    api: &ApiClient,
    token: &str,
    playlist_id: &str,
    accepted: &mut usize,
) -> Result<(), ErrorCategory> {
    loop {
        let next = h
            .step(|e| {
                Ok(e.queue
                    .front_op(playlist_id)
                    .map(|op| (op, e.queue.ticket())))
            })
            .await?;
        let Some((op, ticket)) = next else {
            h.step(|e| {
                e.queue.complete_replay(playlist_id);
                Ok(())
            })
            .await?;
            return Ok(());
        };
        match op.send(api, token, playlist_id).await {
            Ok(created) => {
                h.step(|e| {
                    if e.queue.finish_front(playlist_id, ticket) {
                        *accepted += 1;
                        if let Some(id) = created {
                            e.store.adopt_created(playlist_id, &[id]);
                        }
                    } else {
                        // Dropped while the write was in flight: the server may hold it, so re-read it.
                        e.store.mark_refresh(playlist_id);
                    }
                    e.commit()
                })
                .await?;
            }
            Err(err) => {
                return Err(h
                    .step(|e| {
                        e.queue.fail_front(playlist_id, ticket, &err);
                        Ok(fail_sync(e, &err, playlist_id))
                    })
                    .await?);
            }
        }
    }
}

/// Replays every queued playlist that is not waiting on a conflict. Returns the accepted count and whether any
/// playlist had queued writes. A network failure stops the replay.
async fn replay_all(
    h: &mut Host<'_>,
    api: &ApiClient,
    token: &str,
) -> Result<(usize, bool), ErrorCategory> {
    let ids = h
        .step(|e| {
            let mut ids: Vec<String> = e
                .queue
                .queues()
                .iter()
                .filter(|(id, q)| !q.is_empty() && !e.queue.conflict_pending(id))
                .map(|(id, _)| id.clone())
                .collect();
            ids.sort();
            Ok(ids)
        })
        .await?;
    let mut accepted = 0;
    for id in &ids {
        // Offline: the rest would fail the same way, so stop here and keep their writes queued.
        if replay_playlist(h, api, token, id, &mut accepted).await == Err(ErrorCategory::Offline) {
            break;
        }
    }
    Ok((accepted, !ids.is_empty()))
}

/// Explicit replay (sync_flush_pending). Returns how many queued writes the server accepted.
pub(crate) async fn flush_pending(h: &mut Host<'_>) -> Result<usize, ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    let (accepted, _) = replay_all(h, &api, &token).await?;
    h.step(|e| e.commit().map(|()| accepted)).await
}

/// Whole-playlist conflict resolution. The request steps mirror `conflict::resolve`; each runs with the engine released.
pub(crate) async fn resolve_conflict(
    h: &mut Host<'_>,
    playlist_id: &str,
    resolution: Resolution,
) -> Result<(), ErrorCategory> {
    let (api, token) = h.step(|e| Ok((e.api.clone(), e.token()?))).await?;
    h.step(|e| {
        if !e.queue.conflict_pending(playlist_id) {
            return Err(ErrorCategory::Validation);
        }
        e.state = SyncState::Syncing;
        Ok(())
    })
    .await?;
    let server = match api.get_playlist(&token, playlist_id).await {
        Ok(server) => server,
        Err(err) => return Err(h.step(|e| Ok(fail_sync(e, &err, playlist_id))).await?),
    };
    match resolution {
        Resolution::KeepServer => {
            let entries = match api.list_entries(&token, playlist_id).await {
                Ok(entries) => entries,
                Err(err) => return Err(h.step(|e| Ok(fail_sync(e, &err, playlist_id))).await?),
            };
            h.step(|e| {
                // Fetched before the discard, so a failed fetch above leaves the queue and flag intact.
                e.queue.discard(playlist_id);
                let entries = entries.into_iter().map(LocalEntry::from_server).collect();
                e.store
                    .adopt_server(playlist_id, server.version, Some(entries));
                e.state = SyncState::Synced;
                e.commit()
            })
            .await
        }
        Resolution::KeepMine => {
            // Each replayed write bumps the server version once. A replay that succeeded is complete, so if the
            // re-read fails the version is the pre-replay one plus that count; a stale version would conflict.
            let replayed = h
                .step(|e| {
                    let n = e.queue.queued(playlist_id).len() as i64;
                    e.queue.rebase(playlist_id, server.version);
                    Ok(n)
                })
                .await?;
            let mut accepted = 0;
            if let Err(category) =
                replay_playlist(h, &api, &token, playlist_id, &mut accepted).await
            {
                h.step(|e| {
                    e.commit()?;
                    Ok(())
                })
                .await?;
                return Err(category);
            }
            let version = match api.get_playlist(&token, playlist_id).await {
                Ok(fresh) => fresh.version,
                Err(_) => server.version + replayed,
            };
            let local = h
                .step(|e| Ok(e.store.adopt_server(playlist_id, version, None)))
                .await?;
            let res = match local {
                // KeepMine keeps the local copy: its unsent upload tail goes out at the server's version.
                Some(local) => push_tail(h, &api, &token, &local).await.map(|_| ()),
                None => {
                    h.step(|e| {
                        e.state = SyncState::Synced;
                        Ok(())
                    })
                    .await
                }
            };
            let committed = h.step(|e| e.commit()).await;
            committed.and(res)
        }
    }
}

/// Whole-operation entry points for callers that own the engine (tests, the live backend test). Each runs its
/// steps in order with the engine held by the caller.
impl SyncEngine {
    pub async fn sign_in(
        &mut self,
        email: &str,
        password: &str,
    ) -> Result<SignedInUser, ErrorCategory> {
        sign_in(&mut Host::Direct(self), email, password).await
    }

    pub async fn sign_out(&mut self) -> Result<Option<ErrorCategory>, ErrorCategory> {
        sign_out(&mut Host::Direct(self)).await
    }

    pub async fn status(&mut self) -> SyncStatus {
        status(&mut Host::Direct(self))
            .await
            .expect("direct engine steps do not fail")
    }

    pub async fn upload(&mut self, local_id: &str) -> Result<String, ErrorCategory> {
        upload(&mut Host::Direct(self), local_id).await
    }

    pub async fn resolve_conflict(
        &mut self,
        playlist_id: &str,
        resolution: Resolution,
    ) -> Result<(), ErrorCategory> {
        resolve_conflict(&mut Host::Direct(self), playlist_id, resolution).await
    }

    pub async fn flush_pending(&mut self) -> Result<usize, ErrorCategory> {
        flush_pending(&mut Host::Direct(self)).await
    }
}

#[cfg(test)]
pub(crate) mod testkit {
    use super::*;
    use crate::sync::api::{EntryAddRequest, Provider};
    use crate::sync::credentials::TokenStoreError;
    use crate::sync::queue::WriteOp;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    pub(crate) const TOKEN: &str = "SENTINEL-ENGINE-TOKEN-5d2a";
    pub(crate) const PASSWORD: &str = "SENTINEL-ENGINE-PASSWORD-8e1c";
    pub(crate) const USER: &str = "6f1d2c3a-0000-4000-8000-0000000000aa";
    pub(crate) const PLAYLIST: &str = "6f1d2c3a-0000-4000-8000-000000000001";
    pub(crate) const ENTRY_A: &str = "6f1d2c3a-0000-4000-8000-000000000002";
    pub(crate) const CONFLICT: &str = r#"{"error":{"code":"VERSION_CONFLICT","message":"Playlist was changed.","details":{"currentVersion":9}}}"#;
    pub(crate) const INTERNAL: &str =
        r#"{"error":{"code":"INTERNAL_ERROR","message":"Internal error."}}"#;

    /// Token store the test keeps a handle to, so it can check what reached the store.
    #[derive(Clone, Default)]
    pub(crate) struct MemoryStore(Arc<Mutex<Option<String>>>);

    impl MemoryStore {
        pub(crate) fn with(token: &str) -> Self {
            Self(Arc::new(Mutex::new(Some(token.to_string()))))
        }

        pub(crate) fn slot(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl TokenStore for MemoryStore {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            Ok(self.slot())
        }

        fn set(&self, token: &str) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = Some(token.to_string());
            Ok(())
        }

        fn clear(&self) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    /// Temp directory removed on drop. Created under std::env::temp_dir (TMPDIR = scratch).
    pub(crate) struct TempDir(PathBuf);

    pub(crate) static NEXT: AtomicUsize = AtomicUsize::new(0);

    impl TempDir {
        pub(crate) fn new(tag: &str) -> Self {
            let n = NEXT.fetch_add(1, Ordering::Relaxed);
            let dir =
                std::env::temp_dir().join(format!("lunor-engine-{tag}-{}-{n}", std::process::id()));
            let _ = fs::remove_dir_all(&dir);
            fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        pub(crate) fn file(&self) -> PathBuf {
            self.0.join("sync-snapshot.json")
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    pub(crate) struct Stub {
        pub(crate) base: String,
        seen: Arc<Mutex<Vec<String>>>,
    }

    impl Stub {
        pub(crate) fn lines(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|r| r.lines().next().unwrap_or("").to_string())
                .collect()
        }

        /// Full raw requests (head and body), in arrival order.
        pub(crate) fn requests(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    /// Answers request i with responses[i] (the last response repeats) and records each raw request.
    pub(crate) async fn stub(responses: Vec<String>) -> Stub {
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
                log.lock().unwrap().push(raw);
                let res = &responses[i.min(responses.len() - 1)];
                i += 1;
                let _ = sock.write_all(res.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        Stub { base, seen }
    }

    pub(crate) async fn read_request(sock: &mut TcpStream) -> String {
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

    pub(crate) fn raw(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Base URL of a port nothing listens on: connections are refused.
    pub(crate) fn closed_base() -> String {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}")
    }

    pub(crate) fn login_ok() -> String {
        raw(
            "200 OK",
            &format!(r#"{{"token":"{TOKEN}","expiresAt":"2026-11-18T00:00:00Z"}}"#),
        )
    }

    pub(crate) fn session_ok() -> String {
        raw(
            "200 OK",
            &format!(r#"{{"userId":"{USER}","expiresAt":"2026-11-18T00:00:00Z"}}"#),
        )
    }

    pub(crate) fn created() -> String {
        raw(
            "201 Created",
            &format!(
                r#"{{"id":"{PLAYLIST}","name":"Mix","description":null,"version":1,"createdAt":"2026-10-19T00:00:00Z","updatedAt":"2026-10-19T00:00:00Z"}}"#
            ),
        )
    }

    pub(crate) fn added(track: &str, version: i64) -> String {
        raw(
            "201 Created",
            &format!(
                r#"{{"id":"e-{track}","provider":"Spotify","providerTrackId":"{track}","providerUrl":"https://open.spotify.com/track/{track}","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z","version":{version}}}"#
            ),
        )
    }

    pub(crate) fn playlist_json(version: i64) -> String {
        raw(
            "200 OK",
            &format!(
                r#"{{"id":"{PLAYLIST}","name":"Mix","description":null,"version":{version},"createdAt":"2026-10-19T00:00:00Z","updatedAt":"2026-10-19T00:00:00Z"}}"#
            ),
        )
    }

    pub(crate) fn entries_json() -> String {
        raw(
            "200 OK",
            &format!(
                r#"[{{"id":"{ENTRY_A}","provider":"Spotify","providerTrackId":"track-s","providerUrl":"https://open.spotify.com/track/track-s","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z"}}]"#
            ),
        )
    }

    pub(crate) fn conflict() -> String {
        raw("409 Conflict", CONFLICT)
    }

    pub(crate) fn spotify(track: &str) -> LocalEntry {
        LocalEntry {
            provider: Provider::Spotify,
            provider_track_id: track.to_string(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
            remote_entry_id: None,
        }
    }

    pub(crate) fn add_op(track: &str, expected_version: i64) -> WriteOp {
        WriteOp::AddEntry(EntryAddRequest {
            expected_version,
            provider: Provider::Spotify,
            provider_track_id: track.to_string(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
        })
    }

    pub(crate) fn engine(base: &str, tokens: &MemoryStore, path: PathBuf) -> SyncEngine {
        SyncEngine::new(
            ApiClient::new(base).unwrap(),
            Box::new(tokens.clone()),
            path,
        )
        .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::testkit::*;
    use super::*;
    use crate::sync::credentials::TokenStoreError;
    use crate::sync::queue::WriteOp;
    use crate::sync::settings::{SettingsError, parse_base_url};
    use std::collections::{HashMap, HashSet, VecDeque};
    use std::fs;

    #[tokio::test]
    async fn status_is_signed_out_before_sign_in() {
        let dir = TempDir::new("signed-out");
        let mut e = engine(&closed_base(), &MemoryStore::default(), dir.file());
        let s = e.status().await;
        assert_eq!(s.state, SyncState::SignedOut);
        assert_eq!(s.pending_writes, 0);
        assert!(s.conflicts.is_empty());
    }

    #[tokio::test]
    async fn sign_in_then_status_is_synced() {
        let dir = TempDir::new("sign-in");
        let s = stub(vec![login_ok()]).await;
        let tokens = MemoryStore::default();
        let mut e = engine(&s.base, &tokens, dir.file());
        let user = e.sign_in("a@example.test", PASSWORD).await.unwrap();
        assert_eq!(user.user_id, None);
        assert_eq!(user.expires_at, "2026-11-18T00:00:00Z");
        assert_eq!(tokens.slot(), Some(TOKEN.to_string()));
        assert_eq!(s.lines(), ["POST /api/v1/auth/login HTTP/1.1"]);
        assert_eq!(e.status().await.state, SyncState::Synced);
    }

    #[tokio::test]
    async fn sign_in_whose_snapshot_save_fails_stores_no_token_and_is_not_synced() {
        let dir = TempDir::new("f4-signin-save-fails");
        let s = stub(vec![login_ok()]).await;
        let tokens = MemoryStore::default();
        // The parent directory does not exist, so the snapshot save fails.
        let path = dir
            .file()
            .with_file_name("missing")
            .join("sync-snapshot.json");
        let mut e = engine(&s.base, &tokens, path);
        assert_eq!(
            e.sign_in("a@example.test", PASSWORD).await,
            Err(ErrorCategory::Storage)
        );
        assert_eq!(tokens.slot(), None);
        assert_ne!(e.state, SyncState::Synced);
    }

    /// Token store whose clear always fails; set and get work.
    struct ClearFails(MemoryStore);

    impl TokenStore for ClearFails {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            self.0.get()
        }

        fn set(&self, token: &str) -> Result<(), TokenStoreError> {
            self.0.set(token)
        }

        fn clear(&self) -> Result<(), TokenStoreError> {
            Err(TokenStoreError::Access)
        }
    }

    #[tokio::test]
    async fn sign_in_whose_save_and_token_clear_both_fail_is_storage_error_state() {
        let dir = TempDir::new("f4-signin-clear-fails");
        let s = stub(vec![login_ok()]).await;
        let tokens = MemoryStore::default();
        // The parent directory does not exist, so the snapshot save fails.
        let path = dir
            .file()
            .with_file_name("missing")
            .join("sync-snapshot.json");
        let mut e = SyncEngine::new(
            ApiClient::new(&s.base).unwrap(),
            Box::new(ClearFails(tokens.clone())),
            path,
        )
        .unwrap();
        assert_eq!(
            e.sign_in("a@example.test", PASSWORD).await,
            Err(ErrorCategory::Storage)
        );
        assert_eq!(
            e.state,
            SyncState::Error {
                kind: ErrorCategory::Storage
            }
        );
        // Documented residual: with both the save and the clear failing, the token stays stored.
        assert_eq!(tokens.slot(), Some(TOKEN.to_string()));
    }

    #[tokio::test]
    async fn status_reports_offline_on_connection_refused_and_keeps_queue() {
        let dir = TempDir::new("offline");
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&closed_base(), &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        let s = e.status().await;
        assert_eq!(s.state, SyncState::Offline);
        assert_eq!(s.pending_writes, 1);
        assert_eq!(e.queue.queued(PLAYLIST).len(), 1);
    }

    #[tokio::test]
    async fn upload_then_status_is_synced_only_after_success() {
        let dir = TempDir::new("upload-ok");
        let s = stub(vec![created(), added(ENTRY_A, 2)]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        let local = e.create_local("Mix", None).unwrap();
        e.add_local_entry(&local, spotify(ENTRY_A)).unwrap();
        assert_ne!(e.state, SyncState::Synced, "not synced before the upload");
        let remote = e.upload(&local).await.unwrap();
        assert_eq!(remote, PLAYLIST);
        assert_eq!(e.state, SyncState::Synced);
        assert_eq!(e.status().await.state, SyncState::Synced);
    }

    #[tokio::test]
    async fn youtube_entry_without_url_gets_url_from_id() {
        let dir = TempDir::new("youtube-url");
        let mut e = engine(&closed_base(), &MemoryStore::default(), dir.file());
        let local = e.create_local("Mix", None).unwrap();
        let entry = LocalEntry {
            provider: Provider::YouTube,
            provider_track_id: "SYNTH0001".to_string(),
            provider_url: String::new(),
            title: Some("Song".to_string()),
            artists: None,
            artwork_url: None,
            duration_ms: None,
            remote_entry_id: None,
        };
        e.add_local_entry(&local, entry).unwrap();
        assert_eq!(
            e.store.playlists()[0].entries[0].provider_url,
            "https://www.youtube.com/watch?v=SYNTH0001"
        );
    }

    #[tokio::test]
    async fn failed_upload_is_not_synced() {
        let dir = TempDir::new("upload-fail");
        let s = stub(vec![created(), raw("500 Internal Server Error", INTERNAL)]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        let local = e.create_local("Mix", None).unwrap();
        e.add_local_entry(&local, spotify(ENTRY_A)).unwrap();
        assert_eq!(e.upload(&local).await, Err(ErrorCategory::Internal));
        assert_eq!(
            e.state,
            SyncState::Error {
                kind: ErrorCategory::Internal
            }
        );
    }

    #[tokio::test]
    async fn conflict_is_surfaced_and_keep_server_resolves_it() {
        let dir = TempDir::new("keep-server");
        let s = stub(vec![
            session_ok(),
            conflict(),
            playlist_json(9),
            entries_json(),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        let st = e.status().await;
        assert_eq!(
            st.state,
            SyncState::Conflict {
                playlist_id: PLAYLIST.into()
            }
        );
        assert_eq!(st.conflicts, [PLAYLIST]);
        e.resolve_conflict(PLAYLIST, Resolution::KeepServer)
            .await
            .unwrap();
        let st = e.status().await;
        assert_eq!(st.state, SyncState::Synced);
        assert!(st.conflicts.is_empty());
        assert_eq!(st.pending_writes, 0);
    }

    #[tokio::test]
    async fn conflict_is_surfaced_and_keep_mine_replays_queue() {
        let dir = TempDir::new("keep-mine");
        let s = stub(vec![
            session_ok(),
            conflict(),
            playlist_json(9),
            added(ENTRY_A, 10),
            playlist_json(10),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        assert_eq!(e.status().await.conflicts, [PLAYLIST]);
        e.resolve_conflict(PLAYLIST, Resolution::KeepMine)
            .await
            .unwrap();
        let st = e.status().await;
        assert_eq!(st.state, SyncState::Synced);
        assert!(st.conflicts.is_empty());
        assert_eq!(st.pending_writes, 0);
    }

    #[tokio::test]
    async fn resolve_without_conflict_is_validation() {
        let dir = TempDir::new("no-conflict");
        let mut e = engine(&closed_base(), &MemoryStore::with(TOKEN), dir.file());
        assert_eq!(
            e.resolve_conflict(PLAYLIST, Resolution::KeepMine).await,
            Err(ErrorCategory::Validation)
        );
    }

    #[tokio::test]
    async fn sign_out_clears_token_and_warns_when_logout_fails() {
        let dir = TempDir::new("sign-out-fail");
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&closed_base(), &tokens, dir.file());
        assert_eq!(e.sign_out().await, Ok(Some(ErrorCategory::Offline)));
        assert_eq!(tokens.slot(), None);
        assert_eq!(e.status().await.state, SyncState::SignedOut);
    }

    #[tokio::test]
    async fn sign_out_without_warning_when_logout_succeeds() {
        let dir = TempDir::new("sign-out-ok");
        let s = stub(vec![raw("204 No Content", "")]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        assert_eq!(e.sign_out().await, Ok(None));
        assert_eq!(tokens.slot(), None);
    }

    #[tokio::test]
    async fn mutating_command_saves_snapshot_atomically() {
        let dir = TempDir::new("save");
        let mut e = engine(&closed_base(), &MemoryStore::with(TOKEN), dir.file());
        let local = e.create_local("Mix", Some("desc")).unwrap();
        let snap = persist::load(&dir.file()).unwrap();
        assert_eq!(snap.playlists.len(), 1);
        assert_eq!(snap.playlists[0].id, local);
        assert_eq!(snap.playlists[0].name, "Mix");
    }

    #[tokio::test]
    async fn saved_snapshot_restores_playlists_queue_and_conflicts() {
        let dir = TempDir::new("restore");
        // Partial upload with a remote id, one queued write, and that playlist conflicted.
        let seeded_store = LocalStore::from_playlists(vec![LocalPlaylist {
            id: "local-1".into(),
            name: "Mix".into(),
            description: None,
            entries: vec![spotify(ENTRY_A), spotify("track-b")],
            remote_id: Some(PLAYLIST.into()),
            uploaded: 1,
            queued: 0,
            version: 5,
        }]);
        let seeded_queue = OfflineQueue::from_parts(
            HashMap::from([(PLAYLIST.to_string(), VecDeque::from([add_op(ENTRY_A, 5)]))]),
            HashSet::from([PLAYLIST.to_string()]),
        );
        persist::save(&dir.file(), &persist::capture(&seeded_store, &seeded_queue)).unwrap();
        let saved = persist::load(&dir.file()).unwrap();

        let s = stub(vec![session_ok(), added("track-b", 6), added("track-c", 7)]).await;
        let mut e = engine(&s.base, &MemoryStore::with(TOKEN), dir.file());
        let st = e.status().await;
        assert_eq!(
            st.state,
            SyncState::Conflict {
                playlist_id: PLAYLIST.into()
            }
        );
        assert_eq!(st.conflicts, [PLAYLIST]);
        assert_eq!(st.pending_writes, 1);
        assert_eq!(persist::capture(&e.store, &e.queue), saved);

        // Mutations on the restored state succeed, and the upload resumes from uploaded = 1.
        e.create_local("New", None).unwrap();
        e.add_local_entry("local-1", spotify("track-c")).unwrap();
        assert_eq!(e.upload("local-1").await, Ok(PLAYLIST.to_string()));
        assert_eq!(s.lines().len(), 3, "session check + two resumed adds");
        let after = persist::load(&dir.file()).unwrap();
        assert_eq!(after.playlists.len(), 2);
        assert_eq!(after.playlists[0].uploaded, 3);
    }

    #[tokio::test]
    async fn corrupt_snapshot_fails_construction_with_storage_and_keeps_file() {
        let dir = TempDir::new("corrupt");
        fs::write(dir.file(), b"{not json").unwrap();
        let before = fs::read(dir.file()).unwrap();
        let res = SyncEngine::new(
            ApiClient::new(&closed_base()).unwrap(),
            Box::new(MemoryStore::default()),
            dir.file(),
        );
        assert!(matches!(res, Err(ErrorCategory::Storage)));
        assert_eq!(fs::read(dir.file()).unwrap(), before);
    }

    #[tokio::test]
    async fn sign_out_is_refused_while_writes_are_queued() {
        let dir = TempDir::new("sign-out-refused");
        let s = stub(vec![raw("204 No Content", "")]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        assert_eq!(e.sign_out().await, Err(ErrorCategory::Validation));
        assert!(s.lines().is_empty(), "no logout request");
        assert_eq!(tokens.slot(), Some(TOKEN.to_string()));
        assert_eq!(e.queue.pending(), 1);
    }

    #[tokio::test]
    async fn discard_counts_writes_then_sign_out_succeeds_and_persists_empty_queue() {
        let dir = TempDir::new("discard");
        let s = stub(vec![raw("204 No Content", "")]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        e.queue
            .push("other-playlist", add_op("track-b", 2))
            .unwrap();
        assert_eq!(e.discard_pending(), Ok(2));
        assert_eq!(e.discard_pending(), Ok(0));
        assert_eq!(e.sign_out().await, Ok(None));
        assert_eq!(tokens.slot(), None);
        assert_eq!(s.lines(), ["POST /api/v1/auth/logout HTTP/1.1"]);
        assert!(persist::load(&dir.file()).unwrap().queues.is_empty());
    }

    #[tokio::test]
    async fn new_account_never_replays_the_previous_queue() {
        let dir = TempDir::new("switch-account");
        let s = stub(vec![raw("204 No Content", ""), login_ok()]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue.push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        assert_eq!(
            e.sign_in("b@example.test", PASSWORD).await,
            Err(ErrorCategory::Validation)
        );
        assert!(s.lines().is_empty(), "refused sign-in sends nothing");
        e.discard_pending().unwrap();
        e.sign_out().await.unwrap();
        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(
            s.lines(),
            [
                "POST /api/v1/auth/logout HTTP/1.1",
                "POST /api/v1/auth/login HTTP/1.1"
            ]
        );
        assert_eq!(e.status().await.pending_writes, 0);
        assert_eq!(s.lines().len(), 2, "status replays nothing");
    }

    #[tokio::test]
    async fn new_account_recreates_playlist_after_sign_out_keeps_content() {
        let dir = TempDir::new("rebind");
        let s = stub(vec![
            created(),
            added("track-a", 2),
            added("track-b", 3),
            raw("204 No Content", ""),
            login_ok(),
            created(),
            added("track-a", 2),
            added("track-b", 3),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = engine(&s.base, &tokens, dir.file());
        let local = e.create_local("Mix", Some("notes")).unwrap();
        e.add_local_entry(&local, spotify("track-a")).unwrap();
        e.add_local_entry(&local, spotify("track-b")).unwrap();
        assert_eq!(e.upload(&local).await, Ok(PLAYLIST.to_string()));

        assert_eq!(e.sign_out().await, Ok(None));
        let kept = &e.store.playlists()[0];
        assert_eq!(kept.name, "Mix");
        assert_eq!(kept.description.as_deref(), Some("notes"));
        assert_eq!(kept.entries.len(), 2);
        assert_eq!(kept.remote_id, None);
        assert_eq!(kept.uploaded, 0);
        assert_eq!(
            persist::load(&dir.file()).unwrap().playlists[0].remote_id,
            None
        );

        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(e.upload(&local).await, Ok(PLAYLIST.to_string()));
        let lines = s.lines();
        assert_eq!(
            lines[3..],
            [
                "POST /api/v1/auth/logout HTTP/1.1".to_string(),
                "POST /api/v1/auth/login HTTP/1.1".to_string(),
                "POST /api/v1/playlists HTTP/1.1".to_string(),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ]
        );
        let tracks: Vec<String> = s.requests()[6..]
            .iter()
            .filter_map(|r| {
                let body = r.split_once("\r\n\r\n")?.1;
                let v: serde_json::Value = serde_json::from_str(body).ok()?;
                v["providerTrackId"].as_str().map(str::to_string)
            })
            .collect();
        assert_eq!(tracks, ["track-a", "track-b"]);
        assert_eq!(e.status().await.pending_writes, 0);
    }

    #[test]
    fn every_error_category_is_a_fixed_string() {
        let expected = [
            (ErrorCategory::Validation, "validation"),
            (ErrorCategory::Unauthenticated, "unauthenticated"),
            (ErrorCategory::Conflict, "conflict"),
            (ErrorCategory::RateLimited, "rateLimited"),
            (ErrorCategory::Offline, "offline"),
            (ErrorCategory::Storage, "storage"),
            (ErrorCategory::QueueFull, "queueFull"),
            (ErrorCategory::Internal, "internal"),
        ];
        for (cat, text) in expected {
            assert_eq!(serde_json::to_string(&cat).unwrap(), format!("\"{text}\""));
        }
    }

    #[test]
    fn queue_full_maps_to_queue_full_category_without_message() {
        let cat = ErrorCategory::from(QueueError::Full);
        assert_eq!(cat, ErrorCategory::QueueFull);
        // The wire form is the bare string: the queue's message text must not leak.
        assert_eq!(serde_json::to_string(&cat).unwrap(), "\"queueFull\"");
    }

    #[test]
    fn sync_errors_map_to_fixed_categories() {
        let settings = SyncError::Settings(parse_base_url("").unwrap_err());
        assert_eq!(category(&settings), ErrorCategory::Validation);
        assert_eq!(
            category(&SyncError::Settings(SettingsError::Scheme)),
            ErrorCategory::Validation
        );
        assert_eq!(category(&SyncError::Local("x")), ErrorCategory::Validation);
        assert_eq!(
            category(&SyncError::Validation(TOKEN.into())),
            ErrorCategory::Validation
        );
        assert_eq!(
            category(&SyncError::NotFound(TOKEN.into())),
            ErrorCategory::Validation
        );
        assert_eq!(category(&SyncError::Network), ErrorCategory::Offline);
        assert_eq!(
            category(&SyncError::Unauthenticated(TOKEN.into())),
            ErrorCategory::Unauthenticated
        );
        assert_eq!(
            category(&SyncError::Forbidden(TOKEN.into())),
            ErrorCategory::Unauthenticated
        );
        assert_eq!(
            category(&SyncError::Conflict {
                message: TOKEN.into(),
                current_version: Some(9)
            }),
            ErrorCategory::Conflict
        );
        assert_eq!(
            category(&SyncError::RateLimited {
                message: TOKEN.into(),
                retry_after: Some(5)
            }),
            ErrorCategory::RateLimited
        );
        assert_eq!(
            category(&SyncError::Internal(TOKEN.into())),
            ErrorCategory::Internal
        );
    }

    const LOCAL: &str = "local-1";
    const ENTRY_B: &str = "6f1d2c3a-0000-4000-8000-000000000004";
    const UNAUTHORIZED: &str =
        r#"{"error":{"code":"UNAUTHENTICATED","message":"Authentication required."}}"#;

    /// Uploaded playlist (remote id PLAYLIST, version 5) saved to `path`, then opened on `base`.
    fn bound_engine(
        base: &str,
        tokens: &MemoryStore,
        path: &std::path::Path,
        entries: Vec<LocalEntry>,
        uploaded: usize,
        queue: OfflineQueue,
    ) -> SyncEngine {
        let store = LocalStore::from_playlists(vec![LocalPlaylist {
            id: LOCAL.into(),
            name: "Mix".into(),
            description: None,
            entries,
            remote_id: Some(PLAYLIST.into()),
            uploaded,
            queued: 0,
            version: 5,
        }]);
        persist::save(path, &persist::capture(&store, &queue)).unwrap();
        engine(base, tokens, path.to_path_buf())
    }

    /// Spotify entry the server already holds under `entry_id`.
    fn remote_entry(track: &str, entry_id: &str) -> LocalEntry {
        LocalEntry {
            remote_entry_id: Some(entry_id.into()),
            ..spotify(track)
        }
    }

    fn body_json(raw: &str) -> serde_json::Value {
        serde_json::from_str(raw.split_once("\r\n\r\n").unwrap().1).unwrap()
    }

    #[tokio::test]
    async fn offline_add_to_uploaded_playlist_is_queued_at_known_version() {
        let dir = TempDir::new("b1-offline-add");
        let mut e = bound_engine(
            &closed_base(),
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        assert_eq!(e.queue.pending(), 1);
        match e.queue.queued(PLAYLIST)[0] {
            WriteOp::AddEntry(b) => {
                assert_eq!(b.expected_version, 5);
                assert_eq!(b.provider_track_id, "track-b");
            }
            other => panic!("{other:?}"),
        }
        // A queued add is not on the server yet, so it does not count as uploaded.
        assert_eq!(e.store.playlists()[0].uploaded, 1);
        assert_eq!(e.store.playlists()[0].queued, 1);
        let s = e.status().await;
        assert_eq!(s.state, SyncState::Offline);
        assert_eq!(s.pending_writes, 1);
    }

    #[tokio::test]
    async fn offline_remove_from_uploaded_playlist_is_queued_by_entry_id() {
        let dir = TempDir::new("b1-offline-remove");
        let mut e = bound_engine(
            &closed_base(),
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![
                remote_entry(ENTRY_A, ENTRY_A),
                remote_entry("track-b", ENTRY_B),
            ],
            2,
            OfflineQueue::default(),
        );
        e.remove_local_entry(LOCAL, 0).unwrap();
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 1);
        assert_eq!(pl.uploaded, 1);
        match e.queue.queued(PLAYLIST)[0] {
            WriteOp::RemoveEntry {
                entry_id,
                expected_version,
                ..
            } => {
                assert_eq!(entry_id, ENTRY_A);
                assert_eq!(*expected_version, 5);
            }
            other => panic!("{other:?}"),
        }
    }

    #[tokio::test]
    async fn queued_adds_carry_projected_versions_and_flush_sends_them_all() {
        let dir = TempDir::new("b1-flush");
        let s = stub(vec![added("track-b", 6), added("track-c", 7)]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        e.add_local_entry(LOCAL, spotify("track-c")).unwrap();
        assert_eq!(s.lines().len(), 0, "queued adds send nothing");
        assert_eq!(e.flush_pending().await, Ok(2));
        let reqs = s.requests();
        assert_eq!(reqs.len(), 2);
        assert_eq!(body_json(&reqs[0])["expectedVersion"], 5);
        assert_eq!(body_json(&reqs[1])["expectedVersion"], 6);
        assert_eq!(e.queue.pending(), 0);
        assert!(persist::load(&dir.file()).unwrap().queues.is_empty());
    }

    #[tokio::test]
    async fn offline_queue_cap_rejects_501st_edit_with_queue_full() {
        let dir = TempDir::new("b1-cap");
        let mut e = bound_engine(
            &closed_base(),
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        for i in 0..crate::sync::queue::CAPACITY {
            e.add_local_entry(LOCAL, spotify(&format!("track-{i}")))
                .unwrap();
        }
        assert_eq!(
            e.add_local_entry(LOCAL, spotify("track-over")),
            Err(ErrorCategory::QueueFull)
        );
        assert_eq!(e.queue.pending(), crate::sync::queue::CAPACITY);
        assert_eq!(
            e.store.playlists()[0].entries.len(),
            1 + crate::sync::queue::CAPACITY
        );
    }

    #[tokio::test]
    async fn keep_server_replaces_local_copy_with_server_entries() {
        let dir = TempDir::new("b3-keep-server");
        let s = stub(vec![playlist_json(9), entries_json()]).await;
        let queue = OfflineQueue::from_parts(
            HashMap::from([(PLAYLIST.to_string(), VecDeque::from([add_op("track-x", 5)]))]),
            HashSet::from([PLAYLIST.to_string()]),
        );
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![spotify("track-local"), spotify("track-b")],
            2,
            queue,
        );
        e.resolve_conflict(PLAYLIST, Resolution::KeepServer)
            .await
            .unwrap();
        let pl = &e.store.playlists()[0];
        let got: Vec<(String, Option<String>)> = pl
            .entries
            .iter()
            .map(|x| (x.provider_track_id.clone(), x.remote_entry_id.clone()))
            .collect();
        assert_eq!(got, [("track-s".to_string(), Some(ENTRY_A.to_string()))]);
        assert_eq!(pl.uploaded, 1);
        assert_eq!(pl.version, 9);
        let saved = persist::load(&dir.file()).unwrap();
        assert_eq!(saved.playlists[0].entries.len(), 1);
        assert!(saved.queues.is_empty());
        assert!(saved.conflicted.is_empty());
    }

    #[tokio::test]
    async fn upload_conflict_in_entry_loop_flags_playlist_and_keep_mine_resumes() {
        let dir = TempDir::new("m1-upload-conflict");
        let s = stub(vec![
            created(),
            conflict(),
            playlist_json(9),
            // Fresh read after the (empty) replay, then the unsent tail goes out at that version.
            playlist_json(9),
            added("track-a", 10),
        ])
        .await;
        let mut e = engine(&s.base, &MemoryStore::with(TOKEN), dir.file());
        let local = e.create_local("Mix", None).unwrap();
        e.add_local_entry(&local, spotify("track-a")).unwrap();
        assert_eq!(e.upload(&local).await, Err(ErrorCategory::Conflict));
        let st = e.status().await;
        assert_eq!(st.conflicts, [PLAYLIST]);
        assert_eq!(
            st.state,
            SyncState::Conflict {
                playlist_id: PLAYLIST.into()
            }
        );
        e.resolve_conflict(PLAYLIST, Resolution::KeepMine)
            .await
            .unwrap();
        let st = e.status().await;
        assert!(st.conflicts.is_empty());
        assert_eq!(st.state, SyncState::Synced);
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.uploaded, 1);
        assert_eq!(pl.entries[0].remote_entry_id.as_deref(), Some("e-track-a"));
        assert_eq!(
            s.lines(),
            [
                "POST /api/v1/playlists HTTP/1.1".to_string(),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ]
        );
    }

    #[tokio::test]
    async fn rejected_token_on_restore_unbinds_remote_ids_and_second_account_uploads_fresh() {
        let dir = TempDir::new("m2-fresh-upload");
        let s = stub(vec![
            created(),
            added("track-a", 2),
            added("track-b", 3),
            raw("401 Unauthorized", UNAUTHORIZED),
            login_ok(),
            created(),
            added("track-a", 2),
            added("track-b", 3),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut first = engine(&s.base, &tokens, dir.file());
        let local = first.create_local("Mix", None).unwrap();
        first.add_local_entry(&local, spotify("track-a")).unwrap();
        first.add_local_entry(&local, spotify("track-b")).unwrap();
        assert_eq!(first.upload(&local).await, Ok(PLAYLIST.to_string()));

        // Restart with the same token: the server now rejects it.
        let mut e = engine(&s.base, &tokens, dir.file());
        assert_eq!(e.status().await.state, SyncState::SignedOut);
        assert_eq!(tokens.slot(), None);
        let saved = persist::load(&dir.file()).unwrap();
        assert_eq!(saved.playlists[0].remote_id, None);
        assert_eq!(saved.playlists[0].uploaded, 0);
        assert_eq!(saved.playlists[0].entries.len(), 2);

        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(e.upload(&local).await, Ok(PLAYLIST.to_string()));
        assert_eq!(
            s.lines()[3..],
            [
                "GET /api/v1/auth/session HTTP/1.1".to_string(),
                "POST /api/v1/auth/login HTTP/1.1".to_string(),
                "POST /api/v1/playlists HTTP/1.1".to_string(),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ]
        );
    }

    #[tokio::test]
    async fn sentinel_token_and_password_never_reach_any_output() {
        let dir = TempDir::new("sentinels");
        // Backend echoes the password and token in its error message; the engine must drop them.
        let echo = raw(
            "401 Unauthorized",
            &format!(
                r#"{{"error":{{"code":"UNAUTHENTICATED","message":"bad {PASSWORD} {TOKEN}"}}}}"#
            ),
        );
        let s = stub(vec![echo]).await;
        let tokens = MemoryStore::default();
        let mut e = engine(&s.base, &tokens, dir.file());
        let err = e.sign_in("a@example.test", PASSWORD).await.unwrap_err();
        let mut out = format!("{err:?}{}", serde_json::to_string(&err).unwrap());
        out += &format!("{:?}", e.status().await);

        let ok = stub(vec![login_ok()]).await;
        let mut e2 = engine(&ok.base, &tokens, dir.file());
        let user = e2.sign_in("a@example.test", PASSWORD).await.unwrap();
        out += &format!("{user:?}");
        out += &serde_json::to_string(&e2.status().await.state).unwrap();
        e2.create_local("Mix", None).unwrap();
        out += &fs::read_to_string(dir.file()).unwrap();

        assert!(!out.contains(TOKEN), "token leaked: {out}");
        assert!(!out.contains(PASSWORD), "password leaked: {out}");
        assert!(out.contains("unauthenticated"), "{out}");
        // The token lives only in the store: the snapshot file never holds it.
        assert_eq!(tokens.slot(), Some(TOKEN.to_string()));
    }

    #[tokio::test]
    async fn signed_out_status_reports_pending_count_and_stays_signed_out() {
        let dir = TempDir::new("n2-signed-out-pending");
        let mut e = engine(&closed_base(), &MemoryStore::default(), dir.file());
        e.queue_mut().push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        e.queue_mut().push(PLAYLIST, add_op("track-b", 1)).unwrap();
        e.queue_mut().mark_conflict(PLAYLIST);
        let s = e.status().await;
        assert_eq!(s.state, SyncState::SignedOut);
        assert_eq!(s.pending_writes, 2);
        assert_eq!(s.conflicts, [PLAYLIST]);
    }

    #[tokio::test]
    async fn signed_out_discard_then_sign_in_works() {
        let dir = TempDir::new("n2-discard-sign-in");
        let s = stub(vec![login_ok()]).await;
        let tokens = MemoryStore::default();
        let mut e = engine(&s.base, &tokens, dir.file());
        e.queue_mut().push(PLAYLIST, add_op(ENTRY_A, 1)).unwrap();
        assert_eq!(
            e.sign_in("b@example.test", PASSWORD).await.unwrap_err(),
            ErrorCategory::Validation
        );
        assert!(s.lines().is_empty(), "refused sign-in sends nothing");
        assert_eq!(e.discard_pending(), Ok(1));
        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(s.lines(), ["POST /api/v1/auth/login HTTP/1.1"]);
        assert_eq!(e.status().await.state, SyncState::Synced);
    }

    #[tokio::test]
    async fn entry_added_offline_gets_server_id_on_flush_then_removes() {
        let dir = TempDir::new("n5-remote-id");
        let s = stub(vec![added("track-b", 6), raw("204 No Content", "")]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        assert_eq!(e.flush_pending().await, Ok(1));
        assert_eq!(
            e.store.playlists()[0].entries[1].remote_entry_id.as_deref(),
            Some("e-track-b")
        );
        e.remove_local_entry(LOCAL, 1).unwrap();
        assert_eq!(e.flush_pending().await, Ok(1));
        assert_eq!(
            s.lines()[1],
            format!(
                "DELETE /api/v1/playlists/{PLAYLIST}/entries/e-track-b?expectedVersion=6 HTTP/1.1"
            )
        );
    }

    #[tokio::test]
    async fn keep_mine_adds_after_replay_use_the_fresh_server_version() {
        let dir = TempDir::new("n1-fresh-version");
        let s = stub(vec![
            playlist_json(9),
            added("track-b", 10),
            playlist_json(10),
            added("track-c", 11),
        ])
        .await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        e.queue_mut().mark_conflict(PLAYLIST);
        e.resolve_conflict(PLAYLIST, Resolution::KeepMine)
            .await
            .unwrap();
        assert_eq!(
            e.store.playlists()[0].version,
            10,
            "fresh version after replay"
        );
        e.add_local_entry(LOCAL, spotify("track-c")).unwrap();
        assert_eq!(e.flush_pending().await, Ok(1));
        let reqs = s.requests();
        assert_eq!(body_json(&reqs[1])["expectedVersion"], 9);
        assert_eq!(body_json(&reqs[3])["expectedVersion"], 10);
    }

    #[tokio::test]
    async fn discard_removes_offline_adds_from_local_playlist_and_uploaded_matches_server() {
        let dir = TempDir::new("m1-discard");
        let s = stub(vec![session_ok(), entries_json()]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        assert_eq!(e.discard_pending(), Ok(1));
        let st = e.status().await;
        assert_eq!(st.pending_writes, 0);
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 1, "the never-sent add is gone locally");
        assert_eq!(pl.version, 5, "projected version rewinds to the server's");
        let server = ApiClient::new(&s.base)
            .unwrap()
            .list_entries(TOKEN, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(pl.uploaded, server.len());
        assert_eq!(
            persist::load(&dir.file()).unwrap().playlists[0]
                .entries
                .len(),
            1
        );
    }

    /// Server list holding one Spotify entry per id: the id is both the entry id and the track id.
    fn listing(ids: &[&str]) -> String {
        let items: Vec<String> = ids
            .iter()
            .map(|id| {
                format!(
                    r#"{{"id":"{id}","provider":"Spotify","providerTrackId":"{id}","providerUrl":"https://open.spotify.com/track/{id}","title":null,"artists":null,"artworkUrl":null,"durationMs":null,"position":0,"createdAt":"2026-10-19T00:00:00Z"}}"#
                )
            })
            .collect();
        raw("200 OK", &format!("[{}]", items.join(",")))
    }

    #[tokio::test]
    async fn discard_after_queued_remove_restores_entry_at_its_position() {
        let dir = TempDir::new("d1-discard-remove");
        let s = stub(vec![listing(&["entry-a", "entry-b"]), added("track-c", 6)]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![
                remote_entry("track-a", "entry-a"),
                remote_entry("track-b", "entry-b"),
            ],
            2,
            OfflineQueue::default(),
        );
        e.remove_local_entry(LOCAL, 0).unwrap();
        assert_eq!(e.discard_pending(), Ok(1));
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 2, "the removed entry is back");
        assert_eq!(pl.entries[0].provider_track_id, "track-a");
        assert_eq!(pl.entries[0].remote_entry_id.as_deref(), Some("entry-a"));
        let server = ApiClient::new(&s.base)
            .unwrap()
            .list_entries(TOKEN, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(pl.uploaded, server.len());
        assert_eq!(pl.version, 5, "projected version is back at the server's");
        assert_eq!(
            persist::load(&dir.file()).unwrap().playlists[0]
                .entries
                .len(),
            2
        );

        e.add_local_entry(LOCAL, spotify("track-c")).unwrap();
        assert_eq!(e.flush_pending().await, Ok(1));
        assert!(!e.queue.conflict_pending(PLAYLIST));
        assert_eq!(
            body_json(&s.requests()[1])["expectedVersion"],
            5,
            "the next add is sent at the server version, not into a conflict"
        );
    }

    #[tokio::test]
    async fn discard_after_mixed_add_and_removes_restores_every_entry() {
        let dir = TempDir::new("d2-discard-mix");
        let s = stub(vec![listing(&["entry-a", "entry-b", "entry-c"])]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![
                remote_entry("track-a", "entry-a"),
                remote_entry("track-b", "entry-b"),
                remote_entry("track-c", "entry-c"),
            ],
            3,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-d")).unwrap();
        e.remove_local_entry(LOCAL, 1).unwrap();
        e.remove_local_entry(LOCAL, 0).unwrap();
        assert_eq!(e.queue.pending(), 3);
        assert_eq!(e.discard_pending(), Ok(3));
        let pl = &e.store.playlists()[0];
        let tracks: Vec<&str> = pl
            .entries
            .iter()
            .map(|x| x.provider_track_id.as_str())
            .collect();
        assert_eq!(tracks, ["track-a", "track-b", "track-c"]);
        assert_eq!(pl.entries[1].remote_entry_id.as_deref(), Some("entry-b"));
        assert_eq!(pl.uploaded, 3);
        assert_eq!(pl.version, 5);
        let server = ApiClient::new(&s.base)
            .unwrap()
            .list_entries(TOKEN, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(pl.uploaded, server.len());
    }

    #[tokio::test]
    async fn remove_of_server_entry_without_remote_id_fails_loudly_and_sends_nothing() {
        let dir = TempDir::new("m5-no-remote-id");
        let s = stub(vec![raw("204 No Content", "")]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![spotify("track-a")],
            1,
            OfflineQueue::default(),
        );
        assert_eq!(
            e.remove_local_entry(LOCAL, 0),
            Err(ErrorCategory::Validation)
        );
        assert!(s.lines().is_empty(), "no request is sent");
        assert_eq!(e.queue.pending(), 0);
        assert_eq!(e.store.playlists()[0].entries.len(), 1);
    }

    #[tokio::test]
    async fn offline_add_is_pending_until_flush_then_counts_as_uploaded() {
        let dir = TempDir::new("m2-pending");
        let s = stub(vec![added("track-b", 6)]).await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        assert_eq!(e.store.playlists()[0].uploaded, 1, "not on the server yet");
        assert_eq!(e.store.playlists()[0].entries.len(), 2);
        assert_eq!(e.queue.pending(), 1);
        assert!(s.lines().is_empty());
        assert_eq!(e.flush_pending().await, Ok(1));
        assert_eq!(e.store.playlists()[0].uploaded, 2);
    }

    #[tokio::test]
    async fn failed_post_replay_get_keeps_version_so_next_add_has_no_conflict() {
        let dir = TempDir::new("m4-stale-version");
        let s = stub(vec![
            playlist_json(9),
            added("track-b", 10),
            raw("500 Internal Server Error", INTERNAL),
            added("track-c", 11),
        ])
        .await;
        let mut e = bound_engine(
            &s.base,
            &MemoryStore::with(TOKEN),
            &dir.file(),
            vec![remote_entry(ENTRY_A, ENTRY_A)],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        e.queue_mut().mark_conflict(PLAYLIST);
        // The replay succeeded, so the resolution succeeds even though the re-read failed.
        assert_eq!(
            e.resolve_conflict(PLAYLIST, Resolution::KeepMine).await,
            Ok(())
        );
        assert!(!e.queue.conflict_pending(PLAYLIST));
        assert_eq!(e.store.playlists()[0].version, 10, "9 + one replayed write");
        e.add_local_entry(LOCAL, spotify("track-c")).unwrap();
        assert_eq!(e.flush_pending().await, Ok(1));
        assert_eq!(body_json(&s.requests()[3])["expectedVersion"], 10);
        assert!(!e.queue.conflict_pending(PLAYLIST));
    }

    #[tokio::test]
    async fn rejected_token_with_queued_remove_keeps_binding_then_discard_restores_entry() {
        let dir = TempDir::new("f1-401-remove");
        let s = stub(vec![
            raw("401 Unauthorized", UNAUTHORIZED),
            listing(&["entry-a", "entry-b"]),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = bound_engine(
            &s.base,
            &tokens,
            &dir.file(),
            vec![
                remote_entry("track-a", "entry-a"),
                remote_entry("track-b", "entry-b"),
            ],
            2,
            OfflineQueue::default(),
        );
        e.remove_local_entry(LOCAL, 0).unwrap();
        let st = e.status().await;
        assert_eq!(st.state, SyncState::SignedOut);
        assert_eq!(st.pending_writes, 1);
        assert_eq!(tokens.slot(), None);
        assert_eq!(e.store.playlists()[0].remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(e.discard_pending(), Ok(1));
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 2, "the removed entry is back");
        assert_eq!(pl.entries[0].provider_track_id, "track-a");
        assert_eq!(pl.entries[0].remote_entry_id.as_deref(), Some("entry-a"));
        let server = ApiClient::new(&s.base)
            .unwrap()
            .list_entries(TOKEN, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(pl.uploaded, server.len());
    }

    #[tokio::test]
    async fn rejected_token_with_queued_add_keeps_binding_then_discard_removes_add() {
        let dir = TempDir::new("f2-401-add");
        let s = stub(vec![
            raw("401 Unauthorized", UNAUTHORIZED),
            listing(&["entry-a"]),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = bound_engine(
            &s.base,
            &tokens,
            &dir.file(),
            vec![remote_entry("track-a", "entry-a")],
            1,
            OfflineQueue::default(),
        );
        e.add_local_entry(LOCAL, spotify("track-b")).unwrap();
        let st = e.status().await;
        assert_eq!(st.state, SyncState::SignedOut);
        assert_eq!(st.pending_writes, 1);
        assert_eq!(e.store.playlists()[0].remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(e.discard_pending(), Ok(1));
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 1, "the never-sent add is gone locally");
        assert_eq!(pl.entries[0].provider_track_id, "track-a");
        let server = ApiClient::new(&s.base)
            .unwrap()
            .list_entries(TOKEN, PLAYLIST)
            .await
            .unwrap();
        assert_eq!(pl.uploaded, server.len());
    }

    #[tokio::test]
    async fn restart_with_queued_remove_then_discard_restores_entry_at_its_index() {
        let dir = TempDir::new("f3-restart-discard");
        let tokens = MemoryStore::with(TOKEN);
        {
            let mut e = bound_engine(
                &closed_base(),
                &tokens,
                &dir.file(),
                vec![
                    remote_entry("track-a", "entry-a"),
                    remote_entry("track-b", "entry-b"),
                    remote_entry("track-c", "entry-c"),
                ],
                3,
                OfflineQueue::default(),
            );
            e.remove_local_entry(LOCAL, 1).unwrap();
        }
        let mut e = engine(&closed_base(), &tokens, dir.file());
        assert_eq!(e.queue.pending(), 1);
        assert_eq!(e.discard_pending(), Ok(1));
        let pl = &e.store.playlists()[0];
        let tracks: Vec<&str> = pl
            .entries
            .iter()
            .map(|x| x.provider_track_id.as_str())
            .collect();
        assert_eq!(tracks, ["track-a", "track-b", "track-c"]);
        assert_eq!(pl.entries[1].remote_entry_id.as_deref(), Some("entry-b"));
        assert_eq!(pl.uploaded, 3);
    }

    #[tokio::test]
    async fn attempted_remove_discard_marks_playlist_and_next_status_rereads_server() {
        let dir = TempDir::new("f4-attempted-remove");
        let s = stub(vec![session_ok(), playlist_json(9), entries_json()]).await;
        let tokens = MemoryStore::with(TOKEN);
        // Local state after removing entry-a: only track-b is left, and the removal is queued as attempted.
        let mut e = bound_engine(
            &s.base,
            &tokens,
            &dir.file(),
            vec![remote_entry("track-b", "entry-b")],
            1,
            OfflineQueue::default(),
        );
        e.queue = OfflineQueue::from_parts(
            HashMap::from([(
                PLAYLIST.to_string(),
                VecDeque::from([WriteOp::RemoveEntry {
                    entry_id: ENTRY_A.into(),
                    expected_version: 5,
                    index: 0,
                    entry: remote_entry("track-a", ENTRY_A),
                    attempted: true,
                }]),
            )]),
            HashSet::new(),
        );
        assert_eq!(e.discard_pending(), Ok(1));
        let tracks: Vec<&str> = e.store.playlists()[0]
            .entries
            .iter()
            .map(|x| x.provider_track_id.as_str())
            .collect();
        assert_eq!(
            tracks,
            ["track-b"],
            "the attempted removal is not reinserted"
        );
        assert_eq!(e.store.refresh_marks(), [PLAYLIST.to_string()]);

        let st = e.status().await;
        assert_eq!(st.state, SyncState::Synced);
        assert_eq!(
            s.lines(),
            [
                "GET /api/v1/auth/session HTTP/1.1".to_string(),
                format!("GET /api/v1/playlists/{PLAYLIST} HTTP/1.1"),
                format!("GET /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ]
        );
        let pl = &e.store.playlists()[0];
        let got: Vec<(String, Option<String>)> = pl
            .entries
            .iter()
            .map(|x| (x.provider_track_id.clone(), x.remote_entry_id.clone()))
            .collect();
        assert_eq!(got, [("track-s".to_string(), Some(ENTRY_A.to_string()))]);
        assert_eq!(pl.uploaded, 1);
        assert_eq!(pl.version, 9);
        assert!(e.store.refresh_marks().is_empty());
    }

    #[tokio::test]
    async fn sign_in_after_discarding_previous_account_rebinds_playlists() {
        let dir = TempDir::new("rebind-after-discard");
        let s = stub(vec![
            login_ok(),
            created(),
            added("track-a", 2),
            raw("401 Unauthorized", UNAUTHORIZED),
            login_ok(),
            created(),
            added("track-a", 2),
        ])
        .await;
        let tokens = MemoryStore::default();
        let mut first = engine(&s.base, &tokens, dir.file());
        first.sign_in("a@example.test", PASSWORD).await.unwrap();
        let local = first.create_local("Mix", None).unwrap();
        first.add_local_entry(&local, spotify("track-a")).unwrap();
        assert_eq!(first.upload(&local).await, Ok(PLAYLIST.to_string()));
        first.add_local_entry(&local, spotify("track-b")).unwrap();
        assert_eq!(first.queue.pending(), 1);

        // Restart: the server rejects account A's token while a write is queued, so the binding is kept.
        let mut e = engine(&s.base, &tokens, dir.file());
        assert_eq!(e.status().await.state, SyncState::SignedOut);
        assert_eq!(e.local_playlists()[0].remote_id.as_deref(), Some(PLAYLIST));
        assert_eq!(e.discard_pending(), Ok(1));

        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(e.local_playlists()[0].remote_id, None);
        assert_eq!(e.upload(&local).await, Ok(PLAYLIST.to_string()));
        assert_eq!(e.status().await.state, SyncState::Synced);
        assert_eq!(
            s.lines()[4..],
            [
                "POST /api/v1/auth/login HTTP/1.1".to_string(),
                "POST /api/v1/playlists HTTP/1.1".to_string(),
                format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1"),
            ]
        );
    }

    /// Upload stopped after track-b, so track-c is an unsent tail; a removal was attempted and its mark is set.
    #[tokio::test]
    async fn refresh_mark_with_unsent_tail_keeps_tail_and_is_not_reconciled() {
        let dir = TempDir::new("r6-major3-tail");
        // Only the session read and the later upload are expected; a wrong reconcile would read the playlist.
        let s = stub(vec![session_ok(), added("track-c", 10)]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = bound_engine(
            &s.base,
            &tokens,
            &dir.file(),
            vec![remote_entry("track-b", "entry-b"), spotify("track-c")],
            1,
            OfflineQueue::default(),
        );
        e.queue = OfflineQueue::from_parts(
            HashMap::from([(
                PLAYLIST.to_string(),
                VecDeque::from([WriteOp::RemoveEntry {
                    entry_id: ENTRY_A.into(),
                    expected_version: 5,
                    index: 0,
                    entry: remote_entry("track-a", ENTRY_A),
                    attempted: true,
                }]),
            )]),
            HashSet::new(),
        );
        assert_eq!(e.discard_pending(), Ok(1));
        assert_eq!(e.store.refresh_marks(), [PLAYLIST.to_string()]);

        assert_eq!(e.status().await.state, SyncState::Synced);
        assert_eq!(s.lines(), ["GET /api/v1/auth/session HTTP/1.1".to_string()]);
        let pl = &e.store.playlists()[0];
        assert_eq!(pl.entries.len(), 2, "the unsent tail is not wiped");
        assert_eq!(pl.entries[1].provider_track_id, "track-c");
        assert_eq!(pl.uploaded, 1);
        assert_eq!(e.store.refresh_marks(), [PLAYLIST.to_string()]);

        // The tail still goes out on upload, at the local version.
        assert_eq!(e.upload(LOCAL).await, Ok(PLAYLIST.to_string()));
        assert_eq!(
            s.lines()[1],
            format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1")
        );
        assert_eq!(body_json(&s.requests()[1])["providerTrackId"], "track-c");
    }

    /// Account A's removal was attempted and marked; signing in as B unbinds, so no request may name A's playlist.
    #[tokio::test]
    async fn sign_in_as_other_account_clears_refresh_mark_and_never_reads_old_playlist() {
        let dir = TempDir::new("r6-major4-unbind");
        let s = stub(vec![login_ok(), playlist_json(9), entries_json()]).await;
        let tokens = MemoryStore::with(TOKEN);
        let mut e = bound_engine(
            &s.base,
            &tokens,
            &dir.file(),
            vec![remote_entry("track-b", "entry-b")],
            1,
            OfflineQueue::default(),
        );
        e.queue = OfflineQueue::from_parts(
            HashMap::from([(
                PLAYLIST.to_string(),
                VecDeque::from([WriteOp::RemoveEntry {
                    entry_id: ENTRY_A.into(),
                    expected_version: 5,
                    index: 0,
                    entry: remote_entry("track-a", ENTRY_A),
                    attempted: true,
                }]),
            )]),
            HashSet::new(),
        );
        assert_eq!(e.discard_pending(), Ok(1));
        assert_eq!(e.store.refresh_marks(), [PLAYLIST.to_string()]);

        e.sign_in("b@example.test", PASSWORD).await.unwrap();
        assert_eq!(e.status().await.state, SyncState::Synced);
        assert!(e.store.refresh_marks().is_empty());
        assert_eq!(s.lines(), ["POST /api/v1/auth/login HTTP/1.1".to_string()]);
    }
}

#[cfg(test)]
mod remote_state_rules {
    use super::*;
    use std::collections::HashMap;

    fn playlist(
        remote: Option<&str>,
        version: i64,
        entries: usize,
        uploaded: usize,
    ) -> LocalPlaylist {
        let entry = LocalEntry {
            provider: Provider::YouTube,
            provider_track_id: "dQw4w9WgXcQ".into(),
            provider_url: "https://www.youtube.com/watch?v=dQw4w9WgXcQ".into(),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
            remote_entry_id: None,
        };
        LocalPlaylist {
            id: "l1".into(),
            name: "n".into(),
            description: None,
            entries: vec![entry; entries],
            remote_id: remote.map(str::to_owned),
            uploaded,
            queued: 0,
            version,
        }
    }

    #[test]
    fn each_state_follows_the_rules() {
        let server = HashMap::from([("r1".to_owned(), 3_i64)]);
        assert_eq!(
            remote_state(&playlist(None, 0, 2, 0), 0, &server),
            RemoteState::NotUploaded
        );
        assert_eq!(
            remote_state(&playlist(Some("gone"), 3, 1, 1), 0, &server),
            RemoteState::MissingOnServer
        );
        assert_eq!(
            remote_state(&playlist(Some("r1"), 2, 1, 1), 0, &server),
            RemoteState::ServerNewer
        );
        assert_eq!(
            remote_state(&playlist(Some("r1"), 3, 2, 1), 0, &server),
            RemoteState::LocalChanges
        );
        assert_eq!(
            remote_state(&playlist(Some("r1"), 3, 1, 1), 1, &server),
            RemoteState::LocalChanges
        );
        assert_eq!(
            remote_state(&playlist(Some("r1"), 3, 1, 1), 0, &server),
            RemoteState::InSync
        );
    }
}
