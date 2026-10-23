//! Bounded LRCLIB sidecars. Cache failures never interrupt audio or lyric lookup.
use super::lyrics::Record;
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Duration,
};
const MAX_BYTES: u64 = 131_072;
const MAX_FILES: usize = 256;
pub(super) fn load(path: &Path) -> Option<Record> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file()
        || meta.len() > MAX_BYTES
        || meta.modified().ok()?.elapsed().ok()? > Duration::from_secs(30 * 86400)
    {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let record: Record = serde_json::from_slice(&bytes).ok()?;
    (!record.track_name.trim().is_empty()
        && record.track_name.len() <= 2048
        && !record.artist_name.trim().is_empty()
        && record.artist_name.len() <= 2048
        && record.duration.is_finite()
        && (1.0..=3600.0).contains(&record.duration))
    .then_some(record)
}
pub(super) fn save(path: &Path, record: &Record) {
    let _ = serde_json::to_vec(record).map(|bytes| write(path, &bytes, MAX_BYTES));
}

const NEGATIVE_TTL_SECS: u64 = 86_400;
const NEGATIVE_MAX_BYTES: u64 = 1024;

#[derive(serde::Serialize, serde::Deserialize)]
struct Negative {
    key: String,
    at: u64,
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Sidecar next to the positive entries. `n-` never collides with the hex video-id names.
pub(super) fn negative_path(positive: &Path, key: &str) -> Option<std::path::PathBuf> {
    // FNV-1a: stable across Rust versions, unlike DefaultHasher.
    let hash = key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Some(positive.parent()?.join(format!("n-{hash:016x}.json")))
}

/// True when LRCLIB was searched for this key within the last 24 h and had no usable match.
pub(super) fn negative_fresh(path: &Path, key: &str) -> bool {
    negative_fresh_at(path, key, now())
}
fn negative_fresh_at(path: &Path, key: &str, now: u64) -> bool {
    (|| {
        let meta = fs::symlink_metadata(path).ok()?;
        if !meta.file_type().is_file() || meta.len() > NEGATIVE_MAX_BYTES {
            return None;
        }
        let mut bytes = Vec::new();
        fs::File::open(path)
            .ok()?
            .take(NEGATIVE_MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .ok()?;
        let entry: Negative = serde_json::from_slice(&bytes).ok()?;
        // A future timestamp is clock damage, not a fresh entry.
        (entry.key == key && entry.at <= now && now - entry.at <= NEGATIVE_TTL_SECS).then_some(())
    })()
    .is_some()
}
pub(super) fn negative_save(path: &Path, key: &str) {
    negative_save_at(path, key, now());
}
fn negative_save_at(path: &Path, key: &str, at: u64) {
    let key = key.chars().take(256).collect::<String>();
    let _ = serde_json::to_vec(&Negative { key, at })
        .map(|bytes| write(path, &bytes, NEGATIVE_MAX_BYTES));
}

/// One lyric result of a non-LRCLIB source, stored per (source, track). Positive entries live 30 days.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct SourceEntry {
    pub key: String,
    pub text: Option<String>,
    pub lines: Option<Vec<super::lyrics::TimedLine>>,
    pub instrumental: bool,
    pub timing_mismatch: bool,
    /// Length of the matched candidate, to re-check the 3 s timing rule on reuse.
    pub duration_ms: u32,
}

fn fnv(key: &str) -> u64 {
    key.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// `s-` never collides with the hex video-id names or the `n-` negative names.
pub(super) fn source_path(dir: &Path, source: &str, key: &str) -> std::path::PathBuf {
    dir.join(format!("s-{source}-{:016x}.json", fnv(key)))
}

pub(super) fn source_load(path: &Path, key: &str) -> Option<SourceEntry> {
    let meta = fs::symlink_metadata(path).ok()?;
    if !meta.file_type().is_file()
        || meta.len() > MAX_BYTES
        || meta.modified().ok()?.elapsed().ok()? > Duration::from_secs(30 * 86400)
    {
        return None;
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() as u64 > MAX_BYTES {
        return None;
    }
    let entry: SourceEntry = serde_json::from_slice(&bytes).ok()?;
    (entry.key == key).then_some(entry)
}

pub(super) fn source_save(path: &Path, entry: &SourceEntry) {
    let _ = serde_json::to_vec(entry).map(|bytes| write(path, &bytes, MAX_BYTES));
}

fn write(path: &Path, bytes: &[u8], max: u64) -> std::io::Result<()> {
    static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITES.lock().map_err(|_| std::io::ErrorKind::Other)?;
    if bytes.len() as u64 > max {
        return Ok(());
    }
    let root = path.parent().ok_or(std::io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(root)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(root, fs::Permissions::from_mode(0o700))?;
    }
    let mut files: Vec<_> = fs::read_dir(root)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|s| s == "json"))
        .collect();
    files.sort_by_key(|e| e.metadata().ok().and_then(|m| m.modified().ok()));
    for entry in files.iter().take(files.len().saturating_sub(MAX_FILES - 1)) {
        if entry.path() != path {
            fs::remove_file(entry.path())?;
        }
    }
    let part = path.with_extension(format!("{:032x}.part", rand::random::<u128>()));
    let result = (|| {
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
    let _ = fs::remove_file(part);
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    fn root() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("lunor-lyrics-{:032x}", rand::random::<u128>()))
    }
    fn record() -> Record {
        serde_json::from_value(serde_json::json!({"trackName":"Song","artistName":"Artist","duration":180,"syncedLyrics":"[00:01.00]A fixture line for testing one\n[00:02.00]A fixture line for testing two\n[00:03.00]A fixture line for testing three\n[00:04.00]A fixture line for testing four"})).unwrap()
    }
    #[test]
    fn disk_roundtrip_keeps_real_timestamps_and_private_permissions() {
        let root = root();
        let path = root.join("song.json");
        save(&path, &record());
        let found = super::super::lyrics::usable(load(&path).unwrap()).unwrap();
        assert_eq!(found.timed.unwrap()[0].start_ms, 1000);
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
    fn oversized_corrupt_and_expired_cache_are_misses() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let path = root.join("song.json");
        fs::write(&path, b"invalid").unwrap();
        assert!(load(&path).is_none());
        fs::File::create(&path)
            .unwrap()
            .set_len(MAX_BYTES + 1)
            .unwrap();
        assert!(load(&path).is_none());
        save(&path, &record());
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_times(
                fs::FileTimes::new()
                    .set_modified(std::time::SystemTime::now() - Duration::from_secs(31 * 86400)),
            )
            .unwrap();
        assert!(load(&path).is_none());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn negative_entry_is_fresh_for_24h_and_key_bound() {
        let root = root();
        let positive = root.join("abc.json");
        let path = negative_path(&positive, "key one").unwrap();
        assert_eq!(path.parent(), positive.parent());
        assert_ne!(path, negative_path(&positive, "key two").unwrap());
        let t = 1_000_000;
        negative_save_at(&path, "key one", t);
        assert!(negative_fresh_at(&path, "key one", t + 86_400));
        assert!(!negative_fresh_at(&path, "key one", t + 86_401));
        assert!(!negative_fresh_at(&path, "key two", t));
        assert!(!negative_fresh_at(&path, "key one", t - 1));
        assert!(fs::metadata(&path).unwrap().len() <= NEGATIVE_MAX_BYTES);
        // Oversized, corrupt and missing entries are misses, never errors.
        fs::write(&path, vec![b' '; 2000]).unwrap();
        assert!(!negative_fresh_at(&path, "key one", t));
        fs::write(&path, b"nope").unwrap();
        assert!(!negative_fresh_at(&path, "key one", t));
        assert!(!negative_fresh(&root.join("missing.json"), "key one"));
        fs::remove_dir_all(root).unwrap();
    }
    #[cfg(unix)]
    #[test]
    fn negative_symlink_is_a_miss() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        let real = root.join("real.json");
        negative_save_at(&real, "k", now());
        let link = root.join("n-link.json");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(negative_fresh(&real, "k"));
        assert!(!negative_fresh(&link, "k"));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn negative_and_positive_entries_share_the_file_bound() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        for i in 0..MAX_FILES {
            fs::write(root.join(format!("{i}.json")), b"{}").unwrap();
        }
        let path = negative_path(&root.join("x.json"), "k").unwrap();
        negative_save(&path, "k");
        assert!(negative_fresh(&path, "k"));
        assert_eq!(fs::read_dir(&root).unwrap().count(), MAX_FILES);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn source_entries_are_separate_per_source_and_key_bound() {
        let root = root();
        let (a, b) = (
            source_path(&root, "netease", "k1"),
            source_path(&root, "qq", "k1"),
        );
        assert_ne!(a, b);
        assert_ne!(a, source_path(&root, "netease", "k2"));
        let entry = SourceEntry {
            key: "k1".into(),
            text: Some("fixture".into()),
            lines: None,
            instrumental: false,
            timing_mismatch: false,
            duration_ms: 180_000,
        };
        source_save(&a, &entry);
        assert!(source_load(&a, "k1").is_some());
        assert!(source_load(&a, "other").is_none(), "key mismatch is a miss");
        assert!(source_load(&b, "k1").is_none(), "other source has no entry");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(&a).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn pruning_bounds_disk_usage() {
        let root = root();
        fs::create_dir_all(&root).unwrap();
        for i in 0..MAX_FILES {
            fs::write(root.join(format!("{i}.json")), b"{}").unwrap();
        }
        save(&root.join("new.json"), &record());
        assert_eq!(fs::read_dir(&root).unwrap().count(), MAX_FILES);
        fs::remove_dir_all(root).unwrap();
    }
}
