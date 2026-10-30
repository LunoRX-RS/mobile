mod commands;
pub mod core;
mod platform;
// Public so tests/backend_sync_live.rs can reach the sync client API (was private).
pub mod sync;

use sync::api::ApiClient;
use sync::credentials::KeyringTokenStore;
use sync::engine::{ErrorCategory, SyncEngine};
use sync::settings::default_base_url;
use tauri::{Emitter, Manager};
use tauri_plugin_deep_link::DeepLinkExt;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let builder = tauri::Builder::default();
    // A phone has no second instance and no window to raise.
    #[cfg(desktop)]
    let builder = builder.plugin(tauri_plugin_single_instance::init(|app, _, _| {
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.unminimize();
            let _ = window.show();
            let _ = window.set_focus();
        }
    }));
    #[cfg(mobile)]
    let builder = builder.plugin(tauri_plugin_lunor_native::init());
    builder
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_deep_link::init())
        .setup(|app| {
            #[cfg(target_os = "android")]
            {
                platform::android_tls::init(app.handle());
                platform::android_keyring::init(app.handle());
            }
            let library_path = app
                .path()
                .app_data_dir()
                .map(|dir| dir.join("liked-songs.json"))
                .map_err(|_| core::library::LibraryError::Storage);
            app.manage(commands::library::LibraryState::open(library_path));
            // Liked-songs sync keeps its account files under the app data directory next to the signed-out file.
            if let Ok(dir) = app.path().app_data_dir() {
                app.manage(commands::likes::LikesState::new(dir));
            }
            app.manage(commands::library::HistoryState(
                commands::library::LibraryState::open(
                    app.path()
                        .app_data_dir()
                        .map(|dir| dir.join("recently-played.json"))
                        .map_err(|_| core::library::LibraryError::Storage),
                ),
            ));
            let telemetry = commands::telemetry::TelemetryState::open(
                app.path()
                    .app_data_dir()
                    .map(|dir| dir.join("telemetry.json"))
                    .map_err(|_| core::library::LibraryError::Storage),
            );
            telemetry.record("app.start");
            app.manage(telemetry);
            app.manage(commands::stats::AnalyticsState::open(
                app.path()
                    .app_data_dir()
                    .map(|dir| dir.join("listening-log.json"))
                    .map_err(|_| core::library::LibraryError::Storage),
            ));
            app.manage(commands::stats::StatsState::open(
                app.path()
                    .app_data_dir()
                    .map(|dir| dir.join("listening-stats.json"))
                    .map_err(|_| core::library::LibraryError::Storage),
            ));
            let yt = core::playback::extract::resolver().ok();
            app.manage(commands::state::SearchState::production(yt.clone()));
            app.manage(commands::youtube::YouTubeState::new(yt));
            let offline = app
                .path()
                .app_data_dir()
                .map_err(|_| core::playback::cache::CacheError::Storage)
                .and_then(|dir| core::playback::cache::DiskCache::open(dir.join("media")))
                .map(|cache| {
                    let handle = app.handle().clone();
                    core::playback::cache::OfflineManager::new(
                        cache,
                        std::sync::Arc::new(move || {
                            let _ = handle.emit_to("main", "core://offline/changed", ());
                        }),
                    )
                });
            if let Err(kind) = &offline {
                eprintln!("media cache unavailable: {kind:?}");
            }
            app.manage(commands::offline::OfflineState { manager: offline });
            platform::discord::start(app.path().app_data_dir().ok());
            commands::player::PlayerState::start(app.handle());
            platform::install(app.handle());
            tauri::async_runtime::spawn(commands::youtube::YouTubeState::restore(
                app.handle().clone(),
            ));
            let engine = app
                .path()
                .app_data_dir()
                .map_err(|_| ErrorCategory::Storage)
                .and_then(|dir| {
                    std::fs::create_dir_all(&dir).map_err(|_| ErrorCategory::Storage)?;
                    let api = ApiClient::new(&default_base_url())
                        .map_err(|e| sync::engine::category(&e))?;
                    SyncEngine::new(
                        api,
                        Box::new(KeyringTokenStore),
                        dir.join("sync-snapshot.json"),
                    )
                });
            // Only the category is logged; the app still starts and the sync commands fail with it.
            if let Err(kind) = &engine {
                eprintln!("sync engine unavailable: {kind:?}");
            }
            app.manage(commands::sync::SyncEngineState::new(engine));
            commands::sync::start_auto_sync(app.handle().clone());
            app.manage(commands::remote::RemoteState::default());
            app.manage(commands::share::PendingShare::default());
            commands::likes::start(app.handle());
            // Links that opened the app, and links while it runs (on Android and iOS the plugin forwards them here).
            let handle = app.handle().clone();
            app.deep_link()
                .on_open_url(move |event| commands::share::accept_urls(&handle, &event.urls()));
            if let Ok(Some(urls)) = app.deep_link().get_current() {
                commands::share::accept_urls(app.handle(), &urls);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::library::library_history,
            commands::library::library_clear_history,
            commands::discovery::discovery_feed,
            commands::discovery::discovery_genres,
            commands::fonts::fonts_installed,
            commands::discord::discord_settings_get,
            commands::discord::discord_settings_set,
            commands::discovery::search_suggestions,
            commands::library::library_likes,
            commands::library::library_set_like,
            commands::library::library_artist,
            commands::library::library_album,
            commands::search::search_query,
            commands::search::search_cancel,
            commands::search::providers_list,
            commands::providers::provider_credential_set,
            commands::providers::provider_credential_status,
            commands::providers::provider_credential_clear,
            commands::youtube::youtube_connect,
            commands::youtube::youtube_connect_finish,
            commands::youtube::youtube_connection_status,
            commands::youtube::youtube_import,
            commands::youtube_import::youtube_import_playlist_from_url,
            commands::podcasts::podcast_browse,
            commands::podcasts::podcast_shows,
            commands::podcasts::podcast_episodes,
            commands::youtube::youtube_disconnect,
            commands::youtube::youtube_like_push_get,
            commands::youtube::youtube_like_push_set,
            commands::share::shared_link_take,
            commands::share::shared_token_from_url,
            commands::share::shared_playlist_get,
            commands::sync::sync_sign_in,
            commands::sync::sync_sign_out,
            commands::sync::sync_discard_pending,
            commands::sync::sync_status,
            commands::sync::sync_create_local_playlist,
            commands::sync::sync_add_local_entry,
            commands::sync::sync_upload_playlist,
            commands::sync::sync_list_local_playlists,
            commands::sync::sync_check_playlists,
            commands::sync::sync_set_playlist_description,
            commands::sync::sync_playlist_entries,
            commands::sync::sync_delete_local_playlist,
            commands::sync::sync_rename_local_playlist,
            commands::sync::sync_move_local_entry,
            commands::telemetry::telemetry_get,
            commands::telemetry::telemetry_set,
            commands::telemetry::telemetry_record,
            commands::telemetry::telemetry_flush,
            commands::telemetry::telemetry_delete,
            commands::remote::remote_start,
            commands::remote::remote_stop,
            commands::remote::remote_send,
            commands::stats::stats_local,
            commands::stats::stats_report,
            commands::stats::stats_clear_local,
            commands::stats::stats_set_cloud,
            commands::stats::stats_cloud_sync,
            commands::stats::stats_cloud_clear,
            commands::sync::sync_remove_local_entry,
            commands::sync::sync_flush_pending,
            commands::sync::sync_register,
            commands::sync::sync_share_playlist,
            commands::sync::sync_list_shares,
            commands::sync::sync_revoke_share,
            commands::sync::sync_resolve_conflict,
            commands::player::player_audio_settings,
            commands::player::player_set_audio_settings,
            commands::player::player_effect_presets,
            commands::player::player_state,
            commands::player::player_progress,
            commands::player::player_track_info,
            commands::player::player_load,
            commands::player::player_play,
            commands::player::player_pause,
            commands::player::player_resume,
            commands::player::player_stop,
            commands::player::player_skip,
            commands::player::player_set_mode,
            commands::player::player_queue_edit,
            commands::player::player_queue_more,
            commands::player::player_seek,
            commands::player::player_set_volume,
            commands::player::player_observe,
            commands::player::player_open_external,
            commands::offline::offline_status,
            commands::offline::offline_keep,
            commands::offline::offline_cancel,
            commands::offline::offline_remove,
            commands::offline::offline_set_limit,
            commands::offline::offline_usage,
            commands::offline::offline_clear_transient,
            commands::lrc::lrc_load,
            commands::lrc::lrc_save,
            commands::lrc::lrc_delete,
            commands::lrc::lrc_reveal,
            commands::lrc::lrc_list,
            commands::lyric_sources::lyrics_source_fetch,
            commands::lyric_sources::lyrics_source_search,
            commands::lyric_sources::lyrics_source_bind,
            commands::lyric_sources::lyrics_source_unbind,
            commands::lyric_sources::lyrics_settings_get,
            commands::lyric_sources::lyrics_settings_set
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
