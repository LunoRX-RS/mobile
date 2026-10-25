//! Every event the app sends to the main window goes through here, so mobile and desktop keep one call shape and files
//! port mechanically. On desktop this also notes the event in the activity log; that log is not part of the mobile app.
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// Emits to the main window and returns the Tauri result unchanged.
pub fn emit_main<T: Serialize>(
    app: &AppHandle,
    event: &'static str,
    payload: &T,
) -> tauri::Result<()> {
    app.emit_to("main", event, payload)
}
