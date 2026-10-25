//! Tauri command surface for sync. Commands are thin: typed arguments in, typed DTOs out.
//! Only fixed error categories leave this layer, and the session token is never read from or returned to the frontend.

use serde::{Deserialize, Serialize};
use std::sync::{Mutex as StdMutex, PoisonError};
use tauri::{AppHandle, Manager, State};
use tokio::sync::{Mutex as TokioMutex, MutexGuard as TokioMutexGuard};

use crate::sync::api::Provider;
use crate::sync::conflict::Resolution;
use crate::sync::engine::{self, ErrorCategory, Host, SyncEngine, SyncState};
use crate::sync::local_store::LocalEntry;

/// The engine is checked out of `slot` only for a short step (`phase`). Network operations hold `net` for their
/// whole run and release the engine between requests (see `sync::engine`), so local commands never wait on the
/// network. `net` serialises network operations; status reads answer from the last snapshot while one is in flight.
pub struct SyncEngineState {
    slot: StdMutex<Result<Option<SyncEngine>, ErrorCategory>>,
    phase: TokioMutex<()>,
    pub(crate) net: TokioMutex<()>,
    last_status: StdMutex<Option<SyncStatusDto>>,
}

impl SyncEngineState {
    /// `Err` holds the startup category (e.g. a corrupt snapshot); every command then fails with it.
    pub fn new(engine: Result<SyncEngine, ErrorCategory>) -> Self {
        Self {
            slot: StdMutex::new(engine.map(Some)),
            phase: TokioMutex::new(()),
            net: TokioMutex::new(()),
            last_status: StdMutex::new(None),
        }
    }

    pub(crate) async fn checkout(&self) -> Result<Checkout<'_>, ErrorCategory> {
        let phase = self.phase.lock().await;
        self.take(phase)
    }

    fn take<'a>(&'a self, phase: TokioMutexGuard<'a, ()>) -> Result<Checkout<'a>, ErrorCategory> {
        let mut slot = self.slot.lock().unwrap_or_else(PoisonError::into_inner);
        let engine = match &mut *slot {
            Ok(engine) => engine.take().ok_or(ErrorCategory::Internal)?,
            Err(kind) => return Err(*kind),
        };
        Ok(Checkout {
            state: self,
            _phase: phase,
            engine: Some(engine),
        })
    }

    /// Last status computed while the engine was free; `state` is Syncing because an operation holds it.
    fn busy_status(&self) -> SyncStatusDto {
        let last = self
            .last_status
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        SyncStatusDto {
            state: SyncState::Syncing,
            pending_writes: last.as_ref().map_or(0, |s| s.pending_writes),
            conflicts: last.as_ref().map_or_else(Vec::new, |s| s.conflicts.clone()),
        }
    }

    fn remember(&self, status: &SyncStatusDto) {
        *self
            .last_status
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(status.clone());
    }
}

/// Owns the engine for one step. Dropping it returns the engine to the slot, even when the step is cancelled.
pub(crate) struct Checkout<'a> {
    state: &'a SyncEngineState,
    _phase: TokioMutexGuard<'a, ()>,
    engine: Option<SyncEngine>,
}

impl Checkout<'_> {
    pub(crate) fn engine(&mut self) -> &mut SyncEngine {
        self.engine.as_mut().expect("engine is present until drop")
    }
}

impl Drop for Checkout<'_> {
    fn drop(&mut self) {
        let Some(engine) = self.engine.take() else {
            return;
        };
        let mut slot = self
            .state
            .slot
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Ok(slot) = &mut *slot {
            *slot = Some(engine);
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignedInDto {
    pub user_id: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SignOutDto {
    pub warning: Option<ErrorCategory>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SyncStatusDto {
    pub state: SyncState,
    pub pending_writes: usize,
    pub conflicts: Vec<String>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalEntryDto {
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
pub struct LocalPlaylistDto {
    pub id: String,
    pub name: String,
    pub remote_id: Option<String>,
    pub entry_count: usize,
    /// Entries the server has accepted; below entry_count means a partial upload that can resume.
    pub uploaded: usize,
    /// Queued writes for this playlist alone; they are not on the server yet.
    pub queued_writes: usize,
    /// Entries past the server-held and queued ones: the unsent partial-upload tail.
    pub unsent_entries: usize,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ResolutionDto {
    KeepServer,
    KeepMine,
}

// The IPC command queues antibot tokens and calls the `_locked` step directly; tests use this plain form.
#[cfg(test)]
pub async fn run_sign_in(
    state: &SyncEngineState,
    email: &str,
    password: &str,
) -> Result<SignedInDto, ErrorCategory> {
    let _net = state.net.lock().await;
    run_sign_in_locked(state, email, password).await
}

/// The caller holds `state.net`.
async fn run_register_locked(
    state: &SyncEngineState,
    email: &str,
    password: &str,
) -> Result<SignedInDto, ErrorCategory> {
    let user = engine::register(&mut Host::Shared(state), email, password).await?;
    Ok(SignedInDto {
        user_id: user.user_id,
        expires_at: user.expires_at,
    })
}

/// The caller holds `state.net`.
async fn run_sign_in_locked(
    state: &SyncEngineState,
    email: &str,
    password: &str,
) -> Result<SignedInDto, ErrorCategory> {
    let user = engine::sign_in(&mut Host::Shared(state), email, password).await?;
    Ok(SignedInDto {
        user_id: user.user_id,
        expires_at: user.expires_at,
    })
}

pub async fn run_sign_out(state: &SyncEngineState) -> Result<SignOutDto, ErrorCategory> {
    let _net = state.net.lock().await;
    let warning = engine::sign_out(&mut Host::Shared(state)).await?;
    Ok(SignOutDto { warning })
}

/// Explicit discard of all queued writes; the only way past the sign-out guard.
/// Explicit discard of all queued writes; the only way past the sign-out guard.
pub async fn run_discard_pending(state: &SyncEngineState) -> Result<usize, ErrorCategory> {
    let mut co = state.checkout().await?;
    co.engine().discard_pending()
}

pub async fn run_status(state: &SyncEngineState) -> SyncStatusDto {
    // A network operation in flight owns the gate: answer from the last snapshot instead of waiting for its requests.
    let Ok(_net) = state.net.try_lock() else {
        return state.busy_status();
    };
    let status = match engine::status(&mut Host::Shared(state)).await {
        Ok(status) => SyncStatusDto {
            state: status.state,
            pending_writes: status.pending_writes,
            conflicts: status.conflicts,
        },
        Err(kind) => SyncStatusDto {
            state: SyncState::Error { kind },
            pending_writes: 0,
            conflicts: Vec::new(),
        },
    };
    state.remember(&status);
    status
}

pub async fn run_create_local_playlist(
    state: &SyncEngineState,
    name: &str,
    description: Option<&str>,
) -> Result<String, ErrorCategory> {
    let mut co = state.checkout().await?;
    co.engine().create_local(name, description)
}

pub async fn run_add_local_entry(
    state: &SyncEngineState,
    local_id: &str,
    entry: LocalEntryDto,
) -> Result<(), ErrorCategory> {
    let entry = LocalEntry {
        provider: entry.provider,
        provider_track_id: entry.provider_track_id,
        provider_url: entry.provider_url,
        title: entry.title,
        artists: entry.artists,
        artwork_url: entry.artwork_url,
        duration_ms: entry.duration_ms,
        remote_entry_id: None,
    };
    let mut co = state.checkout().await?;
    co.engine().add_local_entry(local_id, entry)
}

/// The playlist's entries in order, as the local copy holds them.
pub async fn run_playlist_entries(
    state: &SyncEngineState,
    local_id: &str,
) -> Result<Vec<LocalEntryDto>, ErrorCategory> {
    let mut co = state.checkout().await?;
    let engine = co.engine();
    let playlist = engine
        .local_playlists()
        .iter()
        .find(|p| p.id == local_id)
        .ok_or(ErrorCategory::Validation)?;
    Ok(playlist
        .entries
        .iter()
        .map(|e| LocalEntryDto {
            provider: e.provider,
            provider_track_id: e.provider_track_id.clone(),
            provider_url: e.provider_url.clone(),
            title: e.title.clone(),
            artists: e.artists.clone(),
            artwork_url: e.artwork_url.clone(),
            duration_ms: e.duration_ms,
        })
        .collect())
}

/// Deletes a local playlist that is not on the server. A playlist imported from YouTube Music is remembered as
/// deleted so the next import does not bring it back.
pub async fn run_delete_local_playlist(
    app: &tauri::AppHandle,
    state: &SyncEngineState,
    local_id: &str,
) -> Result<(), ErrorCategory> {
    let mut co = state.checkout().await?;
    let engine = co.engine();
    let description = engine
        .local_playlists()
        .iter()
        .find(|p| p.id == local_id)
        .and_then(|p| p.description.clone());
    engine.delete_local(local_id)?;
    crate::commands::youtube_import::forget_playlist(app, description.as_deref());
    Ok(())
}

pub async fn run_rename_local_playlist(
    state: &SyncEngineState,
    local_id: &str,
    name: &str,
) -> Result<(), ErrorCategory> {
    let mut co = state.checkout().await?;
    co.engine().rename_local(local_id, name)
}

pub async fn run_move_local_entry(
    state: &SyncEngineState,
    local_id: &str,
    from: usize,
    to: usize,
) -> Result<(), ErrorCategory> {
    let mut co = state.checkout().await?;
    co.engine().move_entry(local_id, from, to)
}

pub async fn run_remove_local_entry(
    state: &SyncEngineState,
    local_id: &str,
    index: usize,
) -> Result<(), ErrorCategory> {
    let mut co = state.checkout().await?;
    co.engine().remove_local_entry(local_id, index)
}

/// Returns how many queued writes the server accepted.
/// Returns how many queued writes the server accepted.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareLinkDto {
    pub id: String,
    /// Contains the share token. Shown to the user once and never logged.
    pub url: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShareDto {
    pub id: String,
    pub created_at: String,
    pub active: bool,
}

pub async fn run_share_playlist(
    state: &SyncEngineState,
    local_id: &str,
) -> Result<ShareLinkDto, ErrorCategory> {
    let _net = state.net.lock().await;
    let link = engine::share(&mut Host::Shared(state), local_id).await?;
    Ok(ShareLinkDto {
        id: link.id,
        url: link.url,
    })
}

pub async fn run_list_shares(
    state: &SyncEngineState,
    local_id: &str,
) -> Result<Vec<ShareDto>, ErrorCategory> {
    let _net = state.net.lock().await;
    let shares = engine::list_shares(&mut Host::Shared(state), local_id).await?;
    Ok(shares
        .into_iter()
        .map(|(id, created_at, active)| ShareDto {
            id,
            created_at,
            active,
        })
        .collect())
}

pub async fn run_revoke_share(
    state: &SyncEngineState,
    local_id: &str,
    share: &str,
) -> Result<(), ErrorCategory> {
    let _net = state.net.lock().await;
    engine::revoke_share(&mut Host::Shared(state), local_id, share).await
}

pub async fn run_flush_pending(state: &SyncEngineState) -> Result<usize, ErrorCategory> {
    let _net = state.net.lock().await;
    engine::flush_pending(&mut Host::Shared(state)).await
}

pub async fn run_list_local_playlists(
    state: &SyncEngineState,
) -> Result<Vec<LocalPlaylistDto>, ErrorCategory> {
    let mut co = state.checkout().await?;
    let engine = co.engine();
    Ok(engine
        .local_playlists()
        .iter()
        .map(|p| LocalPlaylistDto {
            id: p.id.clone(),
            name: p.name.clone(),
            remote_id: p.remote_id.clone(),
            entry_count: p.entries.len(),
            uploaded: p.uploaded,
            queued_writes: p
                .remote_id
                .as_deref()
                .map_or(0, |r| engine.queued_writes(r)),
            unsent_entries: p.entries.len() - p.uploaded - p.queued,
        })
        .collect())
}

pub async fn run_upload_playlist(
    state: &SyncEngineState,
    local_id: &str,
) -> Result<String, ErrorCategory> {
    let _net = state.net.lock().await;
    engine::upload(&mut Host::Shared(state), local_id).await
}

pub async fn run_resolve_conflict(
    state: &SyncEngineState,
    playlist_id: &str,
    resolution: ResolutionDto,
) -> Result<(), ErrorCategory> {
    let resolution = match resolution {
        ResolutionDto::KeepServer => Resolution::KeepServer,
        ResolutionDto::KeepMine => Resolution::KeepMine,
    };
    let _net = state.net.lock().await;
    engine::resolve_conflict(&mut Host::Shared(state), playlist_id, resolution).await
}

/// Cloudflare Turnstile tokens are at most 2048 characters of letters, digits, `.`, `_` and `-`.
fn antibot_tokens(tokens: [Option<String>; 2]) -> Result<Vec<String>, ErrorCategory> {
    let tokens: Vec<String> = tokens.into_iter().flatten().collect();
    let ok = |t: &String| {
        !t.is_empty()
            && t.len() <= 2048
            && t.bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    };
    if tokens.iter().all(ok) {
        Ok(tokens)
    } else {
        Err(ErrorCategory::Validation)
    }
}

/// Queues the tokens, runs the sign-in step, and always drops tokens it did not use.
async fn with_antibot<T>(
    state: &SyncEngineState,
    tokens: Vec<String>,
    run: impl std::future::Future<Output = Result<T, ErrorCategory>>,
) -> Result<T, ErrorCategory> {
    let _net = state.net.lock().await;
    engine::set_antibot(&mut Host::Shared(state), tokens).await?;
    let out = run.await;
    // Best effort: a token is single use, so a leftover is useless to the next call.
    let _ = engine::set_antibot(&mut Host::Shared(state), Vec::new()).await;
    out
}

#[tauri::command]
pub async fn sync_register(
    app: AppHandle,
    state: State<'_, SyncEngineState>,
    email: String,
    password: String,
    register_token: Option<String>,
    login_token: Option<String>,
) -> Result<SignedInDto, ErrorCategory> {
    let tokens = antibot_tokens([register_token, login_token])?;
    // run_register takes the network lock itself, so queue and run under one hold of it instead.
    let out = with_antibot(
        &state,
        tokens,
        run_register_locked(&state, &email, &password),
    )
    .await?;
    kick_likes(&app);
    Ok(out)
}

#[tauri::command]
pub async fn sync_sign_in(
    app: AppHandle,
    state: State<'_, SyncEngineState>,
    email: String,
    password: String,
    token: Option<String>,
) -> Result<SignedInDto, ErrorCategory> {
    let tokens = antibot_tokens([token, None])?;
    let out = with_antibot(
        &state,
        tokens,
        run_sign_in_locked(&state, &email, &password),
    )
    .await?;
    kick_likes(&app);
    Ok(out)
}

#[tauri::command]
pub async fn sync_sign_out(
    app: AppHandle,
    state: State<'_, SyncEngineState>,
) -> Result<SignOutDto, ErrorCategory> {
    let out = run_sign_out(&state).await?;
    kick_likes(&app);
    Ok(out)
}

/// The liked-songs engine re-reads the token and switches the local library to the matching account.
fn kick_likes(app: &AppHandle) {
    if let Some(likes) = app.try_state::<crate::commands::likes::LikesState>() {
        likes.kick();
    }
}

#[tauri::command]
pub async fn sync_discard_pending(
    state: State<'_, SyncEngineState>,
) -> Result<usize, ErrorCategory> {
    run_discard_pending(&state).await
}

#[tauri::command]
pub async fn sync_status(
    state: State<'_, SyncEngineState>,
) -> Result<SyncStatusDto, ErrorCategory> {
    Ok(run_status(&state).await)
}

#[tauri::command]
pub async fn sync_create_local_playlist(
    state: State<'_, SyncEngineState>,
    name: String,
    description: Option<String>,
) -> Result<String, ErrorCategory> {
    run_create_local_playlist(&state, &name, description.as_deref()).await
}

#[tauri::command]
pub async fn sync_add_local_entry(
    state: State<'_, SyncEngineState>,
    local_id: String,
    entry: LocalEntryDto,
) -> Result<(), ErrorCategory> {
    run_add_local_entry(&state, &local_id, entry).await
}

#[tauri::command]
pub async fn sync_upload_playlist(
    state: State<'_, SyncEngineState>,
    local_id: String,
) -> Result<String, ErrorCategory> {
    run_upload_playlist(&state, &local_id).await
}

#[tauri::command]
pub async fn sync_list_local_playlists(
    state: State<'_, SyncEngineState>,
) -> Result<Vec<LocalPlaylistDto>, ErrorCategory> {
    run_list_local_playlists(&state).await
}

#[tauri::command]
pub async fn sync_playlist_entries(
    state: State<'_, SyncEngineState>,
    local_id: String,
) -> Result<Vec<LocalEntryDto>, ErrorCategory> {
    run_playlist_entries(&state, &local_id).await
}

#[tauri::command]
pub async fn sync_delete_local_playlist(
    app: tauri::AppHandle,
    state: State<'_, SyncEngineState>,
    local_id: String,
) -> Result<(), ErrorCategory> {
    run_delete_local_playlist(&app, &state, &local_id).await
}

#[tauri::command]
pub async fn sync_rename_local_playlist(
    state: State<'_, SyncEngineState>,
    local_id: String,
    name: String,
) -> Result<(), ErrorCategory> {
    run_rename_local_playlist(&state, &local_id, &name).await
}

#[tauri::command]
pub async fn sync_move_local_entry(
    state: State<'_, SyncEngineState>,
    local_id: String,
    from: usize,
    to: usize,
) -> Result<(), ErrorCategory> {
    run_move_local_entry(&state, &local_id, from, to).await
}

#[tauri::command]
pub async fn sync_remove_local_entry(
    state: State<'_, SyncEngineState>,
    local_id: String,
    index: usize,
) -> Result<(), ErrorCategory> {
    run_remove_local_entry(&state, &local_id, index).await
}

#[tauri::command]
pub async fn sync_share_playlist(
    state: State<'_, SyncEngineState>,
    local_id: String,
) -> Result<ShareLinkDto, ErrorCategory> {
    run_share_playlist(&state, &local_id).await
}

#[tauri::command]
pub async fn sync_list_shares(
    state: State<'_, SyncEngineState>,
    local_id: String,
) -> Result<Vec<ShareDto>, ErrorCategory> {
    run_list_shares(&state, &local_id).await
}

#[tauri::command]
pub async fn sync_revoke_share(
    state: State<'_, SyncEngineState>,
    local_id: String,
    share_id: String,
) -> Result<(), ErrorCategory> {
    run_revoke_share(&state, &local_id, &share_id).await
}

#[tauri::command]
pub async fn sync_flush_pending(state: State<'_, SyncEngineState>) -> Result<usize, ErrorCategory> {
    run_flush_pending(&state).await
}

#[tauri::command]
pub async fn sync_resolve_conflict(
    state: State<'_, SyncEngineState>,
    playlist_id: String,
    resolution: ResolutionDto,
) -> Result<(), ErrorCategory> {
    run_resolve_conflict(&state, &playlist_id, resolution).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sync::engine::testkit::*;
    use crate::sync::queue::WriteOp;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpListener;
    use tokio::sync::oneshot;

    /// Test seam: queues a write without going through a command, as `queue_mut` does on the engine.
    fn seed_queue(st: &SyncEngineState, playlist: &str, op: WriteOp) {
        let mut slot = st.slot.lock().unwrap_or_else(PoisonError::into_inner);
        slot.as_mut()
            .unwrap()
            .as_mut()
            .unwrap()
            .queue_mut()
            .push(playlist, op)
            .unwrap();
    }

    fn state(base: &str, tokens: &MemoryStore, path: PathBuf) -> SyncEngineState {
        SyncEngineState::new(Ok(engine(base, tokens, path)))
    }

    fn spotify_dto() -> LocalEntryDto {
        LocalEntryDto {
            provider: Provider::Spotify,
            provider_track_id: ENTRY_A.to_string(),
            provider_url: format!("https://open.spotify.com/track/{ENTRY_A}"),
            title: None,
            artists: None,
            artwork_url: None,
            duration_ms: None,
        }
    }

    fn track_dto(track: &str) -> LocalEntryDto {
        LocalEntryDto {
            provider_track_id: track.into(),
            provider_url: format!("https://open.spotify.com/track/{track}"),
            ..spotify_dto()
        }
    }

    fn json<T: Serialize>(v: &T) -> String {
        serde_json::to_string(v).unwrap()
    }

    #[tokio::test]
    async fn status_is_signed_out_before_sign_in_with_camel_case_dto() {
        let dir = TempDir::new("cmd-status");
        let st = state(&closed_base(), &MemoryStore::default(), dir.file());
        let s = json(&run_status(&st).await);
        assert_eq!(
            s,
            r#"{"state":{"state":"signedOut"},"pendingWrites":0,"conflicts":[]}"#
        );
    }

    #[tokio::test]
    async fn sign_in_then_status_is_signed_in() {
        let dir = TempDir::new("cmd-sign-in");
        let s = stub(vec![login_ok()]).await;
        let st = state(&s.base, &MemoryStore::default(), dir.file());
        let dto = run_sign_in(&st, "a@example.test", "pw").await.unwrap();
        assert_eq!(dto.user_id, None);
        assert_eq!(dto.expires_at, "2026-11-18T00:00:00Z");
        assert_eq!(run_status(&st).await.state, SyncState::Synced);
    }

    #[tokio::test]
    async fn sign_in_failure_is_a_fixed_category_without_backend_text() {
        let dir = TempDir::new("cmd-sign-in-fail");
        let echo = raw(
            "401 Unauthorized",
            &format!(r#"{{"error":{{"code":"UNAUTHENTICATED","message":"bad {PASSWORD}"}}}}"#),
        );
        let s = stub(vec![echo]).await;
        let st = state(&s.base, &MemoryStore::default(), dir.file());
        let err = run_sign_in(&st, "a@example.test", PASSWORD)
            .await
            .unwrap_err();
        assert_eq!(json(&err), r#""unauthenticated""#);
    }

    #[tokio::test]
    async fn upload_via_command_then_status_is_synced_only_after_success() {
        let dir = TempDir::new("cmd-upload");
        let s = stub(vec![created(), added(ENTRY_A, 2)]).await;
        let st = state(&s.base, &MemoryStore::with(TOKEN), dir.file());
        let local = run_create_local_playlist(&st, "Mix", None).await.unwrap();
        run_add_local_entry(&st, &local, spotify_dto())
            .await
            .unwrap();
        assert_eq!(
            run_upload_playlist(&st, &local).await,
            Ok(PLAYLIST.to_string())
        );
        let s = run_status(&st).await;
        assert_eq!(s.state, SyncState::Synced);
        assert_eq!(s.pending_writes, 0);
    }

    #[tokio::test]
    async fn upload_offline_is_offline_category_and_status_is_offline() {
        let dir = TempDir::new("cmd-offline");
        let st = state(&closed_base(), &MemoryStore::with(TOKEN), dir.file());
        let local = run_create_local_playlist(&st, "Mix", None).await.unwrap();
        let err = run_upload_playlist(&st, &local).await.unwrap_err();
        assert_eq!(json(&err), r#""offline""#);
        assert_eq!(json(&run_status(&st).await.state), r#"{"state":"offline"}"#);
    }

    #[tokio::test]
    async fn conflict_is_surfaced_and_keep_mine_resolves_it() {
        let dir = TempDir::new("cmd-keep-mine");
        let s = stub(vec![
            session_ok(),
            conflict(),
            playlist_json(9),
            added(ENTRY_A, 10),
            playlist_json(10),
        ])
        .await;
        let st = state(&s.base, &MemoryStore::with(TOKEN), dir.file());
        seed_queue(&st, PLAYLIST, add_op(ENTRY_A, 1));
        let before = run_status(&st).await;
        assert_eq!(before.conflicts, [PLAYLIST]);
        assert_eq!(
            json(&before.state),
            format!(r#"{{"state":"conflict","playlistId":"{PLAYLIST}"}}"#)
        );
        run_resolve_conflict(&st, PLAYLIST, ResolutionDto::KeepMine)
            .await
            .unwrap();
        let after = run_status(&st).await;
        assert!(after.conflicts.is_empty());
        assert_eq!(after.state, SyncState::Synced);
    }

    #[tokio::test]
    async fn conflict_is_resolved_by_keep_server() {
        let dir = TempDir::new("cmd-keep-server");
        let s = stub(vec![
            session_ok(),
            conflict(),
            playlist_json(9),
            entries_json(),
        ])
        .await;
        let st = state(&s.base, &MemoryStore::with(TOKEN), dir.file());
        seed_queue(&st, PLAYLIST, add_op(ENTRY_A, 1));
        assert_eq!(run_status(&st).await.conflicts, [PLAYLIST]);
        run_resolve_conflict(&st, PLAYLIST, ResolutionDto::KeepServer)
            .await
            .unwrap();
        let after = run_status(&st).await;
        assert!(after.conflicts.is_empty());
        assert_eq!(after.pending_writes, 0);
    }

    #[test]
    fn resolution_accepts_only_the_two_fixed_values() {
        assert!(matches!(
            serde_json::from_str::<ResolutionDto>(r#""keepServer""#),
            Ok(ResolutionDto::KeepServer)
        ));
        assert!(matches!(
            serde_json::from_str::<ResolutionDto>(r#""keepMine""#),
            Ok(ResolutionDto::KeepMine)
        ));
        assert!(serde_json::from_str::<ResolutionDto>(r#""merge""#).is_err());
    }

    #[tokio::test]
    async fn sign_out_warning_is_a_fixed_category() {
        let dir = TempDir::new("cmd-sign-out");
        let st = state(&closed_base(), &MemoryStore::with(TOKEN), dir.file());
        assert_eq!(
            json(&run_sign_out(&st).await.unwrap()),
            r#"{"warning":"offline"}"#
        );
        let s = stub(vec![raw("204 No Content", "")]).await;
        let st = state(&s.base, &MemoryStore::with(TOKEN), dir.file());
        assert_eq!(
            json(&run_sign_out(&st).await.unwrap()),
            r#"{"warning":null}"#
        );
    }

    #[tokio::test]
    async fn sign_out_is_refused_until_writes_are_discarded() {
        let dir = TempDir::new("cmd-discard");
        let s = stub(vec![raw("204 No Content", "")]).await;
        let tokens = MemoryStore::with(TOKEN);
        let st = state(&s.base, &tokens, dir.file());
        seed_queue(&st, PLAYLIST, add_op(ENTRY_A, 1));
        assert_eq!(
            json(&run_sign_out(&st).await.unwrap_err()),
            r#""validation""#
        );
        assert!(s.lines().is_empty(), "no logout request");
        assert_eq!(tokens.slot(), Some(TOKEN.to_string()));
        assert_eq!(run_discard_pending(&st).await, Ok(1));
        assert_eq!(
            json(&run_sign_out(&st).await.unwrap()),
            r#"{"warning":null}"#
        );
        assert_eq!(tokens.slot(), None);
    }

    #[tokio::test]
    async fn startup_failure_makes_every_command_fail_with_storage() {
        let st = SyncEngineState::new(Err(ErrorCategory::Storage));
        assert_eq!(
            json(&run_status(&st).await.state),
            r#"{"state":"error","kind":"storage"}"#
        );
        assert_eq!(
            run_sign_in(&st, "a@example.test", PASSWORD)
                .await
                .unwrap_err(),
            ErrorCategory::Storage
        );
        assert_eq!(run_sign_out(&st).await.unwrap_err(), ErrorCategory::Storage);
        assert_eq!(run_discard_pending(&st).await, Err(ErrorCategory::Storage));
        assert_eq!(
            run_upload_playlist(&st, "x").await,
            Err(ErrorCategory::Storage)
        );
    }

    #[tokio::test]
    async fn validation_error_is_a_fixed_category() {
        let dir = TempDir::new("cmd-validation");
        let st = state(&closed_base(), &MemoryStore::default(), dir.file());
        let err = run_create_local_playlist(&st, "   ", None)
            .await
            .unwrap_err();
        assert_eq!(json(&err), r#""validation""#);
    }

    #[tokio::test]
    async fn partial_upload_resumes_from_uploaded_count_and_reports_progress() {
        let dir = TempDir::new("cmd-resume");
        let s = stub(vec![
            created(),
            added(ENTRY_A, 2),
            raw("500 Internal Server Error", INTERNAL),
            added("track-b", 3),
        ])
        .await;
        let st = state(&s.base, &MemoryStore::with(TOKEN), dir.file());
        let local = run_create_local_playlist(&st, "Mix", None).await.unwrap();
        run_add_local_entry(&st, &local, spotify_dto())
            .await
            .unwrap();
        let mut second = spotify_dto();
        second.provider_track_id = "track-b".into();
        second.provider_url = "https://open.spotify.com/track/track-b".into();
        run_add_local_entry(&st, &local, second).await.unwrap();

        assert_eq!(
            run_upload_playlist(&st, &local).await,
            Err(ErrorCategory::Internal)
        );
        let v = serde_json::to_value(run_list_local_playlists(&st).await.unwrap()).unwrap();
        assert_eq!(v[0]["uploaded"], 1);
        assert_eq!(v[0]["entryCount"], 2);
        assert_eq!(v[0]["remoteId"], PLAYLIST);

        assert_eq!(
            run_upload_playlist(&st, &local).await,
            Ok(PLAYLIST.to_string())
        );
        let v = serde_json::to_value(run_list_local_playlists(&st).await.unwrap()).unwrap();
        assert_eq!(v[0]["uploaded"], 2);
        // Create once, then only the unsent tail (track-b) is sent on resume.
        assert_eq!(s.lines().len(), 4);
        assert_eq!(
            s.lines()[3],
            format!("POST /api/v1/playlists/{PLAYLIST}/entries HTTP/1.1")
        );
    }

    #[tokio::test]
    async fn sentinel_token_and_password_never_reach_any_dto() {
        let dir = TempDir::new("cmd-sentinels");
        let ok = stub(vec![login_ok()]).await;
        let tokens = MemoryStore::default();
        let st = state(&ok.base, &tokens, dir.file());
        let mut out = json(&run_sign_in(&st, "a@example.test", PASSWORD).await.unwrap());
        out += &json(&run_status(&st).await);
        out += &json(
            &run_sign_out(&SyncEngineState::new(Ok(engine(
                &closed_base(),
                &tokens,
                dir.file(),
            ))))
            .await
            .unwrap(),
        );
        out += &json(&run_upload_playlist(&st, "missing").await.unwrap_err());
        assert!(!out.contains(TOKEN), "token leaked: {out}");
        assert!(!out.contains(PASSWORD), "password leaked: {out}");
        assert_eq!(tokens.slot(), None, "sign-out cleared the store");
    }

    #[tokio::test]
    async fn list_reports_queued_writes_and_unsent_tail_per_playlist() {
        use crate::sync::local_store::{LocalPlaylist, LocalStore};
        use crate::sync::persist;
        use crate::sync::queue::{OfflineQueue, WriteOp};
        use std::collections::{HashMap, HashSet, VecDeque};

        let dir = TempDir::new("m3-per-playlist");
        const OTHER: &str = "6f1d2c3a-0000-4000-8000-000000000005";
        let remote_a = |track: &str| LocalEntry {
            remote_entry_id: Some(ENTRY_A.into()),
            ..spotify(track)
        };
        let store = LocalStore::from_playlists(vec![
            LocalPlaylist {
                id: "local-1".into(),
                name: "Mix".into(),
                description: None,
                // One unsent tail entry, two queued reorders.
                entries: vec![remote_a(ENTRY_A), spotify("track-b")],
                remote_id: Some(PLAYLIST.into()),
                uploaded: 1,
                queued: 0,
                version: 5,
            },
            LocalPlaylist {
                id: "local-2".into(),
                name: "Other".into(),
                description: None,
                entries: vec![remote_a(ENTRY_A), spotify("track-z")],
                remote_id: Some(OTHER.into()),
                uploaded: 1,
                queued: 1,
                version: 5,
            },
        ]);
        let reorder = || WriteOp::Reorder {
            entry_ids: vec![ENTRY_A.into()],
            expected_version: 5,
        };
        let queue = OfflineQueue::from_parts(
            HashMap::from([
                (PLAYLIST.to_string(), VecDeque::from([reorder(), reorder()])),
                (OTHER.to_string(), VecDeque::from([add_op("track-z", 5)])),
            ]),
            HashSet::new(),
        );
        persist::save(&dir.file(), &persist::capture(&store, &queue)).unwrap();

        let st = state(&closed_base(), &MemoryStore::with(TOKEN), dir.file());
        let v = serde_json::to_value(run_list_local_playlists(&st).await.unwrap()).unwrap();
        assert_eq!(v[0]["queuedWrites"], 2, "playlist 1 only: two reorders");
        assert_eq!(v[0]["unsentEntries"], 1);
        assert_eq!(v[1]["queuedWrites"], 1, "playlist 2 only: its one add");
        assert_eq!(v[1]["unsentEntries"], 0);
    }

    #[tokio::test]
    async fn status_does_not_wait_while_an_operation_holds_the_engine() {
        let dir = TempDir::new("cmd-busy-status");
        let st = state(&closed_base(), &MemoryStore::default(), dir.file());
        let held = st.net.lock().await;
        let busy = tokio::time::timeout(std::time::Duration::from_secs(1), run_status(&st))
            .await
            .expect("sync_status must not wait for an operation in flight");
        assert_eq!(busy.state, SyncState::Syncing);
        drop(held);
        assert_eq!(run_status(&st).await.state, SyncState::SignedOut);
    }

    #[tokio::test]
    async fn dropped_checkout_returns_the_engine_to_the_slot() {
        let dir = TempDir::new("cmd-checkout-drop");
        let st = state(&closed_base(), &MemoryStore::default(), dir.file());
        {
            let mut co = st.checkout().await.unwrap();
            assert_eq!(co.engine().local_playlists().len(), 0);
        }
        assert!(st.slot.lock().unwrap().as_ref().unwrap().is_some());
        assert_eq!(run_status(&st).await.state, SyncState::SignedOut);
    }

    /// One scripted answer per request, in order. `Hold` waits for its response: a stalled server when its sender is never used.
    enum Reply {
        Now(String),
        Hold(oneshot::Receiver<String>),
    }

    async fn scripted_stub(replies: Vec<Reply>) -> (String, Arc<StdMutex<Vec<String>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(StdMutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            for reply in replies {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let request = read_request(&mut sock).await;
                log.lock().unwrap().push(request);
                let res = match reply {
                    Reply::Now(res) => res,
                    Reply::Hold(rx) => match rx.await {
                        Ok(res) => res,
                        Err(_) => return,
                    },
                };
                let _ = sock.write_all(res.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        (base, seen)
    }

    async fn wait_for_requests(seen: &Arc<StdMutex<Vec<String>>>, n: usize) {
        tokio::time::timeout(Duration::from_secs(2), async {
            while seen.lock().unwrap().len() < n {
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .expect("request reached the server");
    }

    /// Local commands must finish while an upload is stalled on the server (the M2 failure: they queued behind it).
    #[tokio::test]
    async fn local_commands_finish_while_an_upload_is_stalled_on_the_server() {
        let dir = TempDir::new("cmd-stalled-upload");
        let (_never_answered, stalled) = oneshot::channel::<String>();
        let (base, seen) = scripted_stub(vec![Reply::Now(created()), Reply::Hold(stalled)]).await;
        let st = Arc::new(state(&base, &MemoryStore::with(TOKEN), dir.file()));
        let local = run_create_local_playlist(&st, "Mix", None).await.unwrap();
        run_add_local_entry(&st, &local, spotify_dto())
            .await
            .unwrap();
        let uploading = tokio::spawn({
            let (st, local) = (st.clone(), local.clone());
            async move { run_upload_playlist(&st, &local).await }
        });
        wait_for_requests(&seen, 2).await;

        let quick = Duration::from_secs(1);
        let other = tokio::time::timeout(quick, run_create_local_playlist(&st, "Other", None))
            .await
            .expect("create waits behind the upload")
            .unwrap();
        tokio::time::timeout(quick, run_add_local_entry(&st, &other, spotify_dto()))
            .await
            .expect("add waits behind the upload")
            .unwrap();
        tokio::time::timeout(quick, run_remove_local_entry(&st, &other, 0))
            .await
            .expect("remove waits behind the upload")
            .unwrap();
        let lists = tokio::time::timeout(quick, run_list_local_playlists(&st))
            .await
            .expect("list waits behind the upload")
            .unwrap();
        assert_eq!(lists.len(), 2);
        assert_eq!(lists[1].entry_count, 0);
        assert_eq!(
            tokio::time::timeout(quick, run_discard_pending(&st))
                .await
                .expect("discard waits behind the upload"),
            Ok(0)
        );

        // Cancelling the stalled upload must release the network gate; the engine stays usable.
        uploading.abort();
        let _ = uploading.await;
        assert!(st.net.try_lock().is_ok(), "network gate released on cancel");
    }

    /// An edit made during an upload is kept at merge-back, persisted, and sent by the same upload.
    #[tokio::test]
    async fn local_append_made_during_an_upload_is_kept_and_sent_after_it() {
        let dir = TempDir::new("cmd-merge-back");
        let (release, held) = oneshot::channel::<String>();
        let (base, seen) = scripted_stub(vec![
            Reply::Now(created()),
            Reply::Hold(held),
            Reply::Now(added("track-b", 3)),
        ])
        .await;
        let tokens = MemoryStore::with(TOKEN);
        let st = Arc::new(state(&base, &tokens, dir.file()));
        let local = run_create_local_playlist(&st, "Mix", None).await.unwrap();
        run_add_local_entry(&st, &local, spotify_dto())
            .await
            .unwrap();
        let uploading = tokio::spawn({
            let (st, local) = (st.clone(), local.clone());
            async move { run_upload_playlist(&st, &local).await }
        });
        wait_for_requests(&seen, 2).await;

        let mut edit = spotify_dto();
        edit.provider_track_id = "track-b".into();
        edit.provider_url = "https://open.spotify.com/track/track-b".into();
        run_add_local_entry(&st, &local, edit).await.unwrap();
        // The entry at the upload position is the one in flight: removing it is refused, not lost.
        assert_eq!(
            run_remove_local_entry(&st, &local, 0).await,
            Err(ErrorCategory::Conflict)
        );

        release.send(added(ENTRY_A, 2)).unwrap();
        assert_eq!(
            uploading.await.unwrap(),
            Ok(PLAYLIST.to_string()),
            "the upload also sends the edit"
        );
        let v = serde_json::to_value(run_list_local_playlists(&st).await.unwrap()).unwrap();
        assert_eq!(v[0]["entryCount"], 2);
        assert_eq!(v[0]["uploaded"], 2);
        assert_eq!(v[0]["unsentEntries"], 0);

        // The merged state is what the snapshot holds.
        let reloaded = engine(&base, &tokens, dir.file());
        let entries = &reloaded.local_playlists()[0].entries;
        assert_eq!(entries[1].provider_track_id, "track-b");
        assert_eq!(entries[1].remote_entry_id.as_deref(), Some("e-track-b"));
    }

    /// Discard during an upload undoes the queued writes and leaves the in-flight upload's bookkeeping intact.
    #[tokio::test]
    async fn discard_during_an_upload_undoes_queued_writes_and_keeps_upload_progress() {
        use crate::sync::local_store::{LocalPlaylist, LocalStore};
        use crate::sync::persist;
        use crate::sync::queue::OfflineQueue;
        use std::collections::{HashMap, HashSet, VecDeque};

        const OTHER: &str = "6f1d2c3a-0000-4000-8000-000000000007";
        let dir = TempDir::new("cmd-discard-upload");
        // Playlist 1 is bound: one server-held entry and one queued add.
        let remote_a = LocalEntry {
            remote_entry_id: Some(ENTRY_A.into()),
            ..spotify(ENTRY_A)
        };
        let store = LocalStore::from_playlists(vec![LocalPlaylist {
            id: "local-1".into(),
            name: "Mix".into(),
            description: None,
            entries: vec![remote_a, spotify("track-q")],
            remote_id: Some(PLAYLIST.into()),
            uploaded: 1,
            queued: 1,
            version: 2,
        }]);
        let queue = OfflineQueue::from_parts(
            HashMap::from([(PLAYLIST.to_string(), VecDeque::from([add_op("track-q", 2)]))]),
            HashSet::new(),
        );
        persist::save(&dir.file(), &persist::capture(&store, &queue)).unwrap();

        let created_other = created().replace(PLAYLIST, OTHER);
        let (release, held) = oneshot::channel::<String>();
        let (base, seen) = scripted_stub(vec![
            Reply::Now(created_other),
            Reply::Hold(held),
            Reply::Now(added("track-y", 2)),
        ])
        .await;
        let st = Arc::new(state(&base, &MemoryStore::with(TOKEN), dir.file()));
        let other = run_create_local_playlist(&st, "Other", None).await.unwrap();
        run_add_local_entry(&st, &other, track_dto("track-x"))
            .await
            .unwrap();
        run_add_local_entry(&st, &other, track_dto("track-y"))
            .await
            .unwrap();
        let uploading = tokio::spawn({
            let (st, other) = (st.clone(), other.clone());
            async move { run_upload_playlist(&st, &other).await }
        });
        wait_for_requests(&seen, 2).await;

        assert_eq!(run_discard_pending(&st).await, Ok(1));
        release.send(added("track-x", 1)).unwrap();
        assert_eq!(uploading.await.unwrap(), Ok(OTHER.to_string()));

        let v = serde_json::to_value(run_list_local_playlists(&st).await.unwrap()).unwrap();
        // Playlist 1: the queued add is gone, the server-held entry stays.
        assert_eq!(v[0]["entryCount"], 1);
        assert_eq!(v[0]["queuedWrites"], 0);
        // Playlist 2: the in-flight add is recorded against its own entry; the tail is sent.
        assert_eq!(v[1]["entryCount"], 2);
        assert_eq!(v[1]["uploaded"], 2);
        assert_eq!(v[1]["unsentEntries"], 0);
        assert_eq!(run_status(&st).await.pending_writes, 0);
    }
}
