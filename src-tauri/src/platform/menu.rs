//! macOS "Playback" menu. Built on top of Tauri's default menu (app, File, Edit, View, Window, Help), so
//! About, Quit and Edit's copy/paste keep working; Playback is inserted before Window. Menu events are
//! handled in Rust and call the same core functions as the player commands, so they work with the window
//! hidden or the webview unfocused.

use tauri::menu::{Menu, MenuItem, MenuItemBuilder, MenuItemKind, SubmenuBuilder};
use tauri::{AppHandle, Wry};

use super::control::{self, Remote};
use crate::commands::player::PlaybackStateDto;

const ID_TOGGLE: &str = "playback_toggle";
const ID_STOP: &str = "playback_stop";
const ID_PREVIOUS: &str = "playback_previous";
const ID_NEXT: &str = "playback_next";
const ID_BACK: &str = "playback_seek_back";
const ID_FORWARD: &str = "playback_seek_forward";
const ID_VOL_UP: &str = "playback_volume_up";
const ID_VOL_DOWN: &str = "playback_volume_down";
const ID_MUTE: &str = "playback_mute";

/// What the Playback menu should show. Pure so the mapping is testable without AppKit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MenuModel {
    pub toggle_label: &'static str,
    pub toggle_enabled: bool,
    pub stop_enabled: bool,
    pub seek_enabled: bool,
    pub skip_enabled: bool,
    pub volume_enabled: bool,
    pub mute_label: &'static str,
}

pub fn model(state: PlaybackStateDto, volume: u8) -> MenuModel {
    use PlaybackStateDto as S;
    let loaded = matches!(state, S::Ready | S::Playing | S::Paused | S::Buffering);
    MenuModel {
        toggle_label: if matches!(state, S::Playing | S::Buffering) {
            "Pause"
        } else {
            "Play"
        },
        toggle_enabled: loaded,
        stop_enabled: state != S::Idle,
        seek_enabled: loaded,
        skip_enabled: loaded,
        volume_enabled: loaded,
        mute_label: if volume == 0 { "Unmute" } else { "Mute" },
    }
}

pub struct PlaybackMenu {
    toggle: MenuItem<Wry>,
    stop: MenuItem<Wry>,
    previous: MenuItem<Wry>,
    next: MenuItem<Wry>,
    back: MenuItem<Wry>,
    forward: MenuItem<Wry>,
    vol_up: MenuItem<Wry>,
    vol_down: MenuItem<Wry>,
    mute: MenuItem<Wry>,
}

impl PlaybackMenu {
    pub fn apply(&self, m: &MenuModel) {
        // Failures only mean the menu is momentarily stale; the next state change retries.
        let _ = self.toggle.set_text(m.toggle_label);
        let _ = self.toggle.set_enabled(m.toggle_enabled);
        let _ = self.stop.set_enabled(m.stop_enabled);
        for item in [&self.back, &self.forward] {
            let _ = item.set_enabled(m.seek_enabled);
        }
        for item in [&self.previous, &self.next] {
            let _ = item.set_enabled(m.skip_enabled);
        }
        for item in [&self.vol_up, &self.vol_down, &self.mute] {
            let _ = item.set_enabled(m.volume_enabled);
        }
        let _ = self.mute.set_text(m.mute_label);
    }
}

fn item(app: &AppHandle, id: &str, text: &str, accel: &str) -> tauri::Result<MenuItem<Wry>> {
    MenuItemBuilder::with_id(id, text)
        .accelerator(accel)
        .enabled(false)
        .build(app)
}

fn build(app: &AppHandle) -> tauri::Result<PlaybackMenu> {
    let menu = PlaybackMenu {
        toggle: item(app, ID_TOGGLE, "Play", "CmdOrCtrl+P")?,
        stop: item(app, ID_STOP, "Stop", "CmdOrCtrl+.")?,
        previous: item(app, ID_PREVIOUS, "Previous Track", "CmdOrCtrl+Alt+Left")?,
        next: item(app, ID_NEXT, "Next Track", "CmdOrCtrl+Alt+Right")?,
        back: item(app, ID_BACK, "Seek Back 10 Seconds", "CmdOrCtrl+Left")?,
        forward: item(
            app,
            ID_FORWARD,
            "Seek Forward 10 Seconds",
            "CmdOrCtrl+Right",
        )?,
        vol_up: item(app, ID_VOL_UP, "Volume Up", "CmdOrCtrl+Up")?,
        vol_down: item(app, ID_VOL_DOWN, "Volume Down", "CmdOrCtrl+Down")?,
        mute: item(app, ID_MUTE, "Mute", "CmdOrCtrl+Alt+Down")?,
    };
    let playback = SubmenuBuilder::new(app, "Playback")
        .item(&menu.toggle)
        .item(&menu.stop)
        .separator()
        .item(&menu.previous)
        .item(&menu.next)
        .separator()
        .item(&menu.back)
        .item(&menu.forward)
        .separator()
        .item(&menu.vol_up)
        .item(&menu.vol_down)
        .item(&menu.mute)
        .build()?;
    let root = Menu::default(app)?;
    let before_window = root
        .items()?
        .iter()
        .position(
            |i| matches!(i, MenuItemKind::Submenu(s) if s.text().is_ok_and(|t| t == "Window")),
        )
        .unwrap_or(root.items()?.len());
    root.insert(&playback, before_window)?;
    app.set_menu(root)?;
    Ok(menu)
}

fn command_for(id: &str) -> Option<Remote> {
    Some(match id {
        ID_TOGGLE => Remote::Toggle,
        ID_STOP => Remote::Stop,
        ID_PREVIOUS => Remote::Previous,
        ID_NEXT => Remote::Next,
        ID_BACK => Remote::SEEK_BACK,
        ID_FORWARD => Remote::SEEK_FORWARD,
        ID_VOL_UP => Remote::VOLUME_UP,
        ID_VOL_DOWN => Remote::VOLUME_DOWN,
        ID_MUTE => Remote::ToggleMute,
        _ => return None,
    })
}

/// Installs the menu and its event handler. On failure logs one line and leaves Tauri's default menu.
pub fn install(app: &AppHandle) -> Option<PlaybackMenu> {
    match build(app) {
        Ok(menu) => {
            app.on_menu_event(|app, event| {
                if let Some(cmd) = command_for(event.id().as_ref()) {
                    tauri::async_runtime::spawn(control::run(app.clone(), cmd));
                }
            });
            Some(menu)
        }
        Err(_) => {
            eprintln!("playback menu unavailable");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlaybackStateDto as S;

    #[test]
    fn toggle_label_follows_state() {
        assert_eq!(model(S::Playing, 50).toggle_label, "Pause");
        assert_eq!(model(S::Buffering, 50).toggle_label, "Pause");
        assert_eq!(model(S::Paused, 50).toggle_label, "Play");
        assert_eq!(model(S::Idle, 50).toggle_label, "Play");
    }

    #[test]
    fn nothing_loaded_disables_everything_but_labels() {
        for s in [
            S::Idle,
            S::Resolving,
            S::Preparing,
            S::Ended,
            S::Error,
            S::Blocked,
        ] {
            let m = model(s, 50);
            assert!(
                !m.toggle_enabled && !m.seek_enabled && !m.skip_enabled && !m.volume_enabled,
                "{s:?}"
            );
        }
        assert!(!model(S::Idle, 50).stop_enabled);
    }

    #[test]
    fn loaded_enables_transport() {
        for s in [S::Ready, S::Playing, S::Paused, S::Buffering] {
            let m = model(s, 50);
            assert!(
                m.toggle_enabled
                    && m.stop_enabled
                    && m.seek_enabled
                    && m.skip_enabled
                    && m.volume_enabled
            );
        }
        // A stuck Error/Ended session can still be stopped back to Idle.
        assert!(model(S::Ended, 50).stop_enabled);
    }

    #[test]
    fn mute_label_follows_volume() {
        assert_eq!(model(S::Playing, 0).mute_label, "Unmute");
        assert_eq!(model(S::Playing, 1).mute_label, "Mute");
    }

    #[test]
    fn every_menu_id_maps_to_a_command() {
        for id in [
            ID_TOGGLE,
            ID_STOP,
            ID_PREVIOUS,
            ID_NEXT,
            ID_BACK,
            ID_FORWARD,
            ID_VOL_UP,
            ID_VOL_DOWN,
            ID_MUTE,
        ] {
            assert!(command_for(id).is_some(), "{id}");
        }
        assert_eq!(command_for("unknown"), None);
    }
}
