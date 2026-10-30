/// A value from `.env` at the repo root (`KEY=value`), else `default`. Baked into the binary at build time; the run-time
/// environment variable of the same name still overrides it.
fn dotenv_default(key: &str, default: &str) -> String {
    println!("cargo:rerun-if-changed=../.env");
    println!("cargo:rerun-if-env-changed={key}");
    let from_file = std::fs::read_to_string("../.env").ok().and_then(|text| {
        text.lines().find_map(|line| {
            let (k, value) = line.trim().split_once('=')?;
            (k.trim() == key).then(|| value.trim().trim_matches('"').to_owned())
        })
    });
    let value = from_file
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| default.to_owned());
    // Only plain URL characters: the value is embedded in source as a string literal by env!().
    if value
        .bytes()
        .all(|b| b.is_ascii_graphic() && b != b'"' && b != b'\\')
    {
        value
    } else {
        default.to_owned()
    }
}

fn main() {
    println!(
        "cargo:rustc-env=LUNORX_DEFAULT_BACKEND_URL={}",
        dotenv_default("LUNORX_BACKEND_URL", "https://api.lunorx.xyz")
    );
    // Where recipients open a share link: the website, not the API.
    println!(
        "cargo:rustc-env=LUNORX_DEFAULT_SHARE_BASE={}",
        dotenv_default("LUNORX_SHARE_BASE", "https://play.lunorx.xyz")
    );
    println!("cargo:rerun-if-env-changed=MPV_LIB_DIR");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        let directory = std::env::var("MPV_LIB_DIR").ok().or_else(|| {
            ["/opt/homebrew/opt/mpv/lib", "/usr/local/opt/mpv/lib"]
                .into_iter()
                .find(|dir| std::path::Path::new(dir).join("libmpv.dylib").is_file())
                .map(str::to_owned)
        });
        if let Some(directory) = directory {
            println!("cargo:rustc-link-search=native={directory}");
            println!("cargo:rustc-link-arg=-Wl,-rpath,{directory}");
        }
    }
    tauri_build::try_build(tauri_build::Attributes::new().app_manifest(
        tauri_build::AppManifest::new().commands(&[
            "library_history",
            "library_clear_history",
            "discovery_feed",
            "discovery_genres",
            "fonts_installed",
            "discord_settings_get",
            "discord_settings_set",
            "search_suggestions",
            "library_likes",
            "library_set_like",
            "library_artist",
            "library_album",
            "search_query",
            "search_cancel",
            "providers_list",
            "provider_credential_set",
            "provider_credential_status",
            "provider_credential_clear",
            "youtube_connect",
            "youtube_connect_finish",
            "youtube_connection_status",
            "youtube_import",
            "youtube_import_playlist_from_url",
            "podcast_browse",
            "youtube_disconnect",
            "youtube_like_push_get",
            "youtube_like_push_set",
            "sync_sign_in",
            "sync_sign_out",
            "sync_discard_pending",
            "sync_status",
            "sync_create_local_playlist",
            "sync_add_local_entry",
            "sync_upload_playlist",
            "sync_list_local_playlists",
            "sync_check_playlists",
            "sync_set_playlist_description",
            "sync_playlist_entries",
            "sync_delete_local_playlist",
            "sync_rename_local_playlist",
            "sync_move_local_entry",
            "telemetry_get",
            "telemetry_set",
            "telemetry_record",
            "telemetry_flush",
            "telemetry_delete",
            "remote_start",
            "remote_stop",
            "remote_send",
            "stats_local",
            "stats_report",
            "stats_clear_local",
            "stats_set_cloud",
            "stats_cloud_sync",
            "stats_cloud_clear",
            "sync_remove_local_entry",
            "sync_flush_pending",
            "sync_register",
            "shared_link_take",
            "shared_playlist_get",
            "shared_token_from_url",
            "sync_share_playlist",
            "sync_list_shares",
            "sync_revoke_share",
            "sync_resolve_conflict",
            "player_audio_settings",
            "player_set_audio_settings",
            "player_effect_presets",
            "player_state",
            "player_progress",
            "player_track_info",
            "player_load",
            "player_play",
            "player_pause",
            "player_resume",
            "player_stop",
            "player_skip",
            "player_set_mode",
            "player_queue_edit",
            "player_queue_more",
            "player_seek",
            "player_set_volume",
            "player_observe",
            "player_open_external",
            "offline_status",
            "offline_keep",
            "offline_cancel",
            "offline_remove",
            "offline_set_limit",
            "offline_usage",
            "offline_clear_transient",
            "lrc_load",
            "lrc_save",
            "lrc_delete",
            "lrc_reveal",
            "lrc_list",
            "lyrics_source_fetch",
            "lyrics_source_search",
            "lyrics_source_bind",
            "lyrics_source_unbind",
            "lyrics_settings_get",
            "lyrics_settings_set",
        ]),
    ))
    .unwrap()
}
