//! OS integration. macOS: system Now Playing, hardware media keys and the Playback menu. iOS and Android: lock-screen
//! metadata through the lunor-native plugin. Every other platform gets two empty functions, so callers need no cfg.

#[cfg(target_os = "android")]
pub mod android_keyring;
pub mod discord;
pub mod fonts;
#[cfg(target_os = "ios")]
pub mod ios_keyring;

#[cfg(any(target_os = "macos", mobile))]
pub mod control;
#[cfg(target_os = "macos")]
mod macos_media;
#[cfg(target_os = "macos")]
mod menu;
#[cfg(mobile)]
mod mobile_media;

/// Wires media keys, Now Playing and the Playback menu. Call from `setup`, on the main thread, after
/// `PlayerState` is managed. A failure is logged once and never retried.
#[cfg(target_os = "macos")]
pub fn install(app: &tauri::AppHandle) {
    macos_media::set_app_icon();
    let menu = menu::install(app);
    macos_media::install(app, menu);
}

/// Tells the OS integration that player state, position or volume may have changed. Cheap and
/// non-blocking; safe to call from any thread.
#[cfg(target_os = "macos")]
pub fn poke() {
    macos_media::poke();
}

#[cfg(mobile)]
pub fn install(app: &tauri::AppHandle) {
    mobile_media::install(app);
}

#[cfg(mobile)]
pub fn poke() {
    mobile_media::poke();
}

#[cfg(not(any(target_os = "macos", mobile)))]
pub fn install(_app: &tauri::AppHandle) {}

#[cfg(not(any(target_os = "macos", mobile)))]
pub fn poke() {}
