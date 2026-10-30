pub mod audio;
pub mod command;
pub(crate) mod diag;
pub mod engine;
pub mod extract;
pub mod host;
#[cfg(desktop)]
mod locale;
pub mod lyrics;
#[cfg(mobile)]
pub mod mobile;
#[cfg_attr(desktop, allow(dead_code))]
// used by the mobile host only; built on desktop so its tests run in CI
mod mobile_protocol;
#[cfg(desktop)]
pub mod native;
pub mod presets;
mod proxy;
#[cfg_attr(desktop, allow(dead_code))]
// the page fallback is wired on mobile only; built on desktop so its tests run in CI
pub mod routing;
pub mod state;
pub mod track_info;

pub mod cache;

pub mod autoplay;

mod lyrics_cache;

pub mod lyric_prefs;

pub mod lyric_sources;
