//! Keeps the device in step with the signed-in YouTube Music account: liked songs, saved playlists, recent history.
//! Read-only toward YouTube (there is no write API behind it). A full pass runs after sign-in, on launch at most every
//! six hours, and on demand. While connected, a quick pass every two minutes picks up new likes, un-likes, new
//! playlists and playlists whose size changed.
use crate::{
    commands::{
        emit::emit_main,
        library::{HistoryState, LibraryState},
        sync::SyncEngineState,
        youtube::YouTubeState,
    },
    core::youtube_import::rate,
    core::{
        library::{LibraryError, SavedTrack},
        youtube_import::{Import, Origin, fetch},
    },
    sync::engine::ErrorCategory,
    sync::{api::Provider, local_store::LocalEntry},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tauri::{AppHandle, Manager, State, WebviewWindow};

const EVENT: &str = "core://youtube/import";
const AUTO_EVERY_SECS: u64 = 6 * 3600;
const QUICK_EVERY: Duration = Duration::from_secs(120);
const MEMORY_FILE: &str = "youtube-import.json";
static RUNNING: AtomicBool = AtomicBool::new(false);
static LOOPING: AtomicBool = AtomicBool::new(false);
static COUNTS: Mutex<Option<HashMap<String, u64>>> = Mutex::new(None);

#[derive(Serialize, Clone, Default)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub liked_added: usize,
    pub liked_removed: usize,
    pub history_added: usize,
    pub playlists_created: usize,
    pub playlists_updated: usize,
    pub playlist_tracks_added: usize,
    /// Sections that could not be read or stored: "likes", "playlists", "history" and the YouTube ones "ytLikes", "ytPlaylists", "ytHistory".
    pub failed: Vec<&'static str>,
}
impl ImportReport {
    fn changed(&self) -> bool {
        self.liked_added
            + self.liked_removed
            + self.history_added
            + self.playlists_created
            + self.playlists_updated
            > 0
            || !self.failed.is_empty()
    }
}

#[derive(Serialize, Clone)]
#[serde(rename_all = "camelCase", tag = "state")]
enum ImportEvent {
    Running,
    Done { report: ImportReport },
    Failed,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Full,
    Quick,
}

/// What the importer remembers between runs, on disk: which liked songs came from YouTube (only those are removed
/// when they are un-liked there) and which imported playlists the user deleted (never brought back).
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Memory {
    liked_ids: BTreeSet<String>,
    skipped_playlists: BTreeSet<String>,
}
fn memory_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(MEMORY_FILE))
}
fn load_memory(app: &AppHandle) -> Memory {
    memory_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .filter(|b| b.len() <= 4 * 1024 * 1024)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}
fn save_memory(app: &AppHandle, memory: &Memory) {
    if let Some(path) = memory_path(app)
        && let Ok(bytes) = serde_json::to_vec(memory)
    {
        let _ = std::fs::write(path, bytes);
    }
}

/// Likes and un-likes made here that YouTube has not heard yet. The file is the queue: a failed or offline send stays until
/// a later pass delivers it, and the pull side skips these ids so it cannot undo them in the meantime.
#[derive(Serialize, Deserialize, Default, Clone)]
#[serde(rename_all = "camelCase", default)]
struct Outbox {
    enabled: bool,
    pending: BTreeMap<String, bool>,
}
const OUTBOX_FILE: &str = "youtube-likes-out.json";
const OUTBOX_MAX: usize = 500;
/// Guards read-modify-write of the outbox file.
static OUTBOX: Mutex<()> = Mutex::new(());
static SENDING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn outbox_edit<T>(app: &AppHandle, change: impl FnOnce(&mut Outbox) -> T) -> Option<T> {
    let path = app.path().app_data_dir().ok()?.join(OUTBOX_FILE);
    let _guard = OUTBOX.lock().unwrap_or_else(|e| e.into_inner());
    let mut outbox: Outbox = std::fs::read(&path)
        .ok()
        .filter(|b| b.len() <= 256 * 1024)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let before = outbox.clone();
    let out = change(&mut outbox);
    if outbox.enabled != before.enabled || outbox.pending != before.pending {
        let _ = std::fs::create_dir_all(path.parent()?);
        std::fs::write(&path, serde_json::to_vec(&outbox).ok()?).ok()?;
    }
    Some(out)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LikePushDto {
    pub enabled: bool,
    pub pending: usize,
}
pub fn like_push_state(app: &AppHandle) -> LikePushDto {
    outbox_edit(app, |o| LikePushDto {
        enabled: o.enabled,
        pending: o.pending.len(),
    })
    .unwrap_or(LikePushDto {
        enabled: false,
        pending: 0,
    })
}
/// Turning it off also drops what was waiting: the user chose not to write to the account.
pub fn set_like_push(app: &AppHandle, enabled: bool) -> LikePushDto {
    outbox_edit(app, |o| {
        o.enabled = enabled;
        if !enabled {
            o.pending.clear();
        }
    });
    if enabled {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { flush_likes(&app).await });
    }
    like_push_state(app)
}

/// Queues a change when sending is on and the queue has room (or already holds this id). True when it was queued.
fn enqueue(outbox: &mut Outbox, video_id: &str, liked: bool) -> bool {
    if !outbox.enabled
        || (outbox.pending.len() >= OUTBOX_MAX && !outbox.pending.contains_key(video_id))
    {
        return false;
    }
    outbox.pending.insert(video_id.to_owned(), liked);
    true
}

/// Forgets a change once YouTube has it. A change made again while it was in flight stays for the next round.
fn settle(outbox: &mut Outbox, video_id: &str, liked: bool) {
    if outbox.pending.get(video_id) == Some(&liked) {
        outbox.pending.remove(video_id);
    }
}

/// Sends the changes in order and reports each delivered one. Stops at the first failure and keeps the rest.
async fn deliver(
    yt: &crate::core::yt::Yt,
    waiting: Vec<(String, bool)>,
    mut delivered: impl FnMut(&str, bool),
) {
    for (id, liked) in waiting {
        if rate(yt, &id, liked).await.is_err() {
            return; // offline or signed out: keep the rest for the next pass
        }
        delivered(&id, liked);
    }
}

/// Called after a like or un-like is saved here. Does nothing unless the user turned sending on and the id is a video id.
pub fn push_like(app: &AppHandle, video_id: &str, liked: bool) {
    if !crate::core::playback::extract::valid_id(video_id) {
        return;
    }
    let queued = outbox_edit(app, |o| enqueue(o, video_id, liked)).unwrap_or(false);
    if queued {
        let app = app.clone();
        tauri::async_runtime::spawn(async move { flush_likes(&app).await });
    }
}

/// Sends the waiting likes one at a time. A change made again while one was in flight stays queued for the next round.
async fn flush_likes(app: &AppHandle) {
    let _one_at_a_time = SENDING.lock().await;
    let youtube = app.state::<YouTubeState>();
    let Some(yt) = youtube.yt.clone().filter(|_| youtube.connected()) else {
        return;
    };
    let waiting: Vec<(String, bool)> = outbox_edit(app, |o| {
        o.enabled
            .then(|| o.pending.iter().map(|(k, v)| (k.clone(), *v)).collect())
    })
    .flatten()
    .unwrap_or_default();
    deliver(&yt, waiting, |id, liked| {
        outbox_edit(app, |o| settle(o, id, liked));
    })
    .await;
}

use crate::sync::local_store::{
    MUSIC_MARKER, YOUTUBE_MARKER, marker_id as marker_line_id, split_note,
};
fn marker(origin: Origin, id: &str) -> String {
    let prefix = if origin == Origin::Music {
        MUSIC_MARKER
    } else {
        YOUTUBE_MARKER
    };
    format!("{prefix}{id}]")
}
/// The playlist id of the import marker a description ends with, if any (the user may have added text above it).
pub fn marker_id(description: &str) -> Option<&str> {
    split_note(description).1.and_then(marker_line_id)
}
/// Called when the user deletes a local playlist: an imported one must not come back on the next pass.
pub fn forget_playlist(app: &AppHandle, description: Option<&str>) {
    if let Some(id) = description.and_then(marker_id) {
        let mut memory = load_memory(app);
        memory.skipped_playlists.insert(id.to_owned());
        save_memory(app, &memory);
    }
}

pub fn entry(t: &SavedTrack) -> LocalEntry {
    LocalEntry {
        provider: Provider::YouTube,
        provider_track_id: t.provider_track_id.clone(),
        provider_url: t.canonical_url.clone(),
        title: Some(t.title.clone()),
        artists: Some(t.artists.clone()),
        artwork_url: t.artwork_url.clone(),
        duration_ms: t.duration_ms.and_then(|d| i64::try_from(d).ok()),
        remote_entry_id: None,
    }
}

/// Creates each imported playlist once (found again by the marker in its description) and adds only the tracks the
/// local copy does not have yet, so a repeat pass never duplicates and never removes anything the user changed.
async fn apply_playlists(
    app: &AppHandle,
    import: &Import,
    skipped: &BTreeSet<String>,
    report: &mut ImportReport,
) {
    if import.playlists.is_empty() {
        return;
    }
    let state = app.state::<SyncEngineState>();
    let Ok(mut checkout) = state.checkout().await else {
        report.failed.push("playlists");
        return;
    };
    let engine = checkout.engine();
    for playlist in import.playlists.iter().filter(|p| !skipped.contains(&p.id)) {
        let note = marker(playlist.origin, &playlist.id);
        let existing = engine
            .local_playlists()
            .iter()
            .find(|p| p.description.as_deref().and_then(|d| split_note(d).1) == Some(note.as_str()))
            .map(|p| p.id.clone());
        let (local_id, created) = match existing {
            Some(id) => (id, false),
            None => match engine.create_local(&playlist.name, Some(&note)) {
                Ok(id) => (id, true),
                Err(_) => {
                    report.failed.push("playlists");
                    continue;
                }
            },
        };
        let have: HashSet<String> = engine
            .local_playlists()
            .iter()
            .find(|p| p.id == local_id)
            .map(|p| {
                p.entries
                    .iter()
                    .map(|e| e.provider_track_id.clone())
                    .collect()
            })
            .unwrap_or_default();
        let mut added = 0;
        for track in playlist
            .tracks
            .iter()
            .filter(|t| !have.contains(&t.provider_track_id))
        {
            if engine.add_local_entry(&local_id, entry(track)).is_ok() {
                added += 1;
            }
        }
        report.playlist_tracks_added += added;
        if created {
            report.playlists_created += 1;
        } else if added > 0 {
            report.playlists_updated += 1;
        }
    }
}

/// Counts of the last liked-songs read, for finding out why an import is shorter than YouTube Music's count.
/// Written to `youtube-import-diag.json` in the app data folder. It holds numbers and a stop reason only: no ids,
/// titles, cookies or URLs.
const DIAG_FILE: &str = "youtube-import-diag.json";
fn write_liked_diag(app: &AppHandle, import: &Import) {
    let Ok(dir) = app.path().app_data_dir() else {
        return;
    };
    let s = &import.liked_stats;
    let value = serde_json::json!({
        "section": "ytmusic.liked",
        "readOk": !import.failed.contains(&"likes"),
        "stop": s.stop,
        "pages": s.pages,
        "rows": s.rows,
        "noId": s.no_id,
        "otherRows": s.other,
        "tracks": s.tracks,
        "kept": s.kept,
        "headerCount": s.header_count,
        "continuationKeys": s.continuation_keys,
        "at": now_secs(),
    });
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join(DIAG_FILE),
        serde_json::to_vec_pretty(&value).unwrap_or_default(),
    );
}

pub async fn run(app: &AppHandle) -> Result<ImportReport, LibraryError> {
    run_mode(app, Mode::Full).await
}

/// Android only: mirrors the import into a progress notification (a Live Update on Android 16). Failures are ignored.
fn live_update(app: &AppHandle, state: &'static str) {
    #[cfg(target_os = "android")]
    {
        use tauri_plugin_lunor_native::LunorNativeExt;
        let app = app.clone();
        tauri::async_runtime::spawn_blocking(move || {
            let _ = app
                .lunor_native()
                .call::<serde_json::Value>("importProgress", serde_json::json!({ "state": state }));
        });
    }
    #[cfg(not(target_os = "android"))]
    let _ = (app, state);
}

async fn run_mode(app: &AppHandle, mode: Mode) -> Result<ImportReport, LibraryError> {
    if RUNNING.swap(true, Ordering::AcqRel) {
        return Err(LibraryError::InUse);
    }
    let result = run_inner(app, mode).await;
    RUNNING.store(false, Ordering::Release);
    // A quick pass that found nothing stays silent, so the settings line does not flicker every two minutes.
    let event = match &result {
        Ok(report) if mode == Mode::Quick && !report.changed() => None,
        Ok(report) => Some(ImportEvent::Done {
            report: report.clone(),
        }),
        Err(_) if mode == Mode::Quick => None,
        Err(_) => Some(ImportEvent::Failed),
    };
    if let Some(event) = event {
        live_update(
            app,
            if matches!(event, ImportEvent::Done { .. }) {
                "done"
            } else {
                "failed"
            },
        );
        let _ = emit_main(app, EVENT, &event);
    }
    result
}

async fn run_inner(app: &AppHandle, mode: Mode) -> Result<ImportReport, LibraryError> {
    let yt = app
        .state::<YouTubeState>()
        .yt
        .clone()
        .ok_or(LibraryError::Unavailable)?;
    if mode == Mode::Full {
        live_update(app, "running");
        let _ = emit_main(app, EVENT, &ImportEvent::Running);
    }
    let known = if mode == Mode::Quick {
        COUNTS
            .lock()
            .map(|c| c.clone().unwrap_or_default())
            .unwrap_or_default()
    } else {
        HashMap::new()
    };
    flush_likes(app).await;
    let mut import = fetch(&yt, &known).await;
    write_liked_diag(app, &import);
    // A like or un-like made here and not yet delivered wins over what YouTube still says.
    let undelivered: BTreeSet<String> =
        outbox_edit(app, |o| o.pending.keys().cloned().collect()).unwrap_or_default();
    import
        .liked
        .retain(|t| !undelivered.contains(&t.provider_track_id));
    let mut memory = load_memory(app);
    let mut report = ImportReport {
        failed: import.failed.clone(),
        ..Default::default()
    };

    if !(import.failed.contains(&"likes") && import.failed.contains(&"ytLikes")) {
        let incoming: BTreeSet<String> = import
            .liked
            .iter()
            .map(|t| t.provider_track_id.clone())
            .collect();
        let library = app.state::<LibraryState>();
        // Only songs that came from YouTube are removed, and only when the whole list was read.
        let removed: HashSet<String> = if import.liked_complete {
            memory
                .liked_ids
                .iter()
                .filter(|id| !incoming.contains(*id) && !undelivered.contains(*id))
                .cloned()
                .collect()
        } else {
            HashSet::new()
        };
        let outcome = library.with(|l| {
            let removed_n = if removed.is_empty() {
                0
            } else {
                l.remove_youtube_ids(&removed)?
            };
            let added_n = l.merge_append(import.liked.clone(), 5000)?;
            Ok((added_n, removed_n))
        });
        match outcome {
            Ok((added, removed_n)) => {
                report.liked_added = added;
                report.liked_removed = removed_n;
                memory.liked_ids = if import.liked_complete {
                    incoming
                } else {
                    memory.liked_ids.union(&incoming).cloned().collect()
                };
                if added + removed_n > 0 {
                    let _ = emit_main(app, "core://library/changed", &());
                }
            }
            Err(_) => report.failed.push("likes"),
        }
    }

    if !import.history.is_empty() {
        match app.state::<HistoryState>().0.with(|l| {
            l.merge_append(import.history.clone(), 100)
                .and_then(|n| Ok((n, l.list()?)))
        }) {
            Ok((n, tracks)) => {
                report.history_added = n;
                if n > 0 {
                    let _ = emit_main(app, "core://history/changed", &tracks);
                }
            }
            Err(_) => report.failed.push("history"),
        }
    }

    apply_playlists(app, &import, &memory.skipped_playlists, &mut report).await;
    if !import.failed.contains(&"playlists")
        && let Ok(mut counts) = COUNTS.lock()
    {
        *counts = Some(import.counts.clone());
    }
    save_memory(app, &memory);
    report.failed.sort_unstable();
    report.failed.dedup();
    Ok(report)
}

/// Starts the two-minute quick pass once. It runs only while the account is connected.
pub fn start_sync_loop(app: &AppHandle) {
    if LOOPING.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(QUICK_EVERY).await;
            let connected = app.state::<YouTubeState>().connected();
            if connected {
                let _ = run_mode(&app, Mode::Quick).await;
            }
        }
    });
}

fn stamp_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("youtube-import.last"))
}
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Automatic full pass: skipped when the last one finished less than six hours ago, unless `force`. Also starts
/// the quick loop.
pub async fn run_auto(app: &AppHandle, force: bool) {
    start_sync_loop(app);
    let path = stamp_path(app);
    if !force
        && let Some(last) = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| s.trim().parse::<u64>().ok())
        && now_secs().saturating_sub(last) < AUTO_EVERY_SECS
    {
        return;
    }
    if run(app).await.is_ok()
        && let Some(path) = path
    {
        let _ = std::fs::write(path, now_secs().to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::yt::Yt;
    use innerlight::{Client, FixtureTransport, Policy};
    use serde_json::json;
    use std::sync::Arc;

    #[test]
    fn marker_round_trips_and_rejects_other_descriptions() {
        assert_eq!(marker_id(&marker(Origin::Music, "PL123")), Some("PL123"));
        assert_eq!(marker_id(&marker(Origin::YouTube, "WL")), Some("WL"));
        assert_eq!(marker_id("My own playlist"), None);
        assert_eq!(
            marker_id("my text\nImported from YouTube Music [PL9]"),
            Some("PL9")
        );
        assert_eq!(marker_id("Imported from YouTube Music [PL1"), None);
    }

    #[test]
    fn changes_queue_only_when_sending_is_on_and_replace_older_ones() {
        let mut outbox = Outbox::default();
        assert!(!enqueue(&mut outbox, "r-G2pKr2_n8", true), "off by default");
        outbox.enabled = true;
        assert!(enqueue(&mut outbox, "r-G2pKr2_n8", true));
        assert!(enqueue(&mut outbox, "r-G2pKr2_n8", false), "latest wins");
        assert_eq!(outbox.pending.get("r-G2pKr2_n8"), Some(&false));
        for i in 0..OUTBOX_MAX {
            outbox.pending.insert(format!("{i:011}"), true);
        }
        assert!(!enqueue(&mut outbox, "aqz-KE-bpKQ", true), "full queue");
        assert!(
            enqueue(&mut outbox, "00000000001", false),
            "known id still updates"
        );
    }

    #[test]
    fn a_change_made_again_in_flight_stays_queued() {
        let mut outbox = Outbox {
            enabled: true,
            ..Default::default()
        };
        enqueue(&mut outbox, "r-G2pKr2_n8", true);
        enqueue(&mut outbox, "r-G2pKr2_n8", false); // user flipped it while "true" was being sent
        settle(&mut outbox, "r-G2pKr2_n8", true);
        assert_eq!(outbox.pending.get("r-G2pKr2_n8"), Some(&false));
        settle(&mut outbox, "r-G2pKr2_n8", false);
        assert!(outbox.pending.is_empty());
    }

    #[tokio::test]
    async fn flush_sends_in_order_reports_each_and_keeps_the_rest_after_a_failure() {
        let transport = Arc::new(
            FixtureTransport::new()
                .with_match(
                    "like/removelike",
                    "aqz-KE-bpKQ",
                    json!({"responseContext": {}}),
                )
                .with_match("like/like", "r-G2pKr2_n8", json!({"responseContext": {}}))
                .with_raw("like/like", 500, b"{}"),
        );
        let client = Client::builder()
            .transport(transport.clone())
            // No retry: innerlight's polite gate would re-send a 5xx twice more, and this test counts requests.
            .policy(Policy::no_retry())
            .build()
            .unwrap();
        client.set_cookie("SAPISID=abc123; SID=x").unwrap();
        let yt = Yt::from_client(client);
        let waiting = vec![
            ("r-G2pKr2_n8".to_owned(), true),
            ("aqz-KE-bpKQ".to_owned(), false),
            ("abcdefghijk".to_owned(), true), // the service fails this one
            ("bbbbbbbbbbb".to_owned(), true), // never attempted
        ];
        let mut done = Vec::new();
        deliver(&yt, waiting, |id, liked| done.push((id.to_owned(), liked))).await;
        assert_eq!(
            done,
            [
                ("r-G2pKr2_n8".to_owned(), true),
                ("aqz-KE-bpKQ".to_owned(), false)
            ]
        );
        let endpoints: Vec<_> = transport
            .requests()
            .iter()
            .map(|r| r.endpoint.clone())
            .collect();
        assert_eq!(endpoints, ["like/like", "like/removelike", "like/like"]);
    }
}

/// Imports a single YouTube / YouTube Music playlist (by URL or id) into the local library.
/// Public playlists are fetched anonymously; no sign-in is required. Returns the new local playlist id.
#[tauri::command]
pub async fn youtube_import_playlist_from_url(
    app: AppHandle,
    window: WebviewWindow,
    engine: State<'_, SyncEngineState>,
    url: String,
) -> Result<String, ErrorCategory> {
    // Only the main window may trigger imports.
    if window.label() != "main" {
        return Err(ErrorCategory::Validation);
    }
    let id = parse_youtube_playlist_id(&url).ok_or(ErrorCategory::Validation)?;

    let yt = app
        .state::<crate::commands::youtube::YouTubeState>()
        .yt
        .clone()
        .filter(|_| {
            app.state::<crate::commands::youtube::YouTubeState>()
                .connected()
        })
        .unwrap_or_else(|| {
            crate::core::yt::Yt::new(std::time::Duration::from_secs(30)).unwrap_or_else(|_| {
                crate::core::yt::Yt::from_client(innerlight::Client::builder().build().unwrap())
            })
        });
    let mut videos = Vec::new();
    let mut cont: Option<String> = None;
    let mut pages = 0u32;
    loop {
        if videos.len() >= 500 || pages >= 20 {
            break;
        }
        let page = yt
            .youtube()
            .playlist(&id, cont.as_deref())
            .await
            .map_err(|_| ErrorCategory::Offline)?;
        videos.extend(page.items);
        cont = page.continuation;
        pages += 1;
        if cont.is_none() {
            break;
        }
    }
    let tracks: Vec<SavedTrack> = videos
        .into_iter()
        .filter_map(crate::core::youtube_import::video_track)
        .collect();

    let mut checkout = engine.checkout().await?;
    let eng = checkout.engine();
    let short = &id[..id.len().min(8)];
    let name = format!("YouTube playlist {short}");
    let local_id = eng
        .create_local(&name, None)
        .map_err(|_| ErrorCategory::Validation)?;
    for t in &tracks {
        let _ = eng.add_local_entry(&local_id, entry(t));
    }
    let _ = app;
    Ok(local_id)
}

fn parse_youtube_playlist_id(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if innerlight::ids::is_playlist_id(trimmed) {
        return Some(trimmed.to_owned());
    }
    // Query string form: ...?list=PL...
    if let Some(idx) = trimmed.find("list=") {
        let rest = &trimmed[idx + 5..];
        let end = rest
            .find(|c: char| c == '&' || c == '#' || c.is_whitespace())
            .unwrap_or(rest.len());
        let candidate = &rest[..end];
        if innerlight::ids::is_playlist_id(candidate) {
            return Some(candidate.to_owned());
        }
    }
    // Path segment form: /playlist/PL...
    if let Ok(url) = url::Url::parse(trimmed) {
        let segs: Vec<&str> = url.path_segments().map(|s| s.collect()).unwrap_or_default();
        for seg in segs {
            if innerlight::ids::is_playlist_id(seg) {
                return Some(seg.to_owned());
            }
        }
    }
    None
}
