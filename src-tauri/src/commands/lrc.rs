//! Lyric Maker IPC: thin layer over `core::lrc`. The frontend sends ids and lyric data, never paths.
use crate::core::lrc::{self, LineIn, LrcError};
use crate::core::playback::lyrics::TimedLine;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager, WebviewWindow};
use tauri_plugin_opener::OpenerExt;

fn dir(app: &AppHandle) -> Result<PathBuf, LrcError> {
    let path = app.path();
    path.document_dir()
        .map(|d| d.join("LUNOR-X Lyrics"))
        .or_else(|_| path.app_data_dir().map(|d| d.join("lyrics")))
        .map_err(|_| LrcError::StorageUnavailable)
}

fn main(window: &WebviewWindow) -> Result<(), LrcError> {
    (window.label() == "main")
        .then_some(())
        .ok_or(LrcError::Validation)
}

/// File access is blocking; keep it off the async runtime threads.
async fn blocking<T: Send + 'static>(
    job: impl FnOnce() -> Result<T, LrcError> + Send + 'static,
) -> Result<T, LrcError> {
    tauri::async_runtime::spawn_blocking(job)
        .await
        .map_err(|_| LrcError::IoFailed)?
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcIdArgs {
    pub provider_track_id: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcLoaded {
    pub exists: bool,
    pub path: Option<String>,
    pub lines: Vec<TimedLine>,
    pub title: Option<String>,
    pub artists: Vec<String>,
}

#[tauri::command]
pub async fn lrc_load(
    window: WebviewWindow,
    app: AppHandle,
    args: LrcIdArgs,
) -> Result<LrcLoaded, LrcError> {
    main(&window)?;
    lrc::validate_id(&args.provider_track_id)?;
    let dir = dir(&app)?;
    blocking(move || {
        Ok(match lrc::load(&dir, &args.provider_track_id)? {
            Some(l) => LrcLoaded {
                exists: true,
                path: Some(l.path.to_string_lossy().into_owned()),
                lines: l.lines,
                title: l.title,
                artists: l.artists,
            },
            None => LrcLoaded {
                exists: false,
                path: None,
                lines: Vec::new(),
                title: None,
                artists: Vec::new(),
            },
        })
    })
    .await
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcSaveArgs {
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub duration_ms: i64,
    pub lines: Vec<LineIn>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcSaved {
    pub path: String,
    pub count: usize,
    pub bytes: usize,
}

#[tauri::command]
pub async fn lrc_save(
    window: WebviewWindow,
    app: AppHandle,
    args: LrcSaveArgs,
) -> Result<LrcSaved, LrcError> {
    main(&window)?;
    lrc::validate_id(&args.provider_track_id)?;
    let prepared = lrc::prepare(&args.title, &args.artists, args.duration_ms, &args.lines)?;
    let dir = dir(&app)?;
    blocking(move || {
        let saved = lrc::save(&dir, &args.provider_track_id, &prepared)?;
        Ok(LrcSaved {
            path: saved
                .path
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default(),
            count: saved.count,
            bytes: saved.bytes,
        })
    })
    .await
}

#[derive(Serialize)]
pub struct LrcDeleted {
    pub deleted: bool,
}

#[tauri::command]
pub async fn lrc_delete(
    window: WebviewWindow,
    app: AppHandle,
    args: LrcIdArgs,
) -> Result<LrcDeleted, LrcError> {
    main(&window)?;
    lrc::validate_id(&args.provider_track_id)?;
    let dir = dir(&app)?;
    blocking(move || {
        lrc::delete(&dir, &args.provider_track_id).map(|deleted| LrcDeleted { deleted })
    })
    .await
}

#[tauri::command]
pub async fn lrc_reveal(
    window: WebviewWindow,
    app: AppHandle,
    args: LrcIdArgs,
) -> Result<(), LrcError> {
    main(&window)?;
    lrc::validate_id(&args.provider_track_id)?;
    let dir = dir(&app)?;
    let path = blocking(move || {
        lrc::find(&dir, &args.provider_track_id)?
            .filter(|p| lrc::inside(&dir, p))
            .ok_or(LrcError::NotFound)
    })
    .await?;
    app.opener()
        .reveal_item_in_dir(path)
        .map_err(|_| LrcError::IoFailed)
}

#[derive(Serialize)]
pub struct LrcListed {
    pub items: Vec<LrcListItem>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LrcListItem {
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub lines: usize,
    pub modified_ms: u64,
}

/// `lrc_list {}`: an empty args object, so the call shape matches the other commands.
#[derive(Deserialize)]
pub struct LrcListArgs {}

#[tauri::command]
pub async fn lrc_list(
    window: WebviewWindow,
    app: AppHandle,
    _args: LrcListArgs,
) -> Result<LrcListed, LrcError> {
    main(&window)?;
    let dir = dir(&app)?;
    blocking(move || {
        Ok(LrcListed {
            items: lrc::list(&dir)?
                .into_iter()
                .map(|i| LrcListItem {
                    provider_track_id: i.provider_track_id,
                    title: i.title,
                    artists: i.artists,
                    lines: i.lines,
                    modified_ms: i.modified_ms,
                })
                .collect(),
        })
    })
    .await
}
