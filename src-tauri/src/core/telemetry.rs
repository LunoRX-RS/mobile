//! Opt-in, anonymous usage counters. Off until the user turns it on. Only counter names and counts are kept: never a title,
//! artist, search text, account id or provider credential. `pending` holds exactly what the next upload will contain.
use super::library::LibraryError;
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, fs, io::Write, path::PathBuf, sync::Mutex};

const MAX_NAMES: usize = 200;
const PREFIXES: [&str; 4] = ["app.", "feature.", "error.", "perf."];

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Saved {
    enabled: bool,
    install_id: String,
    #[serde(default)]
    pending: BTreeMap<String, u32>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TelemetryView {
    pub enabled: bool,
    pub install_id: String,
    pub pending: BTreeMap<String, u32>,
}

/// Install id and the counters taken for one upload.
pub type Taken = (String, BTreeMap<String, u32>);

pub struct Telemetry {
    path: PathBuf,
    saved: Mutex<Saved>,
}

/// Names the server accepts: a known prefix, then `[a-z0-9._-]`, at most 64 characters.
pub fn valid_name(name: &str) -> bool {
    name.len() <= 64
        && PREFIXES
            .iter()
            .any(|p| name.starts_with(p) && name.len() > p.len())
        && name.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn new_install_id() -> String {
    let mut b: [u8; 16] = rand::random();
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    let h: String = b.iter().map(|x| format!("{x:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &h[0..8],
        &h[8..12],
        &h[12..16],
        &h[16..20],
        &h[20..32]
    )
}

impl Telemetry {
    pub fn open(path: PathBuf) -> Result<Self, LibraryError> {
        let mut saved = if path.exists() {
            let meta = fs::symlink_metadata(&path).map_err(|_| LibraryError::Storage)?;
            if !meta.file_type().is_file() || meta.len() > 256 * 1024 {
                return Err(LibraryError::Storage);
            }
            serde_json::from_slice::<Saved>(&fs::read(&path).map_err(|_| LibraryError::Storage)?)
                .map_err(|_| LibraryError::Storage)?
        } else {
            Saved::default()
        };
        if saved.pending.len() > MAX_NAMES || saved.pending.keys().any(|k| !valid_name(k)) {
            saved.pending.clear();
        }
        if saved.install_id.is_empty() {
            saved.install_id = new_install_id();
        }
        Ok(Self {
            path,
            saved: Mutex::new(saved),
        })
    }

    pub fn view(&self) -> Result<TelemetryView, LibraryError> {
        let s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        Ok(view_of(&s))
    }

    /// Turning it off drops anything not yet sent.
    pub fn set_enabled(&self, on: bool) -> Result<TelemetryView, LibraryError> {
        self.update(|s| {
            s.enabled = on;
            if !on {
                s.pending.clear();
            }
        })
    }

    /// Counts one use of `name`. Does nothing while telemetry is off.
    pub fn record(&self, name: &str) -> Result<(), LibraryError> {
        if !valid_name(name) {
            return Err(LibraryError::Validation);
        }
        let s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        if !s.enabled || (!s.pending.contains_key(name) && s.pending.len() >= MAX_NAMES) {
            return Ok(());
        }
        drop(s);
        self.update(|s| {
            let n = s.pending.entry(name.to_string()).or_default();
            *n = n.saturating_add(1);
        })
        .map(|_| ())
    }

    /// Takes what is pending for an upload, or nothing when it is off or empty.
    pub fn take(&self) -> Result<Option<Taken>, LibraryError> {
        let s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        if !s.enabled || s.pending.is_empty() {
            return Ok(None);
        }
        let taken = (s.install_id.clone(), s.pending.clone());
        drop(s);
        self.update(|s| s.pending.clear())?;
        Ok(Some(taken))
    }

    /// Puts counters back after a failed upload, adding to anything counted meanwhile.
    pub fn restore(&self, counters: BTreeMap<String, u32>) -> Result<(), LibraryError> {
        self.update(|s| {
            if !s.enabled {
                return;
            }
            for (k, v) in counters {
                if s.pending.len() >= MAX_NAMES && !s.pending.contains_key(&k) {
                    continue;
                }
                let n = s.pending.entry(k).or_default();
                *n = n.saturating_add(v);
            }
        })
        .map(|_| ())
    }

    /// After the server deleted this install's rows: a new anonymous id, nothing pending, and telemetry off.
    pub fn reset(&self) -> Result<TelemetryView, LibraryError> {
        self.update(|s| {
            s.enabled = false;
            s.pending.clear();
            s.install_id = new_install_id();
        })
    }

    fn update(&self, change: impl FnOnce(&mut Saved)) -> Result<TelemetryView, LibraryError> {
        let mut s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = s.clone();
        change(&mut next);
        let bytes = serde_json::to_vec(&next).map_err(|_| LibraryError::Storage)?;
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|_| LibraryError::Storage)?;
        }
        let part = self
            .path
            .with_extension(format!("{:032x}.part", rand::random::<u128>()));
        let result = (|| {
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&part).map_err(|_| LibraryError::Storage)?;
            file.write_all(&bytes).map_err(|_| LibraryError::Storage)?;
            file.sync_all().map_err(|_| LibraryError::Storage)?;
            fs::rename(&part, &self.path).map_err(|_| LibraryError::Storage)
        })();
        let _ = fs::remove_file(part);
        result?;
        *s = next;
        Ok(view_of(&s))
    }
}

fn view_of(s: &Saved) -> TelemetryView {
    TelemetryView {
        enabled: s.enabled,
        install_id: s.install_id.clone(),
        pending: s.pending.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> (Telemetry, PathBuf) {
        let p =
            std::env::temp_dir().join(format!("lunorx-tel-{:032x}.json", rand::random::<u128>()));
        (Telemetry::open(p.clone()).unwrap(), p)
    }

    #[test]
    fn names_follow_the_server_rules() {
        assert!(valid_name("feature.stats"));
        assert!(valid_name("error.sync-offline"));
        assert!(!valid_name("feature."));
        assert!(!valid_name("song.title"));
        assert!(!valid_name("feature.Stats"));
        assert!(!valid_name("feature.a b"));
        assert!(!valid_name(&format!("feature.{}", "a".repeat(60))));
    }

    #[test]
    fn nothing_is_counted_until_it_is_turned_on_and_off_drops_pending() {
        let (t, p) = open();
        t.record("feature.stats").unwrap();
        assert!(t.view().unwrap().pending.is_empty());
        assert!(t.take().unwrap().is_none());
        t.set_enabled(true).unwrap();
        t.record("feature.stats").unwrap();
        t.record("feature.stats").unwrap();
        assert_eq!(t.view().unwrap().pending["feature.stats"], 2);
        assert!(t.record("a song title").is_err());
        t.set_enabled(false).unwrap();
        assert!(t.view().unwrap().pending.is_empty());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn a_failed_upload_puts_the_counters_back_and_reset_makes_a_new_id() {
        let (t, p) = open();
        t.set_enabled(true).unwrap();
        t.record("app.start").unwrap();
        let (id, taken) = t.take().unwrap().unwrap();
        assert!(t.view().unwrap().pending.is_empty());
        t.record("app.start").unwrap();
        t.restore(taken).unwrap();
        assert_eq!(t.view().unwrap().pending["app.start"], 2);
        let reopened = Telemetry::open(p.clone()).unwrap();
        assert_eq!(reopened.view().unwrap().install_id, id);
        let after = reopened.reset().unwrap();
        assert!(!after.enabled && after.pending.is_empty() && after.install_id != id);
        let _ = fs::remove_file(p);
    }
}
