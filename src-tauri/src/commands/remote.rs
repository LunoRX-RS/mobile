//! Tauri commands for cross-device remote control. Thin: the webview starts and stops the link and sends frames;
//! frames and status changes come back as events. The session token never crosses this boundary.
use std::sync::{Mutex, PoisonError};

use serde_json::Value;
use tauri::{AppHandle, Emitter, State, WebviewWindow};
use tokio::sync::mpsc;

use crate::sync::credentials::KeyringTokenStore;
use crate::sync::remote::{self, LinkStatus, MAX_FRAME_BYTES};
use crate::sync::settings::default_base_url;

#[cfg(mobile)]
const KIND: &str = "phone";
#[cfg(not(mobile))]
const KIND: &str = "computer";

#[derive(Default)]
pub struct RemoteState {
    link: Mutex<Option<Link>>,
}

struct Link {
    out: mpsc::Sender<String>,
    task: tauri::async_runtime::JoinHandle<()>,
}

impl Drop for Link {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RemoteError {
    Validation,
}

fn main(window: &WebviewWindow) -> Result<(), RemoteError> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err(RemoteError::Validation)
    }
}

/// Starts (or restarts) the link for this device. `device_id` is the UUID this install already uses for stats.
#[tauri::command]
pub fn remote_start(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, RemoteState>,
    device_id: String,
    name: String,
) -> Result<(), RemoteError> {
    main(&window)?;
    let id_shape = device_id.len() == 36
        && device_id.char_indices().all(|(i, c)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                c == '-'
            } else {
                c.is_ascii_hexdigit()
            }
        });
    if !id_shape {
        return Err(RemoteError::Validation);
    }
    let name = name.trim().to_owned();
    if !(1..=64).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(RemoteError::Validation);
    }
    let url = remote::socket_url(&default_base_url(), &device_id, &name, KIND)
        .ok_or(RemoteError::Validation)?;
    let (tx, rx) = mpsc::channel::<String>(32);
    let events = app.clone();
    let statuses = app.clone();
    let task = tauri::async_runtime::spawn(async move {
        remote::run(
            url,
            &KeyringTokenStore,
            rx,
            move |text| {
                // Forwarded as a string; the webview parses and validates it.
                let _ = events.emit_to("main", "remote://message", text);
            },
            move |status: LinkStatus| {
                let _ = statuses.emit_to("main", "remote://status", status);
            },
        )
        .await;
    });
    // Replacing the old link drops it, which aborts its task.
    *state.link.lock().unwrap_or_else(PoisonError::into_inner) = Some(Link { out: tx, task });
    Ok(())
}

#[tauri::command]
pub fn remote_stop(
    app: AppHandle,
    window: WebviewWindow,
    state: State<'_, RemoteState>,
) -> Result<(), RemoteError> {
    main(&window)?;
    state
        .link
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take();
    let _ = app.emit_to("main", "remote://status", LinkStatus::Offline);
    Ok(())
}

/// Sends one JSON text frame (a `state` or `command` message). Dropped quietly when the link is down: the webview
/// republishes state on a timer, and a command to an unreachable device has nowhere to go.
#[tauri::command]
pub fn remote_send(
    window: WebviewWindow,
    state: State<'_, RemoteState>,
    message: Value,
) -> Result<(), RemoteError> {
    main(&window)?;
    let kind = message.get("type").and_then(Value::as_str);
    if !matches!(kind, Some("state" | "command")) {
        return Err(RemoteError::Validation);
    }
    let text = message.to_string();
    if text.len() > MAX_FRAME_BYTES {
        return Err(RemoteError::Validation);
    }
    if let Some(link) = state
        .link
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
    {
        let _ = link.out.try_send(text);
    }
    Ok(())
}
