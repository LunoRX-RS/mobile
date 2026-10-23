//! Maps OS-level commands (media keys, Now Playing widget, Playback menu) onto the same core functions the
//! Tauri player commands use: `dispatch`, `run_seek` and `run_set_volume` validate and run under the
//! engine lock, exactly as `player_*` do. No webview involved.

use std::sync::atomic::{AtomicU8, Ordering};

use tauri::{AppHandle, Emitter, Manager};

use crate::commands::player::{
    PlaybackStateDto, PlayerState, dispatch, finish, progress, run_seek, run_set_volume,
};
use crate::core::playback::command::PlaybackCommand;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // step keys and the Playback menu exist on macOS only
pub const VOLUME_STEP: i8 = 5;
#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // step keys and the Playback menu exist on macOS only
const SEEK_STEP_MS: i64 = 10_000;
const RESTORE_VOLUME: u8 = 100;
const RESTART_AFTER_MS: u64 = 3_000;

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // step keys and the Playback menu exist on macOS only
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Remote {
    Play,
    Pause,
    Toggle,
    Stop,
    SeekTo(i64),
    SeekBy(i64),
    VolumeBy(i8),
    ToggleMute,
    Next,
    Previous,
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))] // step keys and the Playback menu exist on macOS only
impl Remote {
    pub const SEEK_BACK: Remote = Remote::SeekBy(-SEEK_STEP_MS);
    pub const SEEK_FORWARD: Remote = Remote::SeekBy(SEEK_STEP_MS);
    pub const VOLUME_UP: Remote = Remote::VolumeBy(VOLUME_STEP);
    pub const VOLUME_DOWN: Remote = Remote::VolumeBy(-VOLUME_STEP);
}

#[derive(Debug, PartialEq)]
enum Plan {
    Core(PlaybackCommand),
    Seek(i64),
    Volume(u8),
    Skip(bool),
    Nothing,
}

pub fn step_volume(current: u8, delta: i8) -> u8 {
    (i16::from(current) + i16::from(delta)).clamp(0, 100) as u8
}

/// Returns (new volume, remembered volume). Muting remembers the level; unmuting restores it.
fn mute_toggle(current: u8, remembered: u8) -> (u8, u8) {
    if current > 0 {
        (0, current)
    } else if remembered > 0 {
        (remembered, 0)
    } else {
        (RESTORE_VOLUME, 0)
    }
}

static MUTED_FROM: AtomicU8 = AtomicU8::new(0);

/// Pure: what to ask core for, plus the remembered pre-mute volume to store if that succeeds.
fn plan(
    cmd: Remote,
    state: PlaybackStateDto,
    position_ms: u64,
    duration_ms: u64,
    volume: u8,
    remembered: u8,
) -> (Plan, u8) {
    use PlaybackStateDto as S;
    let plan = match cmd {
        Remote::Play if state == S::Paused => Plan::Core(PlaybackCommand::Resume),
        Remote::Play => Plan::Core(PlaybackCommand::Play),
        Remote::Pause => Plan::Core(PlaybackCommand::Pause),
        Remote::Toggle => match state {
            S::Playing | S::Buffering => Plan::Core(PlaybackCommand::Pause),
            S::Paused => Plan::Core(PlaybackCommand::Resume),
            S::Ready => Plan::Core(PlaybackCommand::Play),
            _ => Plan::Nothing,
        },
        Remote::Stop => Plan::Core(PlaybackCommand::Stop),
        Remote::Next => Plan::Skip(true),
        // Standard player behaviour: restart the track unless it has only just begun.
        Remote::Previous if position_ms > RESTART_AFTER_MS => Plan::Seek(0),
        Remote::Previous => Plan::Skip(false),
        Remote::SeekTo(ms) => Plan::Seek(ms),
        Remote::SeekBy(delta) => {
            let mut target = (position_ms as i64).saturating_add(delta).max(0);
            if duration_ms > 0 {
                target = target.min(duration_ms as i64);
            }
            Plan::Seek(target)
        }
        Remote::VolumeBy(delta) => Plan::Volume(step_volume(volume, delta)),
        Remote::ToggleMute => {
            let (v, saved) = mute_toggle(volume, remembered);
            return (Plan::Volume(v), saved);
        }
    };
    (plan, remembered)
}

/// Runs one OS command. Core errors are dropped: the OS widget has nowhere to show them, and the engine
/// state is unchanged when it refuses.
pub async fn run(app: AppHandle, cmd: Remote) {
    let state = app.state::<PlayerState>();
    let now = progress(&state).await;
    let (plan, remembered) = plan(
        cmd,
        now.player.state,
        now.position_ms,
        now.duration_ms,
        now.volume,
        MUTED_FROM.load(Ordering::Relaxed),
    );
    let reply = match plan {
        Plan::Core(c) => dispatch(&state, c).await,
        Plan::Seek(ms) => run_seek(&state, ms).await,
        Plan::Volume(v) => {
            let reply = run_set_volume(&state, i64::from(v)).await;
            if reply.result.is_ok() {
                MUTED_FROM.store(remembered, Ordering::Relaxed);
            }
            reply
        }
        Plan::Skip(forward) => {
            crate::commands::player::skip(&app, forward).await;
            let dto = progress(&state).await;
            let _ = app.emit_to("main", "core://player/progress", dto);
            return;
        }
        Plan::Nothing => return,
    };
    let _ = finish(&app, reply);
    // `core://player/state` only fires when the state changes; seek and volume do not, so push progress too.
    let dto = progress(&state).await;
    let _ = app.emit_to("main", "core://player/progress", dto);
}

#[cfg(test)]
mod tests {
    use super::*;
    use PlaybackStateDto as S;

    fn p(cmd: Remote, state: S) -> Plan {
        plan(cmd, state, 30_000, 200_000, 50, 0).0
    }

    #[test]
    fn previous_restarts_after_three_seconds() {
        assert_eq!(p(Remote::Previous, S::Playing), Plan::Seek(0));
        assert_eq!(
            plan(Remote::Previous, S::Playing, 1_000, 200_000, 50, 0).0,
            Plan::Skip(false)
        );
        assert_eq!(p(Remote::Next, S::Playing), Plan::Skip(true));
    }

    #[test]
    fn volume_step_clamps() {
        assert_eq!(step_volume(98, 5), 100);
        assert_eq!(step_volume(100, 5), 100);
        assert_eq!(step_volume(3, -5), 0);
        assert_eq!(step_volume(50, 5), 55);
        assert_eq!(step_volume(50, -5), 45);
    }

    #[test]
    fn mute_remembers_and_restores() {
        assert_eq!(mute_toggle(40, 0), (0, 40));
        assert_eq!(mute_toggle(0, 40), (40, 0));
        assert_eq!(mute_toggle(0, 0), (RESTORE_VOLUME, 0));
    }

    #[test]
    fn toggle_follows_state() {
        assert_eq!(
            p(Remote::Toggle, S::Playing),
            Plan::Core(PlaybackCommand::Pause)
        );
        assert_eq!(
            p(Remote::Toggle, S::Buffering),
            Plan::Core(PlaybackCommand::Pause)
        );
        assert_eq!(
            p(Remote::Toggle, S::Paused),
            Plan::Core(PlaybackCommand::Resume)
        );
        assert_eq!(
            p(Remote::Toggle, S::Ready),
            Plan::Core(PlaybackCommand::Play)
        );
        assert_eq!(p(Remote::Toggle, S::Idle), Plan::Nothing);
        assert_eq!(p(Remote::Toggle, S::Ended), Plan::Nothing);
    }

    #[test]
    fn play_resumes_when_paused() {
        assert_eq!(
            p(Remote::Play, S::Paused),
            Plan::Core(PlaybackCommand::Resume)
        );
        assert_eq!(p(Remote::Play, S::Ready), Plan::Core(PlaybackCommand::Play));
    }

    #[test]
    fn seek_by_clamps_to_track() {
        assert_eq!(
            plan(Remote::SEEK_BACK, S::Playing, 4_000, 200_000, 50, 0).0,
            Plan::Seek(0)
        );
        assert_eq!(
            plan(Remote::SEEK_FORWARD, S::Playing, 195_000, 200_000, 50, 0).0,
            Plan::Seek(200_000)
        );
        assert_eq!(
            plan(Remote::SEEK_FORWARD, S::Playing, 1_000, 0, 50, 0).0,
            Plan::Seek(11_000)
        );
    }

    #[test]
    fn volume_and_mute_plans() {
        assert_eq!(
            plan(Remote::VOLUME_UP, S::Playing, 0, 0, 98, 0).0,
            Plan::Volume(100)
        );
        assert_eq!(
            plan(Remote::VOLUME_DOWN, S::Playing, 0, 0, 2, 0).0,
            Plan::Volume(0)
        );
        assert_eq!(
            plan(Remote::ToggleMute, S::Playing, 0, 0, 60, 0),
            (Plan::Volume(0), 60)
        );
        assert_eq!(
            plan(Remote::ToggleMute, S::Playing, 0, 0, 0, 60),
            (Plan::Volume(60), 0)
        );
    }
}
