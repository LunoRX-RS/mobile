//! Lists the fonts installed on this computer. The names go to the interface, which uses them by family name.
use tauri::WebviewWindow;

/// Synchronous on purpose: Tauri runs it on the main thread, which the macOS font manager requires.
#[tauri::command]
pub fn fonts_installed(window: WebviewWindow) -> Result<Vec<String>, &'static str> {
    if window.label() != "main" {
        return Err("forbidden");
    }
    Ok(crate::platform::fonts::installed_families())
}
