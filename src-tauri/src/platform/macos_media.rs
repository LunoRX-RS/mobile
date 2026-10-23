//! System Now Playing (Control Center / menu bar) and hardware media keys via MPNowPlayingInfoCenter and
//! MPRemoteCommandCenter, through objc2 bindings. souvlaki was rejected: it has no stop command, never sets the playback rate and loads artwork from a URL (network code).
//!
//! Threads: command handlers are registered on the main thread (called from `setup`). The OS invokes
//! them on its own thread; they only spawn an async task and return. A single publisher task wakes on
//! `poke()`, reads core progress, and posts the AppKit calls to the main thread.
//!
//! Artist comes from the track's artists. Artwork is fetched by the publisher task from the track's
//! `artwork_url` (https only, short timeouts, size cap, no redirects), cached for the current URL, and
//! pushed once it arrives; until then (or on failure) the widget shows no cover.

use std::ptr::NonNull;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use block2::RcBlock;
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_app_kit::{NSApplication, NSImage};
use objc2_core_foundation::CGSize;
use objc2_foundation::{MainThreadMarker, NSData, NSDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPMediaItemArtwork, MPMediaItemPropertyArtist,
    MPMediaItemPropertyArtwork, MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState, MPRemoteCommand,
    MPRemoteCommandCenter, MPRemoteCommandEvent, MPRemoteCommandHandlerStatus,
};
use tauri::{AppHandle, Manager};
use tokio::sync::Notify;

use super::control::{self, Remote};
use super::menu::{self, PlaybackMenu};
use crate::commands::player::{PlaybackStateDto, PlayerProgressDto, PlayerState, progress};

static CHANGED: Notify = Notify::const_new();

pub fn poke() {
    CHANGED.notify_one();
}

/// Cover bytes for the URL in `ARTWORK.0`; written by the fetch task, read by the publisher.
static ARTWORK: Mutex<(Option<String>, Option<Vec<u8>>)> = Mutex::new((None, None));
const ARTWORK_MAX_BYTES: usize = 5 * 1024 * 1024;

/// A seek or stall shows up as elapsed time departing from the extrapolated value by more than this.
const DRIFT_MS: u64 = 1_500;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NowPlaying {
    pub title: String,
    /// Artists joined with ", "; empty when unknown.
    pub artist: String,
    pub artwork_url: Option<String>,
    /// True once cover bytes for `artwork_url` are in hand; flips the snapshot so the cover is pushed.
    pub artwork_ready: bool,
    pub duration_ms: u64,
    pub elapsed_ms: u64,
    /// True only in `Playing`; buffering and paused report rate 0.0.
    pub playing: bool,
}

/// Pure: core progress to Now Playing info. `None` clears the widget.
pub fn map(dto: &PlayerProgressDto) -> Option<NowPlaying> {
    use PlaybackStateDto as S;
    let track = dto.track.as_ref()?;
    if !matches!(
        dto.player.state,
        S::Resolving | S::Preparing | S::Ready | S::Playing | S::Paused | S::Buffering
    ) {
        return None;
    }
    let elapsed = if dto.duration_ms > 0 {
        dto.position_ms.min(dto.duration_ms)
    } else {
        dto.position_ms
    };
    Some(NowPlaying {
        title: track.title.clone(),
        artist: track.artists.join(", "),
        artwork_url: track.artwork_url.clone(),
        artwork_ready: false,
        duration_ms: dto.duration_ms,
        elapsed_ms: elapsed,
        playing: dto.player.state == S::Playing,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Controls {
    pub play: bool,
    pub pause: bool,
    pub toggle: bool,
    pub stop: bool,
    pub seek: bool,
    pub skip: bool,
}

pub fn controls(state: PlaybackStateDto) -> Controls {
    use PlaybackStateDto as S;
    let can_play = matches!(state, S::Paused | S::Ready);
    let can_pause = matches!(state, S::Playing | S::Buffering);
    Controls {
        play: can_play,
        pause: can_pause,
        toggle: can_play || can_pause,
        stop: state != S::Idle,
        seek: can_play || can_pause,
        skip: can_play || can_pause,
    }
}

/// Decides whether a new Now Playing snapshot must be pushed. Normal playback is extrapolated by the OS
/// from the rate, so it is not re-pushed; only metadata, state or position discontinuities are.
#[derive(Default)]
pub struct Throttle {
    last: Option<(Option<NowPlaying>, Instant)>,
}

impl Throttle {
    pub fn accept(&mut self, now: &Option<NowPlaying>, at: Instant) -> bool {
        let send = match (&self.last, now) {
            (None, _) => true,
            (Some((None, _)), None) => false,
            (Some((Some(_), _)), None) | (Some((None, _)), Some(_)) => true,
            (Some((Some(old), sent_at)), Some(new)) => {
                old.title != new.title
                    || old.artist != new.artist
                    || old.artwork_url != new.artwork_url
                    || old.artwork_ready != new.artwork_ready
                    || old.duration_ms != new.duration_ms
                    || old.playing != new.playing
                    || if new.playing {
                        let waited = at.saturating_duration_since(*sent_at).as_millis() as u64;
                        (old.elapsed_ms + waited).abs_diff(new.elapsed_ms) > DRIFT_MS
                    } else {
                        old.elapsed_ms != new.elapsed_ms
                    }
            }
        };
        if send {
            self.last = Some((now.clone(), at));
        }
        send
    }
}

/// An unbundled dev run (`tauri dev`) has no .app, so macOS shows a blank grid as the app icon, including as the
/// badge on the Now Playing widget. Setting the icon at runtime fixes that; a bundled app already has the same icon.
/// Main thread only.
pub fn set_app_icon() {
    static ICON: &[u8] = include_bytes!("../../icons/icon.png");
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(ICON)) {
        // SAFETY: plain AppKit call on the main thread with a valid image.
        unsafe { NSApplication::sharedApplication(mtm).setApplicationIconImage(Some(&image)) };
    }
}

fn artwork_slot() -> std::sync::MutexGuard<'static, (Option<String>, Option<Vec<u8>>)> {
    ARTWORK.lock().unwrap_or_else(|e| e.into_inner())
}

/// https only, short timeouts, no redirects, size cap. `None` on any failure.
async fn download_artwork(url: &str) -> Option<Vec<u8>> {
    if !url.starts_with("https://") {
        return None;
    }
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(3))
        .timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let mut resp = client.get(url).send().await.ok()?.error_for_status().ok()?;
    let mut bytes = Vec::new();
    while let Some(chunk) = resp.chunk().await.ok()? {
        if bytes.len() + chunk.len() > ARTWORK_MAX_BYTES {
            return None;
        }
        bytes.extend_from_slice(&chunk);
    }
    Some(bytes)
}

async fn fetch_artwork(url: String) {
    let bytes = download_artwork(&url).await;
    {
        let mut slot = artwork_slot();
        // The track may have changed while downloading; only keep bytes for the current URL.
        if slot.0.as_deref() == Some(url.as_str()) {
            slot.1 = bytes;
        }
    }
    poke();
}

/// Starts a fetch when the cover URL changed and marks `artwork_ready` once bytes are cached.
/// A failed fetch is not retried until the URL changes.
fn track_artwork(info: &mut Option<NowPlaying>) {
    let url = info.as_ref().and_then(|i| i.artwork_url.clone());
    let mut slot = artwork_slot();
    if slot.0 != url {
        *slot = (url.clone(), None);
        if let Some(url) = url {
            tauri::async_runtime::spawn(fetch_artwork(url));
        }
    }
    if let Some(i) = info {
        i.artwork_ready = slot.1.is_some();
    }
}

fn as_object<T: objc2::Message>(v: Retained<T>) -> Retained<AnyObject> {
    // SAFETY: every Objective-C object is an AnyObject; only the static type is widened.
    unsafe { Retained::cast_unchecked(v) }
}

/// Cover as `MPMediaItemArtwork`, or `None` when the bytes are not a decodable image.
///
/// Main thread only.
fn artwork(bytes: &[u8]) -> Option<Retained<AnyObject>> {
    let image = NSImage::initWithData(NSImage::alloc(), &NSData::with_bytes(bytes))?;
    let size = image.size();
    let size = CGSize::new(size.width, size.height);
    // The handler may run on any thread; it only returns the retained image, which AppKit allows.
    let block = RcBlock::new(move |_: CGSize| NonNull::from(&*image));
    // SAFETY: the handler returns a pointer to `image`, which the block keeps alive.
    let art = unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            size,
            &block,
        )
    };
    Some(as_object(art))
}

/// Main thread only.
fn publish(info: &Option<NowPlaying>, cover: Option<&[u8]>) {
    // SAFETY: plain Objective-C calls on the shared MediaPlayer singletons with valid, retained arguments.
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        let Some(info) = info else {
            center.setNowPlayingInfo(None);
            center.setPlaybackState(MPNowPlayingPlaybackState::Stopped);
            return;
        };
        let mut keys: Vec<&NSString> = Vec::new();
        let mut values: Vec<Retained<AnyObject>> = Vec::new();
        if !info.title.is_empty() {
            keys.push(MPMediaItemPropertyTitle);
            values.push(as_object(NSString::from_str(&info.title)));
        }
        if !info.artist.is_empty() {
            keys.push(MPMediaItemPropertyArtist);
            values.push(as_object(NSString::from_str(&info.artist)));
        }
        if let Some(art) = cover.and_then(artwork) {
            keys.push(MPMediaItemPropertyArtwork);
            values.push(art);
        }
        if info.duration_ms > 0 {
            keys.push(MPMediaItemPropertyPlaybackDuration);
            values.push(as_object(NSNumber::new_f64(
                info.duration_ms as f64 / 1000.0,
            )));
        }
        keys.push(MPNowPlayingInfoPropertyElapsedPlaybackTime);
        values.push(as_object(NSNumber::new_f64(
            info.elapsed_ms as f64 / 1000.0,
        )));
        keys.push(MPNowPlayingInfoPropertyPlaybackRate);
        values.push(as_object(NSNumber::new_f64(if info.playing {
            1.0
        } else {
            0.0
        })));
        let dict: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_retained_objects(&keys, &values);
        center.setNowPlayingInfo(Some(&dict));
        center.setPlaybackState(if info.playing {
            MPNowPlayingPlaybackState::Playing
        } else {
            MPNowPlayingPlaybackState::Paused
        });
    }
}

/// Main thread only.
fn publish_controls(c: Controls) {
    // SAFETY: plain Objective-C calls on the shared command center.
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        center.playCommand().setEnabled(c.play);
        center.pauseCommand().setEnabled(c.pause);
        center.togglePlayPauseCommand().setEnabled(c.toggle);
        center.stopCommand().setEnabled(c.stop);
        center.changePlaybackPositionCommand().setEnabled(c.seek);
        center.nextTrackCommand().setEnabled(c.skip);
        center.previousTrackCommand().setEnabled(c.skip);
    }
}

fn spawn_command(app: &AppHandle, cmd: Remote) -> MPRemoteCommandHandlerStatus {
    tauri::async_runtime::spawn(control::run(app.clone(), cmd));
    MPRemoteCommandHandlerStatus::Success
}

/// Main thread only. The OS retains the handler block; the returned token is intentionally leaked because
/// these handlers live for the whole process (ponytail: no unregister path needed).
fn register(command: &MPRemoteCommand, app: &AppHandle, cmd: Remote) {
    let app = app.clone();
    let block = RcBlock::new(move |_event: NonNull<MPRemoteCommandEvent>| spawn_command(&app, cmd));
    // SAFETY: the block matches the handler signature and is copied by the command.
    let token = unsafe { command.addTargetWithHandler(&block) };
    std::mem::forget(token);
}

fn register_all(app: &AppHandle) {
    // SAFETY: shared command center singleton; handlers are Send-safe (they only spawn a task).
    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();
        register(&center.playCommand(), app, Remote::Play);
        register(&center.pauseCommand(), app, Remote::Pause);
        register(&center.togglePlayPauseCommand(), app, Remote::Toggle);
        register(&center.stopCommand(), app, Remote::Stop);
        register(&center.nextTrackCommand(), app, Remote::Next);
        register(&center.previousTrackCommand(), app, Remote::Previous);

        let handler_app = app.clone();
        let block = RcBlock::new(move |event: NonNull<MPRemoteCommandEvent>| {
            // SAFETY: this handler is only attached to changePlaybackPositionCommand, whose events are
            // MPChangePlaybackPositionCommandEvent.
            let seconds = event
                .cast::<MPChangePlaybackPositionCommandEvent>()
                .as_ref()
                .positionTime();
            if seconds.is_finite() && seconds >= 0.0 {
                spawn_command(&handler_app, Remote::SeekTo((seconds * 1000.0) as i64))
            } else {
                MPRemoteCommandHandlerStatus::CommandFailed
            }
        });
        let token = center
            .changePlaybackPositionCommand()
            .addTargetWithHandler(&block);
        std::mem::forget(token);
    }
    publish_controls(controls(PlaybackStateDto::Idle));
}

/// Registers media keys and starts the publisher. Must run on the main thread (Tauri `setup`).
/// Never fails the app and never retries.
pub fn install(app: &AppHandle, menu: Option<PlaybackMenu>) {
    register_all(app);
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut throttle = Throttle::default();
        let mut last_controls = None;
        let mut last_menu = None;
        loop {
            CHANGED.notified().await;
            let dto = progress(&app.state::<PlayerState>()).await;
            let mut info = map(&dto);
            track_artwork(&mut info);
            let cover = artwork_slot().1.clone().filter(|_| info.is_some());
            let ctl = controls(dto.player.state);
            let send_info = throttle.accept(&info, Instant::now());
            let send_ctl = last_controls != Some(ctl);
            last_controls = Some(ctl);
            if send_info || send_ctl {
                // AppKit calls belong on the main thread.
                if app
                    .run_on_main_thread(move || {
                        if send_info {
                            publish(&info, cover.as_deref());
                        }
                        if send_ctl {
                            publish_controls(ctl);
                        }
                    })
                    .is_err()
                {
                    eprintln!("now playing update not delivered");
                }
            }
            if let Some(menu) = &menu {
                let model = menu::model(dto.player.state, dto.volume);
                if last_menu != Some(model) {
                    last_menu = Some(model);
                    menu.apply(&model);
                }
            }
        }
    });
    // Publish the initial empty state (clears stale info from a previous run).
    poke();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::player::PlayerStateDto;
    use std::time::Duration;

    fn dto(state: PlaybackStateDto, pos: u64, dur: u64, titled: bool) -> PlayerProgressDto {
        PlayerProgressDto::for_test(
            PlayerStateDto {
                version: 1,
                state,
                revision: 1,
            },
            titled.then(|| "Song".to_string()),
            pos,
            dur,
        )
    }

    fn np(elapsed: u64, playing: bool) -> Option<NowPlaying> {
        Some(NowPlaying {
            title: "Song".into(),
            artist: String::new(),
            artwork_url: None,
            artwork_ready: false,
            duration_ms: 200_000,
            elapsed_ms: elapsed,
            playing,
        })
    }

    #[test]
    fn maps_playing_with_rate_flag_and_elapsed() {
        let m = map(&dto(PlaybackStateDto::Playing, 12_000, 200_000, true)).unwrap();
        assert_eq!(
            m,
            NowPlaying {
                title: "Song".into(),
                artist: String::new(),
                artwork_url: None,
                artwork_ready: false,
                duration_ms: 200_000,
                elapsed_ms: 12_000,
                playing: true
            }
        );
    }

    #[test]
    fn paused_and_buffering_are_not_playing() {
        assert!(
            !map(&dto(PlaybackStateDto::Paused, 1, 10, true))
                .unwrap()
                .playing
        );
        assert!(
            !map(&dto(PlaybackStateDto::Buffering, 1, 10, true))
                .unwrap()
                .playing
        );
    }

    #[test]
    fn nothing_loaded_clears() {
        assert_eq!(map(&dto(PlaybackStateDto::Idle, 0, 0, false)), None);
        assert_eq!(map(&dto(PlaybackStateDto::Playing, 0, 0, false)), None);
        for s in [
            PlaybackStateDto::Idle,
            PlaybackStateDto::Ended,
            PlaybackStateDto::Error,
            PlaybackStateDto::Blocked,
        ] {
            assert_eq!(map(&dto(s, 5, 10, true)), None, "{s:?}");
        }
    }

    #[test]
    fn elapsed_is_clamped_to_duration() {
        assert_eq!(
            map(&dto(PlaybackStateDto::Playing, 999, 100, true))
                .unwrap()
                .elapsed_ms,
            100
        );
    }

    #[test]
    fn controls_follow_state() {
        let c = controls(PlaybackStateDto::Playing);
        assert!(c.pause && c.toggle && c.seek && c.stop && !c.play);
        let c = controls(PlaybackStateDto::Paused);
        assert!(c.play && c.toggle && c.seek && !c.pause);
        let c = controls(PlaybackStateDto::Idle);
        assert!(!c.play && !c.pause && !c.toggle && !c.stop && !c.seek);
    }

    #[test]
    fn throttle_sends_first_then_skips_steady_playback() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        assert!(th.accept(&np(0, true), t0));
        // Natural advance matches the extrapolation: no push.
        assert!(!th.accept(&np(250, true), t0 + Duration::from_millis(250)));
        assert!(!th.accept(&np(1_000, true), t0 + Duration::from_millis(1_000)));
    }

    #[test]
    fn throttle_sends_on_state_seek_and_clear() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        assert!(th.accept(&np(0, true), t0));
        assert!(
            th.accept(&np(300, false), t0 + Duration::from_millis(300)),
            "pause"
        );
        assert!(
            !th.accept(&np(300, false), t0 + Duration::from_millis(400)),
            "unchanged pause"
        );
        assert!(
            th.accept(&np(90_000, false), t0 + Duration::from_millis(500)),
            "seek while paused"
        );
        assert!(
            th.accept(&np(90_000, true), t0 + Duration::from_millis(600)),
            "resume"
        );
        assert!(
            th.accept(&np(30_000, true), t0 + Duration::from_millis(700)),
            "seek while playing"
        );
        assert!(th.accept(&None, t0 + Duration::from_millis(800)), "clear");
        assert!(
            !th.accept(&None, t0 + Duration::from_millis(900)),
            "already clear"
        );
    }

    #[test]
    fn throttle_sends_on_track_change() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        assert!(th.accept(&np(0, true), t0));
        let mut other = np(0, true);
        other.as_mut().unwrap().title = "Other".into();
        assert!(th.accept(&other, t0 + Duration::from_millis(10)));
    }

    #[test]
    fn throttle_sends_when_artist_or_artwork_changes() {
        let t0 = Instant::now();
        let mut th = Throttle::default();
        assert!(th.accept(&np(0, true), t0));
        let mut with_artist = np(0, true);
        with_artist.as_mut().unwrap().artist = "A, B".into();
        assert!(th.accept(&with_artist, t0 + Duration::from_millis(10)));
        let mut with_url = with_artist.clone();
        with_url.as_mut().unwrap().artwork_url = Some("https://x/c.jpg".into());
        assert!(th.accept(&with_url, t0 + Duration::from_millis(20)));
        let mut ready = with_url.clone();
        ready.as_mut().unwrap().artwork_ready = true;
        assert!(
            th.accept(&ready, t0 + Duration::from_millis(30)),
            "cover arrived"
        );
        assert!(!th.accept(&ready, t0 + Duration::from_millis(40)));
    }

    #[tokio::test]
    async fn artwork_download_rejects_non_https() {
        assert_eq!(download_artwork("http://example.com/c.jpg").await, None);
        assert_eq!(download_artwork("file:///etc/passwd").await, None);
    }
}
