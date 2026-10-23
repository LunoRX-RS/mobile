//! Discord Rich Presence settings. The presence itself is driven by the player, see `platform::discord`.
use crate::platform::discord::{self, Settings, Status};
use serde::{Deserialize, Serialize};
use tauri::WebviewWindow;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordDto {
    pub enabled: bool,
    pub client_id: String,
    /// The built-in Application ID used when `client_id` is empty.
    pub default_client_id: &'static str,
    pub status: Status,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiscordArgs {
    pub enabled: bool,
    pub client_id: String,
}

fn main(window: &WebviewWindow) -> Result<(), &'static str> {
    if window.label() == "main" {
        Ok(())
    } else {
        Err("forbidden")
    }
}

#[tauri::command]
pub fn discord_settings_get(window: WebviewWindow) -> Result<DiscordDto, &'static str> {
    main(&window)?;
    let (s, status) = discord::settings();
    Ok(DiscordDto {
        enabled: s.enabled,
        client_id: s.client_id,
        default_client_id: discord::DEFAULT_CLIENT_ID,
        status,
    })
}

/// Saves the choice and applies it. Refuses a malformed Application ID.
#[tauri::command]
pub fn discord_settings_set(window: WebviewWindow, args: DiscordArgs) -> Result<(), &'static str> {
    main(&window)?;
    discord::apply(Settings {
        enabled: args.enabled,
        client_id: args.client_id,
    })
}
