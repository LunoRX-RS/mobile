//! Listening statistics: device-local play counts, plus an opt-in copy on the backend.
//! The local file is the source of truth. The cloud commands only send play counts, never provider tokens.
use crate::core::analytics::{PlayLog, Report};
use crate::core::library::LibraryError;
use crate::core::stats::{PlayCount, PlayStats, StatsView};
use crate::sync::api::StatsRow;
use crate::sync::engine::{self, ErrorCategory, Host};
use serde::Serialize;
use tauri::{State, WebviewWindow};

use super::sync::SyncEngineState;

pub struct StatsState {
    stats: std::sync::Mutex<Result<PlayStats, LibraryError>>,
    path: Result<std::path::PathBuf, LibraryError>,
}

impl StatsState {
    pub fn open(path: Result<std::path::PathBuf, LibraryError>) -> Self {
        let stats = path
            .as_ref()
            .map_err(|e| *e)
            .and_then(|p| PlayStats::open(p.clone()));
        if let Err(kind) = &stats {
            eprintln!("listening stats unavailable: {kind:?}");
        }
        Self {
            stats: std::sync::Mutex::new(stats),
            path,
        }
    }

    pub(crate) fn with<T>(
        &self,
        action: impl FnOnce(&PlayStats) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let mut stats = self.stats.lock().map_err(|_| LibraryError::Storage)?;
        if stats.is_err() {
            *stats = self
                .path
                .as_ref()
                .map_err(|e| *e)
                .and_then(|p| PlayStats::open(p.clone()));
        }
        action(stats.as_ref().map_err(|e| *e)?)
    }
}

pub struct AnalyticsState {
    log: std::sync::Mutex<Result<PlayLog, LibraryError>>,
    path: Result<std::path::PathBuf, LibraryError>,
}

impl AnalyticsState {
    pub fn open(path: Result<std::path::PathBuf, LibraryError>) -> Self {
        let log = path
            .as_ref()
            .map_err(|e| *e)
            .and_then(|p| PlayLog::open(p.clone()));
        if let Err(kind) = &log {
            eprintln!("listening log unavailable: {kind:?}");
        }
        Self {
            log: std::sync::Mutex::new(log),
            path,
        }
    }

    pub(crate) fn with<T>(
        &self,
        action: impl FnOnce(&mut PlayLog) -> Result<T, LibraryError>,
    ) -> Result<T, LibraryError> {
        let mut log = self.log.lock().map_err(|_| LibraryError::Storage)?;
        if log.is_err() {
            *log = self
                .path
                .as_ref()
                .map_err(|e| *e)
                .and_then(|p| PlayLog::open(p.clone()));
        }
        action(log.as_mut().map_err(|e| *e)?)
    }
}

fn main(window: &WebviewWindow) -> Result<(), LibraryError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(LibraryError::Validation)
    }
}

#[tauri::command]
pub fn stats_local(
    window: WebviewWindow,
    state: State<'_, StatsState>,
) -> Result<StatsView, LibraryError> {
    main(&window)?;
    state.with(PlayStats::view)
}

#[tauri::command]
pub fn stats_clear_local(
    window: WebviewWindow,
    state: State<'_, StatsState>,
    log: State<'_, AnalyticsState>,
) -> Result<StatsView, LibraryError> {
    main(&window)?;
    log.with(PlayLog::clear)?;
    state.with(PlayStats::clear)
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReportArgs {
    /// JavaScript's `Date.getTimezoneOffset()`.
    pub tz_offset_min: i32,
    pub from_ms: u64,
    pub to_ms: u64,
}

/// Listening report for a time range, built from the on-device play log.
#[tauri::command]
pub fn stats_report(
    window: WebviewWindow,
    stats: State<'_, StatsState>,
    log: State<'_, AnalyticsState>,
    args: ReportArgs,
) -> Result<Report, LibraryError> {
    main(&window)?;
    if !(-1440..=1440).contains(&args.tz_offset_min) || args.from_ms > args.to_ms {
        return Err(LibraryError::Validation);
    }
    let meta = stats.with(PlayStats::view)?.tracks;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64);
    log.with(|l| {
        Ok(crate::core::analytics::report(
            l.events(),
            &meta,
            args.tz_offset_min,
            args.from_ms,
            args.to_ms,
            now,
        ))
    })
}

#[tauri::command]
pub fn stats_set_cloud(
    window: WebviewWindow,
    state: State<'_, StatsState>,
    enabled: bool,
) -> Result<StatsView, LibraryError> {
    main(&window)?;
    state.with(|s| s.set_cloud(enabled))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudStats {
    pub tracks: Vec<StatsRow>,
}

fn rows(tracks: &[PlayCount]) -> Vec<StatsRow> {
    tracks
        .iter()
        .map(|t| StatsRow {
            provider: t.provider.clone(),
            provider_track_id: t.provider_track_id.clone(),
            // The server accepts shorter text than the local file does.
            title: t.title.chars().take(512).collect(),
            artists: t
                .artists
                .iter()
                .take(30)
                .map(|a| a.chars().take(256).collect())
                .collect(),
            plays: t.plays,
            last_played_ms: t.last_played_ms,
        })
        .collect()
}

/// Uploads this device's counts and returns the totals over all devices. Refused while cloud sync is off.
#[tauri::command]
pub async fn stats_cloud_sync(
    window: WebviewWindow,
    stats: State<'_, StatsState>,
    sync: State<'_, SyncEngineState>,
) -> Result<CloudStats, ErrorCategory> {
    main(&window).map_err(|_| ErrorCategory::Validation)?;
    let view = stats
        .with(PlayStats::view)
        .map_err(|_| ErrorCategory::Storage)?;
    if !view.cloud {
        return Err(ErrorCategory::Validation);
    }
    let _net = sync.net.lock().await;
    let tracks = engine::stats_sync(
        &mut Host::Shared(&sync),
        &view.device_id,
        &rows(&view.tracks),
    )
    .await?;
    Ok(CloudStats { tracks })
}

/// Deletes the user's stats from the server and turns cloud sync off. Local counts stay.
#[tauri::command]
pub async fn stats_cloud_clear(
    window: WebviewWindow,
    stats: State<'_, StatsState>,
    sync: State<'_, SyncEngineState>,
) -> Result<(), ErrorCategory> {
    main(&window).map_err(|_| ErrorCategory::Validation)?;
    {
        let _net = sync.net.lock().await;
        engine::stats_clear_remote(&mut Host::Shared(&sync)).await?;
    }
    stats
        .with(|s| s.set_cloud(false).map(|_| ()))
        .map_err(|_| ErrorCategory::Storage)
}
