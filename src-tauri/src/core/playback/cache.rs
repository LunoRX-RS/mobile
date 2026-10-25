//! Device-local media cache. Only complete files enter the catalog; playlist pins share media.
use super::{
    audio::AudioQuality,
    diag::stage_log,
    extract::{self, ResolvedAudio},
};
use crate::core::{
    errors::{HostRejectKind, PlaybackErrorKind},
    yt::Yt,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::AsyncWriteExt,
    sync::{Mutex as AsyncMutex, Semaphore},
    task::JoinSet,
};

const MAX_TRACK: u64 = 64 * 1024 * 1024;
const DEFAULT_LIMIT: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum CacheError {
    Validation,
    Storage,
    Full,
    TooLarge,
    Download,
    Busy,
    Unavailable,
    InUse,
    /// YouTube asked for a sign-in or verification instead of giving a stream.
    VerificationRequired,
    /// The media host refused the audio bytes themselves; without a signed-in session only part of a stream is served.
    MediaRefused,
}
type Result<T> = std::result::Result<T, CacheError>;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineTrack {
    pub provider: String,
    pub id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Record {
    track: OfflineTrack,
    quality: String,
    size: u64,
    used: u64,
}
#[derive(Clone, Serialize, Deserialize)]
struct Pinned {
    name: String,
    tracks: Vec<OfflineTrack>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Catalog {
    limit: u64,
    records: HashMap<String, Record>,
    playlists: HashMap<String, Pinned>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            limit: DEFAULT_LIMIT,
            records: HashMap::new(),
            playlists: HashMap::new(),
        }
    }
}
struct Inner {
    catalog: Catalog,
    leases: HashMap<String, Weak<CachedFile>>,
}
pub struct CachedFile {
    pub path: PathBuf,
    pub size: u64,
}
pub struct DiskCache {
    root: PathBuf,
    _lock: fs::File,
    inner: Mutex<Inner>,
    locks: Mutex<HashMap<String, Weak<AsyncMutex<()>>>>,
    downloads: Semaphore,
}
fn quality(q: AudioQuality) -> &'static str {
    match q {
        AudioQuality::Low => "low",
        AudioQuality::Normal => "normal",
        AudioQuality::High => "high",
    }
}
// Preserve YouTube ID case even on case-insensitive filesystems.
fn disk_id(id: &str) -> String {
    id.bytes().map(|b| format!("{b:02x}")).collect()
}
fn key(id: &str, q: AudioQuality) -> String {
    format!("{}-{}", disk_id(id), quality(q))
}
fn valid_track(t: &OfflineTrack) -> bool {
    ["YouTube", "Spotify"].contains(&t.provider.as_str())
        && !t.id.is_empty()
        && t.id.len() <= 256
        && (t.provider != "YouTube" || extract::valid_id(&t.id))
        && t.title.len() <= 2048
        && t.artists.len() <= 30
        && t.artists.iter().all(|a| a.len() <= 512)
        && t.artwork_url.as_ref().is_none_or(|u| u.len() <= 2048)
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
fn storage<T>(_: T) -> CacheError {
    CacheError::Storage
}
fn file_size(path: &Path) -> Option<u64> {
    fs::symlink_metadata(path)
        .ok()
        .filter(|m| m.file_type().is_file())
        .map(|m| m.len())
}
fn dir_bytes(dir: &Path, extension: &str) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == extension)
            && let Some(size) = file_size(&path)
        {
            total += size;
        }
    }
    total
}
fn pinned(c: &Catalog, id: &str) -> bool {
    c.playlists.values().any(|p| {
        p.tracks
            .iter()
            .any(|t| t.provider == "YouTube" && t.id == id)
    })
}
struct Partial(PathBuf);
impl Drop for Partial {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

impl DiskCache {
    pub fn open(root: PathBuf) -> Result<Arc<Self>> {
        fs::create_dir_all(&root).map_err(storage)?;
        if !fs::symlink_metadata(&root)
            .map_err(storage)?
            .file_type()
            .is_dir()
        {
            return Err(CacheError::Storage);
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).map_err(storage)?;
        }
        let lock = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join("cache.lock"))
            .map_err(storage)?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => CacheError::InUse,
            std::fs::TryLockError::Error(_) => CacheError::Storage,
        })?;
        let index = root.join("catalog.json");
        let mut catalog: Catalog = if index.exists() {
            if file_size(&index).is_none_or(|n| n > 16 * 1024 * 1024) {
                return Err(CacheError::Storage);
            }
            serde_json::from_slice(&fs::read(&index).map_err(storage)?).map_err(storage)?
        } else {
            Catalog::default()
        };
        if catalog.records.len() > 10000
            || catalog.playlists.len() > 1000
            || !(64 * 1024 * 1024..=20 * 1024 * 1024 * 1024).contains(&catalog.limit)
        {
            return Err(CacheError::Storage);
        }
        if catalog
            .playlists
            .values()
            .any(|p| p.tracks.len() > 5000 || p.tracks.iter().any(|t| !valid_track(t)))
        {
            return Err(CacheError::Storage);
        }
        // Upgrade the initial development cache names without discarding downloaded audio.
        let old_records = std::mem::take(&mut catalog.records);
        for (k, record) in old_records {
            if !extract::valid_id(&record.track.id)
                || !["low", "normal", "high"].contains(&record.quality.as_str())
            {
                continue;
            }
            let encoded = format!("{}-{}", disk_id(&record.track.id), record.quality);
            let legacy = format!("{}-{}", record.track.id, record.quality);
            if k != encoded && k != legacy {
                continue;
            }
            let target = root.join(format!("{encoded}.audio"));
            if k == legacy
                && !target.exists()
                && file_size(&root.join(format!("{legacy}.audio"))) == Some(record.size)
            {
                fs::rename(root.join(format!("{legacy}.audio")), &target).map_err(storage)?;
            }
            let old_cover = root.join(format!("{}.jpg", record.track.id));
            let new_cover = root.join(format!("{}.jpg", disk_id(&record.track.id)));
            if old_cover.is_file() && !new_cover.exists() {
                fs::rename(old_cover, new_cover).map_err(storage)?;
            }
            catalog.records.insert(encoded, record);
        }
        catalog.records.retain(|k, r| {
            r.track.provider == "YouTube"
                && valid_track(&r.track)
                && extract::valid_id(&r.track.id)
                && ["low", "normal", "high"].contains(&r.quality.as_str())
                && *k == format!("{}-{}", disk_id(&r.track.id), r.quality)
                && r.size > 0
                && r.size <= MAX_TRACK
                && file_size(&root.join(format!("{k}.audio"))) == Some(r.size)
        });
        let cover_ids: HashSet<_> = catalog
            .records
            .values()
            .map(|r| disk_id(&r.track.id))
            .chain(
                catalog
                    .playlists
                    .values()
                    .flat_map(|p| p.tracks.iter().map(|t| disk_id(&t.id))),
            )
            .collect();
        for entry in fs::read_dir(&root).map_err(storage)? {
            let entry = entry.map_err(storage)?;
            if entry.file_type().map_err(storage)?.is_file()
                && (entry.path().extension().is_some_and(|e| e == "part")
                    || (entry.path().extension().is_some_and(|e| e == "jpg")
                        && entry
                            .path()
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| {
                                s.len() == 22
                                    && s.bytes().all(|b| b.is_ascii_hexdigit())
                                    && !cover_ids.contains(s)
                            }))
                    || (entry.path().extension().is_some_and(|e| e == "audio")
                        && entry
                            .path()
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .is_some_and(|s| !catalog.records.contains_key(s))))
            {
                fs::remove_file(entry.path()).map_err(storage)?;
            }
        }
        let cache = Arc::new(Self {
            root,
            _lock: lock,
            inner: Mutex::new(Inner {
                catalog,
                leases: HashMap::new(),
            }),
            locks: Mutex::new(HashMap::new()),
            downloads: Semaphore::new(2),
        });
        {
            let mut inner = cache.inner.lock().map_err(storage)?;
            cache.prune(&mut inner, 0)?;
            cache.save(&inner.catalog)?;
        }
        Ok(cache)
    }
    fn save(&self, catalog: &Catalog) -> Result<()> {
        let part = Partial(
            self.root
                .join(format!("catalog-{:032x}.part", rand::random::<u128>())),
        );
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&part.0).map_err(storage)?;
        use std::io::Write;
        let data = serde_json::to_vec(catalog).map_err(storage)?;
        if data.len() > 16 * 1024 * 1024 {
            return Err(CacheError::Full);
        }
        file.write_all(&data).map_err(storage)?;
        file.sync_all().map_err(storage)?;
        fs::rename(&part.0, self.root.join("catalog.json")).map_err(storage)?;
        Ok(())
    }
    fn prune(&self, inner: &mut Inner, incoming: u64) -> Result<()> {
        let total: u64 = inner.catalog.records.values().map(|r| r.size).sum();
        let mut needed = total
            .saturating_add(incoming)
            .saturating_sub(inner.catalog.limit);
        let protected: HashSet<_> = inner
            .catalog
            .playlists
            .values()
            .flat_map(|p| p.tracks.iter())
            .filter(|t| t.provider == "YouTube")
            .map(|t| t.id.as_str())
            .collect();
        let mut victims: Vec<_> = inner
            .catalog
            .records
            .iter()
            .filter(|(k, r)| {
                !protected.contains(r.track.id.as_str())
                    && inner.leases.get(*k).and_then(Weak::upgrade).is_none()
            })
            .map(|(k, r)| (k.clone(), r.used, r.size))
            .collect();
        victims.sort_by_key(|v| v.1);
        if victims.iter().map(|v| v.2).sum::<u64>() < needed {
            return Err(CacheError::Full);
        }
        for (k, _, size) in victims {
            if needed == 0 {
                break;
            }
            fs::remove_file(self.root.join(format!("{k}.audio"))).map_err(storage)?;
            if let Some(record) = inner.catalog.records.remove(&k)
                && !inner
                    .catalog
                    .records
                    .values()
                    .any(|r| r.track.id == record.track.id)
                && !pinned(&inner.catalog, &record.track.id)
            {
                let _ =
                    fs::remove_file(self.root.join(format!("{}.jpg", disk_id(&record.track.id))));
            }
            inner.leases.remove(&k);
            needed = needed.saturating_sub(size);
        }
        Ok(())
    }
    pub fn get(&self, id: &str, q: AudioQuality) -> Result<Option<Arc<CachedFile>>> {
        if !extract::valid_id(id) {
            return Err(CacheError::Validation);
        }
        let mut inner = self.inner.lock().map_err(storage)?;
        let exact = key(id, q);
        let selected = if inner.catalog.records.contains_key(&exact) {
            Some(exact)
        } else {
            inner
                .catalog
                .records
                .iter()
                .find(|(_, r)| r.track.id == id && pinned(&inner.catalog, id))
                .map(|(k, _)| k.clone())
        };
        let Some(k) = selected else { return Ok(None) };
        let record = inner
            .catalog
            .records
            .get_mut(&k)
            .ok_or(CacheError::Storage)?;
        let path = self.root.join(format!("{k}.audio"));
        if file_size(&path) != Some(record.size) {
            inner.catalog.records.remove(&k);
            self.save(&inner.catalog)?;
            return Ok(None);
        }
        record.used = now();
        let size = record.size;
        if let Some(file) = inner.leases.get(&k).and_then(Weak::upgrade) {
            return Ok(Some(file));
        }
        let file = Arc::new(CachedFile { path, size });
        inner.leases.insert(k, Arc::downgrade(&file));
        self.save(&inner.catalog)?;
        Ok(Some(file))
    }
    pub fn metadata(&self, id: &str) -> Option<OfflineTrack> {
        let inner = self.inner.lock().ok()?;
        let mut track = inner
            .catalog
            .playlists
            .values()
            .flat_map(|p| &p.tracks)
            .find(|t| t.id == id)
            .cloned()
            .or_else(|| {
                inner
                    .catalog
                    .records
                    .values()
                    .find(|r| r.track.id == id)
                    .map(|r| r.track.clone())
            })?;
        track.artwork_url = self.cover(id).or(track.artwork_url);
        Some(track)
    }
    pub fn cover(&self, id: &str) -> Option<String> {
        if !extract::valid_id(id) {
            return None;
        }
        let path = self.root.join(format!("{}.jpg", disk_id(id)));
        if file_size(&path).is_none_or(|n| n > 512 * 1024) {
            return None;
        }
        let bytes = fs::read(path).ok()?;
        if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
            return None;
        }
        Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(bytes)))
    }
    pub async fn ensure(
        &self,
        yt: &Yt,
        mut track: OfflineTrack,
        q: AudioQuality,
    ) -> Result<Arc<CachedFile>> {
        if track.provider != "YouTube" || !valid_track(&track) {
            return Err(CacheError::Validation);
        }
        track.artwork_url = track
            .artwork_url
            .as_deref()
            .and_then(super::track_info::artwork_url);
        if let Some(file) = self.get(&track.id, q)? {
            return Ok(file);
        }
        let k = key(&track.id, q);
        let lock = {
            let mut locks = self.locks.lock().map_err(storage)?;
            locks.retain(|_, v| v.strong_count() > 0);
            if let Some(l) = locks.get(&k).and_then(Weak::upgrade) {
                l
            } else {
                let l = Arc::new(AsyncMutex::new(()));
                locks.insert(k.clone(), Arc::downgrade(&l));
                l
            }
        };
        let _guard = lock.lock().await;
        if let Some(file) = self.get(&track.id, q)? {
            return Ok(file);
        }
        let _permit = self.downloads.acquire().await.map_err(storage)?;
        let audio = extract::resolve_with_quality(yt, &track.id, q)
            .await
            .map_err(|kind| match kind {
                PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired) => {
                    CacheError::VerificationRequired
                }
                _ => CacheError::Download,
            })?;
        if audio.size > MAX_TRACK {
            stage_log!(
                "[debug] cache ensure: {} bytes over the {} limit",
                audio.size,
                MAX_TRACK
            );
            return Err(CacheError::TooLarge);
        }
        if audio.size == 0 {
            stage_log!(
                "[debug] cache ensure: resolved size is zero for {}",
                track.id
            );
            return Err(CacheError::Download);
        }
        {
            let mut inner = self.inner.lock().map_err(storage)?;
            if inner.catalog.records.len() >= 10000 {
                return Err(CacheError::Full);
            }
            self.prune(&mut inner, audio.size)?;
            self.save(&inner.catalog)?;
        }
        let part = Partial(
            self.root
                .join(format!("{k}-{:032x}.part", rand::random::<u128>())),
        );
        if let Err(e) = download(&audio, &part.0).await {
            stage_log!(
                "[debug] cache ensure: download failed for {}: {e:?}",
                track.id
            );
            return Err(e);
        }
        if let Some(bytes) = artwork(&track.id).await {
            let image = Partial(self.root.join(format!(
                "{}-{:032x}.part",
                track.id,
                rand::random::<u128>()
            )));
            if fs::write(&image.0, bytes).is_ok() {
                let _ = fs::rename(
                    &image.0,
                    self.root.join(format!("{}.jpg", disk_id(&track.id))),
                );
            }
        }
        let mut inner = self.inner.lock().map_err(storage)?;
        self.prune(&mut inner, audio.size)?;
        let path = self.root.join(format!("{k}.audio"));
        fs::rename(&part.0, &path).map_err(storage)?;
        inner.catalog.records.insert(
            k.clone(),
            Record {
                track,
                quality: quality(q).into(),
                size: audio.size,
                used: now(),
            },
        );
        self.save(&inner.catalog)?;
        let file = Arc::new(CachedFile {
            path,
            size: audio.size,
        });
        inner.leases.insert(k, Arc::downgrade(&file));
        Ok(file)
    }
    pub fn set_limit(&self, bytes: u64) -> Result<()> {
        if !(64 * 1024 * 1024..=20 * 1024 * 1024 * 1024).contains(&bytes) {
            return Err(CacheError::Validation);
        }
        let mut inner = self.inner.lock().map_err(storage)?;
        let old = inner.catalog.limit;
        inner.catalog.limit = bytes;
        if let Err(e) = self.prune(&mut inner, 0) {
            inner.catalog.limit = old;
            return Err(e);
        }
        if let Err(e) = self.save(&inner.catalog) {
            inner.catalog.limit = old;
            return Err(e);
        }
        Ok(())
    }
    pub fn pin(&self, id: &str, name: String, tracks: Vec<OfflineTrack>) -> Result<()> {
        if id.is_empty()
            || id.len() > 256
            || tracks.len() > 5000
            || tracks.iter().any(|t| !valid_track(t))
        {
            return Err(CacheError::Validation);
        }
        let mut inner = self.inner.lock().map_err(storage)?;
        if !inner.catalog.playlists.contains_key(id) && inner.catalog.playlists.len() >= 1000 {
            return Err(CacheError::Full);
        }
        if inner
            .catalog
            .playlists
            .iter()
            .filter(|(k, _)| k.as_str() != id)
            .map(|(_, p)| p.tracks.len())
            .sum::<usize>()
            + tracks.len()
            > 10000
        {
            return Err(CacheError::Full);
        }
        let old = inner
            .catalog
            .playlists
            .insert(id.into(), Pinned { name, tracks });
        if let Err(e) = self.save(&inner.catalog) {
            if let Some(old) = old {
                inner.catalog.playlists.insert(id.into(), old);
            } else {
                inner.catalog.playlists.remove(id);
            }
            return Err(e);
        }
        Ok(())
    }
    pub fn unpin(&self, id: &str) -> Result<()> {
        let mut inner = self.inner.lock().map_err(storage)?;
        if let Some(p) = inner.catalog.playlists.remove(id) {
            let ids: HashSet<_> = p.tracks.into_iter().map(|t| t.id).collect();
            let victims: Vec<_> = inner
                .catalog
                .records
                .iter()
                .filter(|(k, r)| {
                    ids.contains(&r.track.id)
                        && !pinned(&inner.catalog, &r.track.id)
                        && inner.leases.get(*k).and_then(Weak::upgrade).is_none()
                })
                .map(|(k, _)| k.clone())
                .collect();
            for k in victims {
                fs::remove_file(self.root.join(format!("{k}.audio"))).map_err(storage)?;
                if let Some(record) = inner.catalog.records.remove(&k)
                    && !inner
                        .catalog
                        .records
                        .values()
                        .any(|r| r.track.id == record.track.id)
                {
                    let _ = fs::remove_file(
                        self.root.join(format!("{}.jpg", disk_id(&record.track.id))),
                    );
                }
                inner.leases.remove(&k);
            }
        }
        self.save(&inner.catalog)
    }
    /// Frees every cached song that no playlist keeps and nothing is playing, and returns the bytes
    /// released. Kept and in-play files are untouched, exactly as LRU pruning leaves them.
    pub fn clear_transient(&self) -> Result<u64> {
        let mut inner = self.inner.lock().map_err(storage)?;
        let protected: HashSet<&str> = inner
            .catalog
            .playlists
            .values()
            .flat_map(|p| p.tracks.iter())
            .filter(|t| t.provider == "YouTube")
            .map(|t| t.id.as_str())
            .collect();
        let victims: Vec<_> = inner
            .catalog
            .records
            .iter()
            .filter(|(k, r)| {
                !protected.contains(r.track.id.as_str())
                    && inner.leases.get(*k).and_then(Weak::upgrade).is_none()
            })
            .map(|(k, r)| (k.clone(), r.size))
            .collect();
        let mut freed = 0;
        for (k, size) in victims {
            fs::remove_file(self.root.join(format!("{k}.audio"))).map_err(storage)?;
            if let Some(record) = inner.catalog.records.remove(&k)
                && !inner
                    .catalog
                    .records
                    .values()
                    .any(|r| r.track.id == record.track.id)
            {
                let _ =
                    fs::remove_file(self.root.join(format!("{}.jpg", disk_id(&record.track.id))));
            }
            inner.leases.remove(&k);
            freed += size;
        }
        self.save(&inner.catalog)?;
        Ok(freed)
    }
    /// What the cache occupies: audio from the catalog, artwork and lyrics from the two directories
    /// they live in. A song kept by two playlists is one file, so its bytes appear in both rows and
    /// `shared_bytes` names the overlap; the playlist list is never a partition of `audio_bytes`.
    /// The scans are bounded by the invariants of `open` (orphans are deleted) and `ensure` (at most
    /// 10 000 records, and the lyric cache holds at most 256 files).
    pub fn usage(&self, lyric_dir: Option<&Path>) -> Result<CacheUsage> {
        let (audio_bytes, limit_bytes, clearable_bytes, shared_bytes, playlists) = {
            let inner = self.inner.lock().map_err(storage)?;
            let protected: HashSet<&str> = inner
                .catalog
                .playlists
                .values()
                .flat_map(|p| p.tracks.iter())
                .filter(|t| t.provider == "YouTube")
                .map(|t| t.id.as_str())
                .collect();
            let audio_bytes: u64 = inner.catalog.records.values().map(|r| r.size).sum();
            let clearable_bytes: u64 = inner
                .catalog
                .records
                .iter()
                .filter(|(k, r)| {
                    !protected.contains(r.track.id.as_str())
                        && inner.leases.get(*k).and_then(Weak::upgrade).is_none()
                })
                .map(|(_, r)| r.size)
                .sum();
            // Distinct playlists naming each track, so a duplicate inside one playlist is not "shared".
            let mut keepers: HashMap<&str, usize> = HashMap::new();
            for pinned in inner.catalog.playlists.values() {
                let mut seen: HashSet<&str> = HashSet::new();
                for t in &pinned.tracks {
                    if t.provider == "YouTube" && seen.insert(t.id.as_str()) {
                        *keepers.entry(t.id.as_str()).or_default() += 1;
                    }
                }
            }
            let shared_bytes: u64 = inner
                .catalog
                .records
                .values()
                .filter(|r| keepers.get(r.track.id.as_str()).copied().unwrap_or(0) > 1)
                .map(|r| r.size)
                .sum();
            let mut playlists: Vec<PlaylistUsage> = inner
                .catalog
                .playlists
                .iter()
                .map(|(id, p)| {
                    let names: HashSet<&str> = p.tracks.iter().map(|t| t.id.as_str()).collect();
                    PlaylistUsage {
                        id: id.clone(),
                        name: p.name.clone(),
                        tracks: p.tracks.len(),
                        bytes: inner
                            .catalog
                            .records
                            .values()
                            .filter(|r| names.contains(r.track.id.as_str()))
                            .map(|r| r.size)
                            .sum(),
                    }
                })
                .collect();
            playlists.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.name.cmp(&b.name)));
            (
                audio_bytes,
                inner.catalog.limit,
                clearable_bytes,
                shared_bytes,
                playlists,
            )
        };
        let artwork_bytes = dir_bytes(&self.root, "jpg");
        let lyric_bytes = lyric_dir.map(|dir| dir_bytes(dir, "json")).unwrap_or(0);
        Ok(CacheUsage {
            audio_bytes,
            artwork_bytes,
            lyric_bytes,
            total_bytes: audio_bytes + artwork_bytes + lyric_bytes,
            limit_bytes,
            clearable_bytes,
            shared_bytes,
            playlists,
        })
    }
    pub fn status(&self, id: &str) -> Result<OfflineStatus> {
        let inner = self.inner.lock().map_err(storage)?;
        let playlist = inner.catalog.playlists.get(id);
        let wanted: HashSet<_> = playlist
            .into_iter()
            .flat_map(|p| p.tracks.iter())
            .map(|t| t.id.as_str())
            .collect();
        let ready: HashSet<_> = inner
            .catalog
            .records
            .iter()
            .filter(|(k, r)| {
                wanted.contains(r.track.id.as_str())
                    && file_size(&self.root.join(format!("{k}.audio"))) == Some(r.size)
            })
            .map(|(_, r)| r.track.id.as_str())
            .collect();
        let protected: HashSet<_> = inner
            .catalog
            .playlists
            .values()
            .flat_map(|p| p.tracks.iter())
            .filter(|t| t.provider == "YouTube")
            .map(|t| t.id.as_str())
            .collect();
        let tracks = playlist
            .map(|p| {
                p.tracks
                    .iter()
                    .map(|t| TrackStatus {
                        track: t.clone(),
                        error: None,
                        state: if t.provider != "YouTube" {
                            "unsupported"
                        } else if ready.contains(t.id.as_str()) {
                            "ready"
                        } else {
                            "pending"
                        }
                        .into(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(OfflineStatus {
            changed: false,
            keeping: playlist.is_some(),
            running: false,
            tracks,
            used_bytes: inner.catalog.records.values().map(|r| r.size).sum(),
            pinned_bytes: inner
                .catalog
                .records
                .values()
                .filter(|r| protected.contains(r.track.id.as_str()))
                .map(|r| r.size)
                .sum(),
            limit_bytes: inner.catalog.limit,
        })
    }
}

async fn download(audio: &ResolvedAudio, path: &Path) -> Result<()> {
    let url = reqwest::Url::parse(&audio.url).map_err(|_| CacheError::Download)?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .connect_timeout(Duration::from_secs(8))
        .redirect(reqwest::redirect::Policy::custom(|a| {
            if a.previous().len() < 3
                && a.url().scheme() == "https"
                && a.url()
                    .host_str()
                    .is_some_and(|h| h.ends_with(".googlevideo.com"))
            {
                a.follow()
            } else {
                a.stop()
            }
        }))
        .build()
        .map_err(|_| CacheError::Download)?;
    stage_log!(
        "[debug] download: host={} size={}",
        url.host_str().unwrap_or("-"),
        audio.size
    );
    // googlevideo answers a plain GET with 403; the byte span must arrive as the standard Range header,
    // not the old `range=` query parameter.
    let mut response = client
        .get(url)
        .header("range", format!("bytes=0-{}", audio.size - 1))
        .send()
        .await
        .map_err(|e| {
            stage_log!("[debug] download: send failed: {}", reqwest_failure(&e));
            CacheError::Download
        })?;
    if !response.status().is_success() {
        let status = response.status().as_u16();
        stage_log!("[debug] download: http {status}");
        if matches!(status, 401 | 403 | 404 | 410) {
            return Err(CacheError::MediaRefused);
        }
        return Err(CacheError::Download);
    }
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = tokio::fs::File::from_std(options.open(path).map_err(storage)?);
    let mut count = 0u64;
    while let Some(chunk) = response.chunk().await.map_err(|e| {
        stage_log!(
            "[debug] download: chunk failed at {count} bytes: {}",
            reqwest_failure(&e)
        );
        CacheError::Download
    })? {
        count += chunk.len() as u64;
        if count > audio.size {
            stage_log!("[debug] download: overshoot {count} > {}", audio.size);
            return Err(CacheError::Download);
        }
        file.write_all(&chunk).await.map_err(storage)?;
    }
    if count != audio.size {
        stage_log!("[debug] download: short {count} of {} bytes", audio.size);
        return Err(CacheError::Download);
    }
    file.sync_all().await.map_err(storage)?;
    stage_log!("[debug] download: complete {count} bytes");
    Ok(())
}

/// Identifies a failed request without its signed URL: host, failure class and status only.
fn reqwest_failure(e: &reqwest::Error) -> String {
    format!(
        "host={} connect={} timeout={} status={}",
        e.url().and_then(|u| u.host_str()).unwrap_or("-"),
        e.is_connect(),
        e.is_timeout(),
        e.status()
            .map_or("-".to_string(), |s| s.as_u16().to_string())
    )
}
async fn artwork(id: &str) -> Option<Vec<u8>> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .ok()?;
    let mut response = client
        .get(format!("https://i.ytimg.com/vi/{id}/hqdefault.jpg"))
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mut bytes = Vec::new();
    while let Some(part) = response.chunk().await.ok()? {
        if bytes.len() + part.len() > 512 * 1024 {
            return None;
        }
        bytes.extend_from_slice(&part);
    }
    if !bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return None;
    }
    Some(bytes)
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackStatus {
    pub track: OfflineTrack,
    pub error: Option<CacheError>,
    pub state: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OfflineStatus {
    pub changed: bool,
    pub keeping: bool,
    pub running: bool,
    pub tracks: Vec<TrackStatus>,
    pub used_bytes: u64,
    pub pinned_bytes: u64,
    pub limit_bytes: u64,
}
/// One playlist kept offline, and the bytes its songs occupy. Songs shared with another playlist are
/// counted here too, so these rows overlap rather than add up to `CacheUsage::audio_bytes`.
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaylistUsage {
    pub id: String,
    pub name: String,
    pub tracks: usize,
    pub bytes: u64,
}
#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CacheUsage {
    pub audio_bytes: u64,
    pub artwork_bytes: u64,
    pub lyric_bytes: u64,
    pub total_bytes: u64,
    pub limit_bytes: u64,
    /// Audio no playlist keeps and no player holds a lease on: what clearing the cache would free now.
    pub clearable_bytes: u64,
    pub shared_bytes: u64,
    pub playlists: Vec<PlaylistUsage>,
}
struct Job {
    id: String,
    generation: u64,
    active: bool,
    failed: HashMap<String, CacheError>,
    cancel: Arc<tokio::sync::Notify>,
}
pub struct OfflineManager {
    pub cache: Arc<DiskCache>,
    job: Mutex<Option<Job>>,
    notify: Arc<dyn Fn() + Send + Sync>,
}
impl OfflineManager {
    pub fn new(cache: Arc<DiskCache>, notify: Arc<dyn Fn() + Send + Sync>) -> Arc<Self> {
        Arc::new(Self {
            cache,
            job: Mutex::new(None),
            notify,
        })
    }
    pub fn status(&self, id: &str) -> Result<OfflineStatus> {
        let mut status = self.cache.status(id)?;
        let job = self.job.lock().map_err(storage)?;
        if let Some(job) = job.as_ref().filter(|j| j.id == id) {
            status.running = job.active;
            for t in &mut status.tracks {
                if t.state != "ready"
                    && let Some(error) = job.failed.get(&t.track.id)
                {
                    t.state = "failed".into();
                    t.error = Some(*error);
                }
            }
        }
        Ok(status)
    }
    pub fn start(
        self: &Arc<Self>,
        yt: Yt,
        id: String,
        name: String,
        tracks: Vec<OfflineTrack>,
        q: AudioQuality,
    ) -> Result<()> {
        let mut job = self.job.lock().map_err(storage)?;
        if job.as_ref().is_some_and(|j| j.active) {
            return Err(CacheError::Busy);
        }
        self.cache.pin(&id, name, tracks.clone())?;
        let generation = rand::random::<u64>();
        let cancel = Arc::new(tokio::sync::Notify::new());
        *job = Some(Job {
            id: id.clone(),
            generation,
            active: true,
            failed: HashMap::new(),
            cancel: cancel.clone(),
        });
        drop(job);
        let manager = self.clone();
        tokio::spawn(async move {
            let work = async {
                let unique: HashMap<_, _> = tracks
                    .into_iter()
                    .filter(|t| t.provider == "YouTube")
                    .map(|t| (t.id.clone(), t))
                    .collect();
                let tracks: Vec<_> = unique.into_values().collect();
                for chunk in tracks.chunks(2) {
                    let mut tasks = JoinSet::new();
                    for track in chunk {
                        let cache = manager.cache.clone();
                        let yt = yt.clone();
                        let track = track.clone();
                        tasks.spawn(async move {
                            let id = track.id.clone();
                            (id, cache.ensure(&yt, track, q).await.map(|_| ()))
                        });
                    }
                    while let Some(result) = tasks.join_next().await {
                        if let Ok((id, Err(error))) = result
                            && let Ok(mut job) = manager.job.lock()
                            && let Some(job) = job.as_mut().filter(|j| j.generation == generation)
                        {
                            job.failed.insert(id, error);
                        }
                        (manager.notify)();
                    }
                }
            };
            tokio::select! {_ = work => {}, _ = cancel.notified() => {}}
            if let Ok(mut job) = manager.job.lock()
                && let Some(job) = job.as_mut().filter(|j| j.generation == generation)
            {
                job.active = false;
            }
            (manager.notify)();
        });
        (self.notify)();
        Ok(())
    }
    pub fn cancel(&self, id: &str) -> Result<()> {
        let job = self.job.lock().map_err(storage)?;
        if let Some(j) = job.as_ref().filter(|j| j.id == id) {
            j.cancel.notify_one();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            Self(
                std::env::temp_dir()
                    .join(format!("lunor-cache-test-{:032x}", rand::random::<u128>())),
            )
        }
        fn open(&self) -> Arc<DiskCache> {
            DiskCache::open(self.0.clone()).unwrap()
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn cache_contention_recovers_after_owner_exits() {
        let root = Fixture::new();
        let first = root.open();
        assert!(matches!(
            DiskCache::open(root.0.clone()),
            Err(CacheError::InUse)
        ));
        drop(first);
        drop(root.open());
    }
    fn track(id: &str) -> OfflineTrack {
        OfflineTrack {
            provider: "YouTube".into(),
            id: id.into(),
            title: format!("Track {id}"),
            ..Default::default()
        }
    }
    fn seed(cache: &DiskCache, id: &str, size: u64, used: u64) {
        let k = key(id, AudioQuality::High);
        let file = fs::File::create(cache.root.join(format!("{k}.audio"))).unwrap();
        file.set_len(size).unwrap();
        let mut inner = cache.inner.lock().unwrap();
        inner.catalog.records.insert(
            k,
            Record {
                track: track(id),
                quality: "high".into(),
                size,
                used,
            },
        );
        cache.save(&inner.catalog).unwrap();
    }
    #[test]
    fn pins_share_files_survive_restart_and_last_unpin_removes_only_owned_copy() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "r-G2pKr2_n8", 123, 1);
        cache
            .pin(
                "one",
                "First".into(),
                vec![track("r-G2pKr2_n8"), track("r-G2pKr2_n8")],
            )
            .unwrap();
        cache
            .pin("two", "Second".into(), vec![track("r-G2pKr2_n8")])
            .unwrap();
        assert_eq!(cache.status("one").unwrap().used_bytes, 123);
        drop(cache);
        let cache = dir.open();
        assert_eq!(cache.status("one").unwrap().tracks.len(), 2);
        cache.unpin("one").unwrap();
        assert!(
            cache
                .get("r-G2pKr2_n8", AudioQuality::High)
                .unwrap()
                .is_some()
        );
        cache.unpin("two").unwrap();
        assert!(
            cache
                .get("r-G2pKr2_n8", AudioQuality::High)
                .unwrap()
                .is_none()
        );
    }
    #[test]
    fn lru_preserves_pinned_and_playing_files_and_refuses_impossible_limit() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "aaaaaaaaaaa", 32 * 1024 * 1024, 1);
        seed(&cache, "bbbbbbbbbbb", 32 * 1024 * 1024, 2);
        seed(&cache, "ccccccccccc", 32 * 1024 * 1024, 3);
        cache
            .pin("p", "Pinned".into(), vec![track("aaaaaaaaaaa")])
            .unwrap();
        let active = cache
            .get("bbbbbbbbbbb", AudioQuality::High)
            .unwrap()
            .unwrap();
        cache.set_limit(64 * 1024 * 1024).unwrap();
        assert!(
            cache
                .get("ccccccccccc", AudioQuality::High)
                .unwrap()
                .is_none()
        );
        let mut inner = cache.inner.lock().unwrap();
        assert!(matches!(cache.prune(&mut inner, 1), Err(CacheError::Full)));
        drop(inner);
        drop(active);
        let mut inner = cache.inner.lock().unwrap();
        cache.prune(&mut inner, 1).unwrap();
        assert!(
            !inner
                .catalog
                .records
                .contains_key(&key("bbbbbbbbbbb", AudioQuality::High))
        );
    }
    #[test]
    fn startup_removes_partial_and_orphan_files_and_rejects_truncated_cache_hits() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "aaaaaaaaaaa", 100, 1);
        drop(cache);
        fs::write(dir.0.join("unfinished.part"), [1, 2]).unwrap();
        fs::write(dir.0.join("orphan.audio"), [1]).unwrap();
        fs::OpenOptions::new()
            .write(true)
            .open(
                dir.0
                    .join(format!("{}.audio", key("aaaaaaaaaaa", AudioQuality::High))),
            )
            .unwrap()
            .set_len(1)
            .unwrap();
        let cache = dir.open();
        assert!(
            cache
                .get("aaaaaaaaaaa", AudioQuality::High)
                .unwrap()
                .is_none()
        );
        assert!(!dir.0.join("unfinished.part").exists());
        assert!(!dir.0.join("orphan.audio").exists());
        assert!(
            cache
                .pin("bad", "Bad".into(), vec![track("../unsafe")])
                .is_err()
        );
        assert!(DiskCache::open(dir.0.clone()).is_err());
    }
    #[tokio::test]
    async fn disk_hit_needs_no_network_and_download_requires_exact_bytes() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "aaaaaaaaaaa", 100, 1);
        let yt = Yt::new(Duration::from_nanos(1)).unwrap();
        assert_eq!(
            cache
                .ensure(&yt, track("aaaaaaaaaaa"), AudioQuality::High)
                .await
                .unwrap()
                .size,
            100
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let upstream = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut headers = Vec::new();
            while !headers.ends_with(b"\r\n\r\n") {
                headers.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nxx")
                .await
                .unwrap();
        });
        let part = Partial(dir.0.join("broken.part"));
        assert!(
            download(&ResolvedAudio { url, size: 10 }, &part.0)
                .await
                .is_err()
        );
        drop(part);
        upstream.await.unwrap();
        assert!(!dir.0.join("broken.part").exists());
    }
    #[tokio::test]
    async fn playlist_jobs_deduplicate_reuse_cache_skip_unsupported_and_cancel() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "aaaaaaaaaaa", 100, 1);
        let changed = Arc::new(tokio::sync::Notify::new());
        let callback = changed.clone();
        let manager = OfflineManager::new(cache, Arc::new(move || callback.notify_one()));
        let yt = Yt::new(Duration::from_nanos(1)).unwrap();
        let mut unsupported = track("spotify-id");
        unsupported.provider = "Spotify".into();
        manager
            .start(
                yt.clone(),
                "one".into(),
                "Playlist".into(),
                vec![track("aaaaaaaaaaa"), track("aaaaaaaaaaa"), unsupported],
                AudioQuality::High,
            )
            .unwrap();
        assert!(matches!(
            manager.start(
                yt.clone(),
                "two".into(),
                "Other".into(),
                vec![],
                AudioQuality::High
            ),
            Err(CacheError::Busy)
        ));
        tokio::time::timeout(Duration::from_secs(3), async {
            while manager.status("one").unwrap().running {
                changed.notified().await;
            }
        })
        .await
        .unwrap();
        let status = manager.status("one").unwrap();
        assert_eq!(
            status.tracks.iter().filter(|t| t.state == "ready").count(),
            2
        );
        assert_eq!(status.tracks[2].state, "unsupported");
        assert_eq!(status.used_bytes, 100);
        manager
            .start(
                yt,
                "cancel".into(),
                "Cancel".into(),
                vec![track("bbbbbbbbbbb")],
                AudioQuality::High,
            )
            .unwrap();
        manager.cancel("cancel").unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while manager.status("cancel").unwrap().running {
                changed.notified().await;
            }
        })
        .await
        .unwrap();
        let status = manager.status("cancel").unwrap();
        assert!(status.keeping);
        assert_eq!(status.tracks[0].state, "pending");
    }
    #[tokio::test]
    async fn cancelling_a_stalled_download_removes_its_partial_file() {
        use tokio::io::AsyncReadExt;
        let dir = Fixture::new();
        let _cache = dir.open();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\nConnection: close\r\n\r\nx")
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let path = dir.0.join("cancel.part");
        let task_path = path.clone();
        let worker = tokio::spawn(async move {
            let part = Partial(task_path);
            download(&ResolvedAudio { url, size: 4096 }, &part.0).await
        });
        tokio::time::timeout(Duration::from_secs(2), async {
            while !path.exists() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        worker.abort();
        let _ = worker.await;
        upstream.abort();
        assert!(!path.exists());
    }
    #[tokio::test]
    async fn a_refused_media_fetch_is_reported_as_a_sign_in_problem() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let dir = Fixture::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/audio", listener.local_addr().unwrap());
        let upstream = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut header = Vec::new();
            while !header.ends_with(b"\r\n\r\n") {
                header.push(socket.read_u8().await.unwrap());
            }
            assert!(
                String::from_utf8_lossy(&header)
                    .to_ascii_lowercase()
                    .contains("range: bytes=0-99\r\n")
            );
            socket
                .write_all(
                    b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .await
                .unwrap();
        });
        let path = dir.0.join("refused.part");
        let error = download(&ResolvedAudio { url, size: 100 }, &path)
            .await
            .unwrap_err();
        assert!(matches!(error, CacheError::MediaRefused));
        upstream.await.unwrap();
        assert!(!path.exists());
    }
    #[test]
    fn filenames_preserve_id_case_and_legacy_catalogs_upgrade_without_redownload() {
        let dir = Fixture::new();
        let cache = dir.open();
        seed(&cache, "AbCdEFghIJ1", 100, 1);
        seed(&cache, "abcdefghij1", 200, 2);
        let upper = cache
            .get("AbCdEFghIJ1", AudioQuality::High)
            .unwrap()
            .unwrap();
        let lower = cache
            .get("abcdefghij1", AudioQuality::High)
            .unwrap()
            .unwrap();
        assert_ne!(upper.path, lower.path);
        assert_eq!(upper.size, 100);
        assert_eq!(lower.size, 200);
        drop(upper);
        drop(lower);
        let mut inner = cache.inner.lock().unwrap();
        let encoded = key("AbCdEFghIJ1", AudioQuality::High);
        let record = inner.catalog.records.remove(&encoded).unwrap();
        let legacy = "AbCdEFghIJ1-high";
        fs::rename(
            cache.root.join(format!("{encoded}.audio")),
            cache.root.join(format!("{legacy}.audio")),
        )
        .unwrap();
        inner.catalog.records.insert(legacy.into(), record);
        cache.save(&inner.catalog).unwrap();
        drop(inner);
        drop(cache);
        let cache = dir.open();
        assert_eq!(
            cache
                .get("AbCdEFghIJ1", AudioQuality::High)
                .unwrap()
                .unwrap()
                .size,
            100
        );
        assert_eq!(
            cache
                .get("abcdefghij1", AudioQuality::High)
                .unwrap()
                .unwrap()
                .size,
            200
        );
    }
}
