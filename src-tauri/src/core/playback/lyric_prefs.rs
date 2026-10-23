//! User choices for lyric sources: which are disabled, the Auto priority, and per-track bindings.
//! Both files hold only source ids, track ids and opaque candidate keys: no lyrics, tokens or paths.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

/// Every source id the backend can query, with its display label. `mine` is the frontend's own
/// Lyric Maker entry: it takes part in the order but is never queried here.
pub const SOURCES: [(&str, &str); 7] = [
    ("youtubeMusic", "YouTube Music"),
    ("lrclib", "LRCLIB"),
    ("lyricsplus", "LyricsPlus"),
    ("boidu", "Boidu"),
    ("netease", "Netease Cloud Music"),
    ("qq", "QQ Music"),
    ("kugou", "Kugou"),
];
pub const DEFAULT_ORDER: [&str; 8] = [
    "mine",
    "lrclib",
    "lyricsplus",
    "boidu",
    "netease",
    "kugou",
    "qq",
    "youtubeMusic",
];

const SETTINGS_MAX_BYTES: u64 = 4096;
const BINDINGS_MAX_BYTES: u64 = 65_536;
const BINDINGS_MAX_ENTRIES: usize = 2000;
const MAX_LIST: usize = 16;
const MAX_ID_CHARS: usize = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefsError {
    Validation,
    TooLarge,
    Storage,
}

pub fn known_source(id: &str) -> bool {
    SOURCES.iter().any(|(known, _)| *known == id)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub order: Vec<String>,
    pub disabled: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            order: DEFAULT_ORDER.iter().map(|s| (*s).to_owned()).collect(),
            disabled: Vec::new(),
        }
    }
}

impl Settings {
    pub fn is_disabled(&self, id: &str) -> bool {
        self.disabled.iter().any(|d| d == id)
    }

    /// Position in the Auto order; unknown ids sort last.
    pub fn rank(&self, id: &str) -> usize {
        self.order
            .iter()
            .position(|o| o == id)
            .unwrap_or(usize::MAX)
    }
}

/// Strict (IPC input): unknown ids are an error, duplicates are dropped, missing ids are appended in
/// the default order.
pub fn normalize(order: &[String], disabled: &[String]) -> Result<Settings, PrefsError> {
    if order.len() > MAX_LIST || disabled.len() > MAX_LIST {
        return Err(PrefsError::Validation);
    }
    let valid = |id: &String| id.chars().count() <= MAX_ID_CHARS;
    if !order.iter().chain(disabled).all(valid) {
        return Err(PrefsError::Validation);
    }
    if order.iter().any(|id| !DEFAULT_ORDER.contains(&id.as_str()))
        || disabled.iter().any(|id| !known_source(id))
    {
        return Err(PrefsError::Validation);
    }
    Ok(lenient(order, disabled))
}

/// Same shape without errors, for a file that may be hand-edited or from another version.
fn lenient(order: &[String], disabled: &[String]) -> Settings {
    let mut out = Settings {
        order: Vec::new(),
        disabled: Vec::new(),
    };
    for id in order {
        if DEFAULT_ORDER.contains(&id.as_str()) && !out.order.contains(id) {
            out.order.push(id.clone());
        }
    }
    for id in DEFAULT_ORDER {
        if !out.order.iter().any(|o| o == id) {
            out.order.push(id.to_owned());
        }
    }
    for id in disabled {
        if known_source(id) && !out.disabled.contains(id) {
            out.disabled.push(id.clone());
        }
    }
    out
}

fn read_capped(path: &Path, max: u64) -> Option<Vec<u8>> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file() || meta.len() > max {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(max + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    (bytes.len() as u64 <= max).then_some(bytes)
}

/// Temp file with owner-only permissions, then rename: a crash never leaves a half-written file.
fn write_atomic(path: &Path, bytes: &[u8], max: u64) -> Result<(), PrefsError> {
    if bytes.len() as u64 > max {
        return Err(PrefsError::TooLarge);
    }
    static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITES.lock().map_err(|_| PrefsError::Storage)?;
    let root = path.parent().ok_or(PrefsError::Storage)?;
    let part = path.with_extension(format!("{:032x}.part", rand::random::<u128>()));
    let result = (|| {
        fs::create_dir_all(root)?;
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&part, path)
    })();
    let _ = fs::remove_file(&part);
    result.map_err(|_| PrefsError::Storage)
}

#[derive(Serialize, Deserialize)]
struct SettingsFile {
    version: u8,
    order: Vec<String>,
    disabled: Vec<String>,
}

/// A missing, oversized or corrupt file means defaults.
pub fn load_settings(path: &Path) -> Settings {
    read_capped(path, SETTINGS_MAX_BYTES)
        .and_then(|b| serde_json::from_slice::<SettingsFile>(&b).ok())
        .map(|f| lenient(&f.order, &f.disabled))
        .unwrap_or_default()
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<(), PrefsError> {
    let bytes = serde_json::to_vec(&SettingsFile {
        version: 1,
        order: settings.order.clone(),
        disabled: settings.disabled.clone(),
    })
    .map_err(|_| PrefsError::Storage)?;
    write_atomic(path, &bytes, SETTINGS_MAX_BYTES)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Binding {
    pub provider_track_id: String,
    pub source_id: String,
    pub candidate_key: String,
}

#[derive(Serialize, Deserialize)]
struct BindingsFile {
    version: u8,
    entries: Vec<Binding>,
}

/// Candidate keys are opaque to the app and only ever placed in a query value: this is the whole contract.
pub fn valid_key(key: &str) -> bool {
    (1..=128).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'.' | b'-'))
}

fn load_all(path: &Path) -> Vec<Binding> {
    read_capped(path, BINDINGS_MAX_BYTES)
        .and_then(|b| serde_json::from_slice::<BindingsFile>(&b).ok())
        .map(|f| {
            f.entries
                .into_iter()
                .filter(|e| {
                    known_source(&e.source_id)
                        && valid_key(&e.candidate_key)
                        && crate::core::playback::extract::valid_id(&e.provider_track_id)
                })
                .take(BINDINGS_MAX_ENTRIES)
                .collect()
        })
        .unwrap_or_default()
}

fn save_all(path: &Path, entries: Vec<Binding>) -> Result<(), PrefsError> {
    if entries.len() > BINDINGS_MAX_ENTRIES {
        return Err(PrefsError::TooLarge);
    }
    let bytes = serde_json::to_vec(&BindingsFile {
        version: 1,
        entries,
    })
    .map_err(|_| PrefsError::Storage)?;
    write_atomic(path, &bytes, BINDINGS_MAX_BYTES)
}

/// The bound candidate key per source for one track.
pub fn bindings_for(path: &Path, track: &str) -> Vec<(String, String)> {
    load_all(path)
        .into_iter()
        .filter(|e| e.provider_track_id == track)
        .map(|e| (e.source_id, e.candidate_key))
        .collect()
}

/// Adds or replaces the binding for (track, source). A full file is an error, never a silent eviction.
pub fn bind(path: &Path, binding: Binding) -> Result<(), PrefsError> {
    if !known_source(&binding.source_id)
        || !valid_key(&binding.candidate_key)
        || !crate::core::playback::extract::valid_id(&binding.provider_track_id)
    {
        return Err(PrefsError::Validation);
    }
    let mut entries = load_all(path);
    entries.retain(|e| {
        !(e.provider_track_id == binding.provider_track_id && e.source_id == binding.source_id)
    });
    entries.push(binding);
    save_all(path, entries)
}

/// True when a binding existed.
pub fn unbind(path: &Path, track: &str, source: &str) -> Result<bool, PrefsError> {
    let mut entries = load_all(path);
    let before = entries.len();
    entries.retain(|e| !(e.provider_track_id == track && e.source_id == source));
    if entries.len() == before {
        return Ok(false);
    }
    save_all(path, entries).map(|()| true)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lunor-prefs-{:032x}", rand::random::<u128>()))
    }
    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn defaults_have_every_source_and_nothing_disabled() {
        let d = Settings::default();
        assert_eq!(d.order[0], "mine");
        assert!(d.disabled.is_empty());
        for (id, _) in SOURCES {
            assert!(d.order.iter().any(|o| o == id), "{id}");
        }
    }

    #[test]
    fn normalize_rejects_unknown_drops_duplicates_and_appends_missing() {
        let n = normalize(&s(&["qq", "lrclib", "qq"]), &s(&["kugou", "kugou"])).unwrap();
        assert_eq!(&n.order[..2], ["qq", "lrclib"]);
        assert_eq!(n.order.len(), DEFAULT_ORDER.len());
        // Missing ids keep their default relative order.
        assert_eq!(
            &n.order[2..],
            [
                "mine",
                "lyricsplus",
                "boidu",
                "netease",
                "kugou",
                "youtubeMusic"
            ]
        );
        assert_eq!(n.disabled, ["kugou"]);
        assert_eq!(normalize(&s(&["nope"]), &[]), Err(PrefsError::Validation));
        assert_eq!(
            normalize(&[], &s(&["simpmusic"])),
            Err(PrefsError::Validation)
        );
        // `mine` is only an ordering slot, never a queryable (disable-able) source.
        assert_eq!(normalize(&[], &s(&["mine"])), Err(PrefsError::Validation));
        assert_eq!(
            normalize(&vec!["qq".to_owned(); 17], &[]),
            Err(PrefsError::Validation)
        );
        assert_eq!(
            normalize(&[], &["x".repeat(40)]),
            Err(PrefsError::Validation)
        );
    }

    #[test]
    fn settings_roundtrip_private_and_corrupt_means_defaults() {
        let root = root();
        let path = root.join("lyrics-settings.json");
        assert_eq!(load_settings(&path), Settings::default());
        let n = normalize(&s(&["netease"]), &s(&["boidu"])).unwrap();
        save_settings(&path, &n).unwrap();
        assert_eq!(load_settings(&path), n);
        assert!(n.is_disabled("boidu") && !n.is_disabled("qq"));
        assert!(n.rank("netease") < n.rank("lrclib"));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::write(&path, b"not json").unwrap();
        assert_eq!(load_settings(&path), Settings::default());
        fs::write(&path, vec![b' '; 5000]).unwrap();
        assert_eq!(load_settings(&path), Settings::default());
        // A hand-edited file with unknown ids is repaired, not trusted.
        fs::write(
            &path,
            br#"{"version":1,"order":["zzz","qq"],"disabled":["zzz","kugou"]}"#,
        )
        .unwrap();
        let loaded = load_settings(&path);
        assert_eq!(loaded.order[0], "qq");
        assert_eq!(loaded.disabled, ["kugou"]);
        fs::remove_dir_all(root).unwrap();
    }

    fn binding(track: &str, source: &str, key: &str) -> Binding {
        Binding {
            provider_track_id: track.into(),
            source_id: source.into(),
            candidate_key: key.into(),
        }
    }

    #[test]
    fn bind_persists_replaces_and_unbinds() {
        let root = root();
        let path = root.join("lyrics-bindings.json");
        let track = "dQw4w9WgXcQ";
        assert!(bindings_for(&path, track).is_empty());
        bind(&path, binding(track, "netease", "123")).unwrap();
        bind(&path, binding(track, "qq", "abc_DEF")).unwrap();
        bind(&path, binding(track, "netease", "456")).unwrap();
        let mut got = bindings_for(&path, track);
        got.sort();
        assert_eq!(
            got,
            [
                ("netease".to_owned(), "456".to_owned()),
                ("qq".to_owned(), "abc_DEF".to_owned())
            ]
        );
        assert!(bindings_for(&path, "otherTrack1").is_empty());
        assert!(unbind(&path, track, "netease").unwrap());
        assert!(!unbind(&path, track, "netease").unwrap());
        assert_eq!(bindings_for(&path, track).len(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn bind_rejects_bad_keys_sources_and_ids() {
        let root = root();
        let path = root.join("b.json");
        for bad in ["", "a b", "a/b", "a?b", "ü", &"k".repeat(129)] {
            assert_eq!(
                bind(&path, binding("dQw4w9WgXcQ", "qq", bad)),
                Err(PrefsError::Validation),
                "{bad}"
            );
        }
        assert!(valid_key(&"k".repeat(128)) && valid_key("a:b.c-d_E9"));
        assert_eq!(
            bind(&path, binding("dQw4w9WgXcQ", "simpmusic", "1")),
            Err(PrefsError::Validation)
        );
        assert_eq!(
            bind(&path, binding("../x", "qq", "1")),
            Err(PrefsError::Validation)
        );
        assert!(!path.exists());
    }

    #[test]
    fn bindings_file_is_bounded_and_full_is_an_error_not_an_eviction() {
        let root = root();
        let path = root.join("b.json");
        let mut n = 0;
        let full = loop {
            let id = format!("trk{n:08}");
            match bind(
                &path,
                binding(
                    &id,
                    "kugou",
                    &format!("{}:{}", "9".repeat(20), "A".repeat(32)),
                ),
            ) {
                Ok(()) => n += 1,
                Err(e) => break e,
            }
            assert!(n <= BINDINGS_MAX_ENTRIES);
        };
        assert_eq!(full, PrefsError::TooLarge);
        assert!(fs::metadata(&path).unwrap().len() <= BINDINGS_MAX_BYTES);
        // Everything saved before the limit is still there.
        assert_eq!(bindings_for(&path, "trk00000000").len(), 1);
        assert_eq!(load_all(&path).len(), n);
        // Unbinding makes room again.
        assert!(unbind(&path, "trk00000000", "kugou").unwrap());
        bind(&path, binding("trk99999999", "kugou", "1")).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
