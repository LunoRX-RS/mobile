//! Device-local play counts behind the listening statistics page. Nothing leaves the device unless the user turns cloud sync on.
use super::library::{LibraryError, SavedTrack};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::Mutex};

const MAX_TRACKS: usize = 5000;
const MAX_BYTES: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlayCount {
    pub provider: String,
    pub provider_track_id: String,
    pub title: String,
    pub artists: Vec<String>,
    pub artwork_url: Option<String>,
    pub plays: u32,
    pub last_played_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Saved {
    device_id: String,
    #[serde(default)]
    cloud: bool,
    #[serde(default)]
    tracks: Vec<PlayCount>,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatsView {
    pub device_id: String,
    pub cloud: bool,
    pub tracks: Vec<PlayCount>,
}

pub struct PlayStats {
    path: PathBuf,
    saved: Mutex<Saved>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// A random UUID v4 string, the form the backend expects for a device id.
fn new_device_id() -> String {
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

impl PlayStats {
    pub fn open(path: PathBuf) -> Result<Self, LibraryError> {
        let mut saved = if path.exists() {
            let meta = fs::symlink_metadata(&path).map_err(|_| LibraryError::Storage)?;
            if !meta.file_type().is_file() || meta.len() > MAX_BYTES {
                return Err(LibraryError::Storage);
            }
            serde_json::from_slice::<Saved>(&fs::read(&path).map_err(|_| LibraryError::Storage)?)
                .map_err(|_| LibraryError::Storage)?
        } else {
            Saved::default()
        };
        if saved.tracks.len() > MAX_TRACKS {
            return Err(LibraryError::Storage);
        }
        if saved.device_id.is_empty() {
            saved.device_id = new_device_id();
        }
        Ok(Self {
            path,
            saved: Mutex::new(saved),
        })
    }

    pub fn view(&self) -> Result<StatsView, LibraryError> {
        let s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        Ok(view_of(&s))
    }

    /// Counts one play. Called once per loaded track, the same moment the recent-history list is updated.
    pub fn record(&self, mut track: SavedTrack) -> Result<(), LibraryError> {
        track.validate()?;
        let provider = format!("{:?}", track.provider);
        let mut s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = s.clone();
        let at = now_ms();
        match next
            .tracks
            .iter_mut()
            .find(|t| t.provider == provider && t.provider_track_id == track.provider_track_id)
        {
            Some(t) => {
                t.plays = t.plays.saturating_add(1);
                t.last_played_ms = at;
                t.title = track.title;
                t.artists = track.artists;
                t.artwork_url = track.artwork_url.or(t.artwork_url.take());
            }
            None => {
                if next.tracks.len() >= MAX_TRACKS {
                    // Drop the least-played, oldest entry so the file stays bounded.
                    if let Some(i) = (0..next.tracks.len())
                        .min_by_key(|&i| (next.tracks[i].plays, next.tracks[i].last_played_ms))
                    {
                        next.tracks.swap_remove(i);
                    }
                }
                next.tracks.push(PlayCount {
                    provider,
                    provider_track_id: track.provider_track_id,
                    title: track.title,
                    artists: track.artists,
                    artwork_url: track.artwork_url,
                    plays: 1,
                    last_played_ms: at,
                });
            }
        }
        self.commit(&mut s, next)
    }

    pub fn set_cloud(&self, on: bool) -> Result<StatsView, LibraryError> {
        let mut s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = s.clone();
        next.cloud = on;
        self.commit(&mut s, next)?;
        Ok(view_of(&s))
    }

    pub fn clear(&self) -> Result<StatsView, LibraryError> {
        let mut s = self.saved.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = s.clone();
        next.tracks.clear();
        self.commit(&mut s, next)?;
        Ok(view_of(&s))
    }

    fn commit(&self, current: &mut Saved, next: Saved) -> Result<(), LibraryError> {
        let bytes = serde_json::to_vec(&next).map_err(|_| LibraryError::Storage)?;
        if bytes.len() as u64 > MAX_BYTES {
            return Err(LibraryError::Full);
        }
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
        *current = next;
        Ok(())
    }
}

fn view_of(s: &Saved) -> StatsView {
    let mut tracks = s.tracks.clone();
    tracks.sort_by(|a, b| {
        b.plays
            .cmp(&a.plays)
            .then(b.last_played_ms.cmp(&a.last_played_ms))
    });
    StatsView {
        device_id: s.device_id.clone(),
        cloud: s.cloud,
        tracks,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::ProviderId;

    fn track(id: &str) -> SavedTrack {
        SavedTrack {
            source: None,
            provider: ProviderId::YouTube,
            provider_track_id: id.into(),
            canonical_url: format!("https://www.youtube.com/watch?v={id}"),
            title: "Song".into(),
            artists: vec!["Artist".into()],
            artist_refs: vec![],
            artwork_url: None,
            duration_ms: None,
        }
    }

    fn path() -> PathBuf {
        std::env::temp_dir().join(format!("lunorx-stats-{:032x}.json", rand::random::<u128>()))
    }

    #[test]
    fn plays_are_counted_ordered_and_persist() {
        let p = path();
        let stats = PlayStats::open(p.clone()).unwrap();
        stats.record(track("aaaaaaaaaaa")).unwrap();
        stats.record(track("bbbbbbbbbbb")).unwrap();
        stats.record(track("bbbbbbbbbbb")).unwrap();
        let v = stats.view().unwrap();
        assert_eq!(v.tracks[0].provider_track_id, "bbbbbbbbbbb");
        assert_eq!(v.tracks[0].plays, 2);
        assert_eq!(v.tracks.len(), 2);
        assert_eq!(v.device_id.len(), 36);
        let again = PlayStats::open(p.clone()).unwrap();
        assert_eq!(again.view().unwrap().device_id, v.device_id);
        assert_eq!(again.view().unwrap().tracks.len(), 2);
        assert!(again.set_cloud(true).unwrap().cloud);
        assert!(again.clear().unwrap().tracks.is_empty());
        let _ = fs::remove_file(p);
    }

    #[test]
    fn an_invalid_track_is_not_counted() {
        let p = path();
        let stats = PlayStats::open(p.clone()).unwrap();
        assert!(stats.record(track("bad")).is_err());
        assert!(stats.view().unwrap().tracks.is_empty());
        let _ = fs::remove_file(p);
    }
}
