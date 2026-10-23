//! Discord Rich Presence: shows the playing track on the user's Discord profile, through the Discord app that is
//! already running on this computer. Local IPC only (a Unix socket, or a named pipe on Windows); this code opens no
//! network connection and needs no secret. Off until the user turns it on and gives their own Application ID.
//!
//! What leaves the app: the track title, artists, cover URL, a YouTube link and the playback time, handed to the local
//! Discord app, which then shows them to people who can see the user's profile.
use crate::commands::player::{PlaybackStateDto, PlayerProgressDto};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    path::PathBuf,
    sync::{Mutex, OnceLock, mpsc},
    time::{Duration, Instant},
};

/// LunoRX's own Discord application. An Application ID is public (it appears in every Rich Presence client), not a secret.
pub const DEFAULT_CLIENT_ID: &str = "1562868203460493373";
/// The name of the logo in the Discord application's Art Assets (Developer Portal > Rich Presence).
/// Used as the small corner badge, and as the big picture when a track has no cover.
const LOGO_ASSET: &str = "logo";
const REPO_URL: &str = "https://github.com/LunoRX-RS/desktop";
const SETTINGS_FILE: &str = "discord-presence.json";
const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const MIN_GAP: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(20);
const DRIFT_MS: i64 = 3_000;

#[derive(Clone, Serialize, Deserialize, Default, PartialEq, Eq, Debug)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub enabled: bool,
    /// An Application ID of the user's own, replacing the default. Empty uses `DEFAULT_CLIENT_ID`.
    pub client_id: String,
}

impl Settings {
    /// The Application ID to connect with: the user's own, else the built-in one.
    fn effective_client_id(&self) -> &str {
        if self.client_id.is_empty() {
            DEFAULT_CLIENT_ID
        } else {
            &self.client_id
        }
    }
}

/// 17 to 20 digits, the shape of a Discord snowflake.
pub fn valid_client_id(id: &str) -> bool {
    (17..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit())
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Off,
    Connecting,
    Connected,
    /// Enabled, but Discord is not running or refused the Application ID.
    Unavailable,
}

#[derive(Clone, PartialEq, Debug)]
enum Play {
    Playing,
    Paused,
}

#[derive(Clone, Debug)]
struct Snapshot {
    track_id: String,
    title: String,
    artists: Vec<String>,
    artwork: Option<String>,
    play: Play,
    position_ms: u64,
    duration_ms: u64,
}

enum Msg {
    Settings(Settings),
    Track(Option<Snapshot>),
}

struct Shared {
    tx: mpsc::Sender<Msg>,
    status: Mutex<Status>,
    settings: Mutex<Settings>,
    path: Option<PathBuf>,
}
static SHARED: OnceLock<Shared> = OnceLock::new();

// ---- wire format -------------------------------------------------------------------------------------------------

fn frame(op: u32, payload: &Value) -> Vec<u8> {
    let body = payload.to_string().into_bytes();
    let mut out = Vec::with_capacity(8 + body.len());
    out.extend_from_slice(&op.to_le_bytes());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    out
}

fn read_frame(r: &mut impl Read) -> std::io::Result<(u32, Value)> {
    let mut head = [0u8; 8];
    r.read_exact(&mut head)?;
    let op = u32::from_le_bytes([head[0], head[1], head[2], head[3]]);
    let len = u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::other("discord frame too large"));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body).unwrap_or(Value::Null)))
}

/// Discord wants 2 to 128 characters for the two text lines.
fn line(text: &str, fallback: &str) -> String {
    let t: String = text.trim().chars().take(128).collect();
    if t.chars().count() >= 2 {
        t
    } else {
        fallback.to_owned()
    }
}

fn https(url: &str) -> bool {
    url.starts_with("https://")
        && url.len() <= 256
        && !url.chars().any(|c| c.is_control() || c == ' ')
}

/// YouTube titles carry hashtags and trailing emoji ("... #saran #ติดตามเค้าหน่อย 🎵🎵"). Those only make the card long:
/// drop hashtags, and trailing words that have no letter or digit in them. Falls back to the original when nothing is left.
fn tidy_title(title: &str) -> String {
    let mut words: Vec<&str> = title
        .split_whitespace()
        .filter(|w| !w.starts_with('#'))
        .collect();
    while words
        .last()
        .is_some_and(|w| !w.chars().any(char::is_alphanumeric))
    {
        words.pop();
    }
    let t = words.join(" ");
    if t.is_empty() {
        title.trim().to_owned()
    } else {
        t
    }
}

/// `now_secs` is injected so the timestamps can be tested.
fn activity(s: &Snapshot, now_secs: i64) -> Value {
    let artists = line(&s.artists.join(", "), "Unknown artist");
    let mut a = json!({
        "type": 2,
        // 1 = the artist names the card in the member list ("Listening to SURIYA MQT"), not just the app.
        "status_display_type": 1,
        "details": line(&tidy_title(&s.title), "Unknown title"),
        "state": if s.play == Play::Paused { line(&format!("Paused · {artists}"), "Paused") } else { artists },
    });
    // The title is a link to the video.
    if crate::core::playback::extract::valid_id(&s.track_id) {
        a["details_url"] = json!(format!("https://www.youtube.com/watch?v={}", s.track_id));
    }
    if s.play == Play::Playing && s.duration_ms > 0 {
        let start = now_secs - (s.position_ms / 1000) as i64;
        a["timestamps"] = json!({ "start": start, "end": start + (s.duration_ms / 1000) as i64 });
    }
    // The cover is the big picture, with the logo as the corner badge. Without a cover the logo is the big picture.
    let cover = s.artwork.as_deref().filter(|u| https(u));
    let mut assets = json!({
        "large_image": cover.unwrap_or(LOGO_ASSET),
        // On a "Listening" card Discord shows this as the third line, under the artist.
        "large_text": "on LunoRX",
        "small_text": if s.play == Play::Paused { "LunoRX - paused" } else { "LunoRX" },
    });
    if cover.is_some() {
        assets["small_image"] = json!(LOGO_ASSET);
    }
    a["assets"] = assets;
    let mut buttons = Vec::new();
    if crate::core::playback::extract::valid_id(&s.track_id) {
        buttons.push(json!({ "label": "Listen on YouTube", "url": format!("https://www.youtube.com/watch?v={}", s.track_id) }));
    }
    buttons.push(json!({ "label": "Get LunoRX", "url": REPO_URL }));
    a["buttons"] = Value::Array(buttons);
    a
}

// ---- transport ---------------------------------------------------------------------------------------------------

trait Pipe: Read + Write + Send {}
impl<T: Read + Write + Send> Pipe for T {}

#[cfg(unix)]
fn open_pipe() -> Option<Box<dyn Pipe>> {
    use std::os::unix::net::UnixStream;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for var in ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"] {
        if let Some(d) = std::env::var_os(var) {
            dirs.push(PathBuf::from(d));
        }
    }
    dirs.push(PathBuf::from("/tmp"));
    // Flatpak and snap keep the socket in a subfolder of the runtime dir.
    if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR") {
        dirs.push(PathBuf::from(&d).join("app/com.discordapp.Discord"));
        dirs.push(PathBuf::from(&d).join("snap.discord"));
    }
    for dir in dirs {
        for n in 0..10 {
            if let Ok(s) = UnixStream::connect(dir.join(format!("discord-ipc-{n}"))) {
                let _ = s.set_read_timeout(Some(Duration::from_secs(4)));
                let _ = s.set_write_timeout(Some(Duration::from_secs(4)));
                return Some(Box::new(s));
            }
        }
    }
    None
}

#[cfg(windows)]
fn open_pipe() -> Option<Box<dyn Pipe>> {
    (0..10).find_map(|n| {
        std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!(r"\\.\pipe\discord-ipc-{n}"))
            .ok()
            .map(|f| Box::new(f) as Box<dyn Pipe>)
    })
}

#[cfg(not(any(unix, windows)))]
fn open_pipe() -> Option<Box<dyn Pipe>> {
    None
}

struct Link {
    pipe: Box<dyn Pipe>,
    nonce: u64,
}

impl Link {
    fn connect(client_id: &str) -> Option<Self> {
        Self::over(open_pipe()?, client_id)
    }
    fn over(mut pipe: Box<dyn Pipe>, client_id: &str) -> Option<Self> {
        pipe.write_all(&frame(
            OP_HANDSHAKE,
            &json!({ "v": 1, "client_id": client_id }),
        ))
        .ok()?;
        let (_, reply) = read_frame(&mut pipe).ok()?;
        // READY on success; an error event when the Application ID is unknown.
        (reply["evt"] == "READY").then_some(Self { pipe, nonce: 0 })
    }
    fn set(&mut self, activity: Option<Value>) -> bool {
        self.nonce += 1;
        let payload = json!({
            "cmd": "SET_ACTIVITY",
            "args": { "pid": std::process::id(), "activity": activity },
            "nonce": self.nonce.to_string(),
        });
        if self.pipe.write_all(&frame(OP_FRAME, &payload)).is_err() {
            return false;
        }
        // Read the answer so the pipe never fills up.
        read_frame(&mut self.pipe).is_ok()
    }
}

// ---- worker ------------------------------------------------------------------------------------------------------

fn set_status(s: Status) {
    if let Some(sh) = SHARED.get() {
        *sh.status.lock().unwrap_or_else(|e| e.into_inner()) = s;
    }
}

fn worker(rx: mpsc::Receiver<Msg>, initial: Settings) {
    let mut settings = initial;
    let mut link: Option<Link> = None;
    let mut want: Option<Snapshot> = None;
    let mut sent: Option<(Snapshot, i64)> = None; // what Discord shows, and the start time it was given
    let mut last_send = Instant::now() - MIN_GAP;
    let mut last_try = Instant::now() - RETRY;
    loop {
        match rx.recv_timeout(Duration::from_millis(1000)) {
            Ok(Msg::Settings(s)) => {
                if s != settings {
                    if let Some(mut l) = link.take() {
                        l.set(None);
                    }
                    sent = None;
                    last_try = Instant::now() - RETRY;
                }
                settings = s;
            }
            Ok(Msg::Track(t)) => want = t,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        if !settings.enabled || !valid_client_id(settings.effective_client_id()) {
            if let Some(mut l) = link.take() {
                l.set(None);
            }
            sent = None;
            set_status(Status::Off);
            continue;
        }
        if link.is_none() {
            if last_try.elapsed() < RETRY {
                continue;
            }
            last_try = Instant::now();
            set_status(Status::Connecting);
            match Link::connect(settings.effective_client_id()) {
                Some(l) => {
                    link = Some(l);
                    sent = None;
                    set_status(Status::Connected);
                }
                None => {
                    set_status(Status::Unavailable);
                    continue;
                }
            }
        }
        let Some(l) = link.as_mut() else { continue };
        if last_send.elapsed() < MIN_GAP {
            continue;
        }
        let now = unix_now();
        let due = match (&want, &sent) {
            (None, None) => false,
            (None, Some(_)) => true,
            (Some(_), None) => true,
            (Some(w), Some((s, start))) => {
                let expected = now - (w.position_ms / 1000) as i64;
                s.track_id != w.track_id
                    || s.play != w.play
                    || (w.play == Play::Playing && (expected - start).abs() * 1000 > DRIFT_MS)
            }
        };
        if !due {
            continue;
        }
        let ok = match &want {
            Some(w) => l.set(Some(activity(w, now))),
            None => l.set(None),
        };
        last_send = Instant::now();
        if ok {
            sent = want.clone().map(|w| {
                let start = now - (w.position_ms / 1000) as i64;
                (w, start)
            });
        } else {
            // Discord closed: connect again on the next round.
            link = None;
            sent = None;
            set_status(Status::Unavailable);
        }
    }
}

fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

// ---- public ------------------------------------------------------------------------------------------------------

/// Reads the saved settings and starts the worker. Call once from `setup`.
pub fn start(dir: Option<PathBuf>) {
    let path = dir.map(|d| d.join(SETTINGS_FILE));
    let settings: Settings = path
        .as_ref()
        .and_then(|p| std::fs::read(p).ok())
        .filter(|b| b.len() < 4096)
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let (tx, rx) = mpsc::channel();
    let initial = settings.clone();
    let shared = Shared {
        tx,
        status: Mutex::new(Status::Off),
        settings: Mutex::new(settings),
        path,
    };
    if SHARED.set(shared).is_ok() {
        let _ = std::thread::Builder::new()
            .name("discord-presence".into())
            .spawn(move || worker(rx, initial));
    }
}

pub fn settings() -> (Settings, Status) {
    match SHARED.get() {
        Some(s) => (
            s.settings.lock().unwrap_or_else(|e| e.into_inner()).clone(),
            *s.status.lock().unwrap_or_else(|e| e.into_inner()),
        ),
        None => (Settings::default(), Status::Off),
    }
}

/// Saves and applies new settings. `Err` for a malformed Application ID.
pub fn apply(new: Settings) -> Result<(), &'static str> {
    let clean = Settings {
        enabled: new.enabled,
        client_id: new.client_id.trim().to_owned(),
    };
    if !clean.client_id.is_empty() && !valid_client_id(&clean.client_id) {
        return Err("invalid application id");
    }
    let Some(sh) = SHARED.get() else {
        return Err("unavailable");
    };
    *sh.settings.lock().unwrap_or_else(|e| e.into_inner()) = clean.clone();
    if let (Some(path), Ok(bytes)) = (&sh.path, serde_json::to_vec(&clean)) {
        let _ = std::fs::write(path, bytes);
    }
    let _ = sh.tx.send(Msg::Settings(clean));
    Ok(())
}

/// Offers the player's latest state. Cheap: it only hands a snapshot to the worker, which decides whether Discord needs an update.
pub fn offer(dto: &PlayerProgressDto) {
    let Some(sh) = SHARED.get() else { return };
    if !sh
        .settings
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .enabled
    {
        return;
    }
    let play = match dto.player.state {
        PlaybackStateDto::Playing | PlaybackStateDto::Buffering => Some(Play::Playing),
        PlaybackStateDto::Paused => Some(Play::Paused),
        _ => None,
    };
    let snapshot = match (&dto.track, play) {
        (Some(t), Some(play)) => Some(Snapshot {
            track_id: t.provider_track_id.clone(),
            title: t.title.clone(),
            artists: t.artists.clone(),
            artwork: t.artwork_url.clone(),
            play,
            position_ms: dto.position_ms,
            duration_ms: dto.duration_ms,
        }),
        _ => None,
    };
    let _ = sh.tx.send(Msg::Track(snapshot));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(play: Play) -> Snapshot {
        Snapshot {
            track_id: "aqz-KE-bpKQ".into(),
            title: "BU".into(),
            artists: vec!["SURIYA MQT".into(), "P6ICK".into()],
            artwork: Some("https://i.ytimg.com/vi/aqz-KE-bpKQ/hqdefault.jpg".into()),
            play,
            position_ms: 30_000,
            duration_ms: 166_000,
        }
    }

    /// A stand-in for the Discord app on the other end of a socket pair.
    #[cfg(unix)]
    #[test]
    fn handshake_then_set_and_clear_activity_against_a_fake_discord() {
        use std::os::unix::net::UnixStream;
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let fake = std::thread::spawn(move || {
            let (op, hello) = read_frame(&mut theirs).unwrap();
            assert_eq!(
                (op, hello["client_id"].as_str()),
                (OP_HANDSHAKE, Some("123456789012345678"))
            );
            theirs
                .write_all(&frame(
                    OP_FRAME,
                    &json!({ "cmd": "DISPATCH", "evt": "READY" }),
                ))
                .unwrap();
            let mut seen = Vec::new();
            for _ in 0..2 {
                let (_, set) = read_frame(&mut theirs).unwrap();
                seen.push(set);
                theirs
                    .write_all(&frame(OP_FRAME, &json!({ "cmd": "SET_ACTIVITY" })))
                    .unwrap();
            }
            seen
        });
        let mut link = Link::over(Box::new(ours), "123456789012345678").expect("READY");
        assert!(link.set(Some(activity(&snap(Play::Playing), 100))));
        assert!(link.set(None));
        let seen = fake.join().unwrap();
        assert_eq!(seen[0]["cmd"], "SET_ACTIVITY");
        assert_eq!(seen[0]["args"]["activity"]["details"], "BU");
        assert_eq!(seen[0]["args"]["pid"], std::process::id());
        assert!(seen[1]["args"]["activity"].is_null());
    }

    #[cfg(unix)]
    #[test]
    fn an_unknown_application_id_is_not_ready() {
        use std::os::unix::net::UnixStream;
        let (ours, mut theirs) = UnixStream::pair().unwrap();
        let fake = std::thread::spawn(move || {
            let _ = read_frame(&mut theirs).unwrap();
            theirs
                .write_all(&frame(
                    OP_FRAME,
                    &json!({ "evt": "ERROR", "data": { "code": 4000 } }),
                ))
                .unwrap();
        });
        assert!(Link::over(Box::new(ours), "123456789012345678").is_none());
        fake.join().unwrap();
    }

    #[test]
    fn the_built_in_id_is_valid_and_a_custom_one_replaces_it() {
        assert!(valid_client_id(DEFAULT_CLIENT_ID));
        let mut s = Settings {
            enabled: true,
            client_id: String::new(),
        };
        assert_eq!(s.effective_client_id(), DEFAULT_CLIENT_ID);
        s.client_id = "123456789012345678".into();
        assert_eq!(s.effective_client_id(), "123456789012345678");
    }

    #[test]
    fn client_ids_are_snowflake_shaped() {
        assert!(valid_client_id("123456789012345678"));
        assert!(!valid_client_id("12345"));
        assert!(!valid_client_id("12345678901234567x"));
        assert!(!valid_client_id(&"1".repeat(21)));
    }

    #[test]
    fn titles_lose_hashtags_and_trailing_symbols_but_keep_their_text() {
        assert_eq!(
            tidy_title("BIG ASS - เล่นของสูง ft. SARAN (Don 420) #saran #ติดตามเค้าหน่อย 🎵🎵"),
            "BIG ASS - เล่นของสูง ft. SARAN (Don 420)"
        );
        assert_eq!(tidy_title("Song - Live ~ "), "Song - Live");
        assert_eq!(tidy_title("#only #tags"), "#only #tags");
        assert_eq!(tidy_title("Plain title"), "Plain title");
    }

    #[test]
    fn frames_round_trip() {
        let bytes = frame(OP_FRAME, &json!({ "a": 1 }));
        assert_eq!(&bytes[0..4], &1u32.to_le_bytes());
        let (op, v) = read_frame(&mut std::io::Cursor::new(bytes)).unwrap();
        assert_eq!((op, v["a"].as_i64()), (OP_FRAME, Some(1)));
        let mut huge = vec![0u8; 8];
        huge[4..8].copy_from_slice(&(1_000_000u32).to_le_bytes());
        assert!(read_frame(&mut std::io::Cursor::new(huge)).is_err());
    }

    #[test]
    fn playing_activity_has_listening_type_times_and_a_link() {
        let a = activity(&snap(Play::Playing), 1_000_000);
        assert_eq!(a["type"], 2);
        assert_eq!(a["details"], "BU");
        assert_eq!(a["state"], "SURIYA MQT, P6ICK");
        assert_eq!(a["timestamps"]["start"], 1_000_000 - 30);
        assert_eq!(a["timestamps"]["end"], 1_000_000 - 30 + 166);
        assert_eq!(
            a["assets"]["large_image"],
            "https://i.ytimg.com/vi/aqz-KE-bpKQ/hqdefault.jpg"
        );
        assert_eq!(a["status_display_type"], 1);
        assert_eq!(
            a["details_url"],
            "https://www.youtube.com/watch?v=aqz-KE-bpKQ"
        );
        assert_eq!(
            a["buttons"][0]["url"],
            "https://www.youtube.com/watch?v=aqz-KE-bpKQ"
        );
    }

    #[test]
    fn paused_activity_drops_the_clock_and_bad_input_is_made_safe() {
        let mut s = snap(Play::Paused);
        s.title = "x".into();
        s.artwork = Some("http://insecure.example/a.jpg".into());
        s.track_id = "not an id".into();
        let a = activity(&s, 5);
        assert_eq!(a["state"], "Paused · SURIYA MQT, P6ICK");
        assert!(a.get("timestamps").is_none());
        assert_eq!(a["details"], "Unknown title");
        // No usable cover: the logo is the big picture, and there is no corner badge on top of it.
        assert_eq!(a["assets"]["large_image"], LOGO_ASSET);
        assert!(a["assets"].get("small_image").is_none());
        assert_eq!(a["assets"]["small_text"], "LunoRX - paused");
        // No valid video id: only the link to the app remains.
        assert_eq!(a["buttons"].as_array().unwrap().len(), 1);
        assert_eq!(a["buttons"][0]["label"], "Get LunoRX");
    }
}
