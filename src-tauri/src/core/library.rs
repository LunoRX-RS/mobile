//! Device-local likes and provider-scoped artist profiles. No provider library is modified.
use super::{
    domain::ProviderId,
    playback::{extract, track_info::artwork_url},
    search::hit::ArtistRef,
    yt::Yt,
};
use innerlight::music::AlbumKind;
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::PathBuf, sync::Mutex};
#[derive(Clone, Copy, Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LibraryError {
    Validation,
    Storage,
    InUse,
    Full,
    Unavailable,
    UnsupportedProvider,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedTrack {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<super::search::hit::SearchSource>,
    pub provider: ProviderId,
    pub provider_track_id: String,
    pub canonical_url: String,
    pub title: String,
    pub artists: Vec<String>,
    #[serde(default)]
    pub artist_refs: Vec<ArtistRef>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<u64>,
}
impl SavedTrack {
    pub(crate) fn validate(&mut self) -> Result<(), LibraryError> {
        let id = &self.provider_track_id;
        let canonical = match self.provider {
            ProviderId::YouTube if extract::valid_id(id) => {
                format!("https://www.youtube.com/watch?v={id}")
            }
            ProviderId::Spotify
                if id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()) =>
            {
                format!("https://open.spotify.com/track/{id}")
            }
            _ => return Err(LibraryError::Validation),
        };
        if self.canonical_url != canonical
            || self.title.trim().is_empty()
            || self.title.len() > 2048
            || self.artists.len() > 30
            || self.artists.iter().any(|a| a.len() > 512)
            || self.artist_refs.len() > 30
            || self.artist_refs.iter().any(|a| {
                a.provider != self.provider
                    || a.name.trim().is_empty()
                    || a.name.len() > 512
                    || a.id.as_ref().is_some_and(|id| !artist_id(&a.provider, id))
            })
        {
            return Err(LibraryError::Validation);
        }
        self.artwork_url = self.artwork_url.as_deref().and_then(artwork_url);
        // Spotify metadata is not retained by this app's local library contract.
        if self.provider == ProviderId::Spotify {
            self.source = None;
            self.title = id.clone();
            self.artists.clear();
            self.artist_refs.clear();
            self.artwork_url = None;
            self.duration_ms = None;
        }
        Ok(())
    }
}
pub fn artist_id(provider: &ProviderId, id: &str) -> bool {
    match provider {
        ProviderId::YouTube => {
            id.starts_with("UC")
                && id.len() == 24
                && id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        }
        ProviderId::Spotify => id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric()),
        // A show is not an artist page.
        ProviderId::Podcast => false,
    }
}
pub struct SavedLibrary {
    path: PathBuf,
    _lock: fs::File,
    tracks: Mutex<Vec<SavedTrack>>,
}
impl SavedLibrary {
    pub fn open(path: PathBuf) -> Result<Self, LibraryError> {
        let parent = path.parent().ok_or(LibraryError::Storage)?;
        fs::create_dir_all(parent).map_err(|_| LibraryError::Storage)?;
        let lock = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path.with_extension("lock"))
            .map_err(|_| LibraryError::Storage)?;
        lock.try_lock().map_err(|error| match error {
            std::fs::TryLockError::WouldBlock => LibraryError::InUse,
            std::fs::TryLockError::Error(_) => LibraryError::Storage,
        })?;
        let mut tracks: Vec<SavedTrack> = if path.exists() {
            let meta = fs::symlink_metadata(&path).map_err(|_| LibraryError::Storage)?;
            if !meta.file_type().is_file() || meta.len() > 8 * 1024 * 1024 {
                return Err(LibraryError::Storage);
            }
            serde_json::from_slice(&fs::read(&path).map_err(|_| LibraryError::Storage)?)
                .map_err(|_| LibraryError::Storage)?
        } else {
            Vec::new()
        };
        if tracks.len() > 5000 {
            return Err(LibraryError::Storage);
        }
        let mut ids = std::collections::HashSet::new();
        for track in &mut tracks {
            track.validate().map_err(|_| LibraryError::Storage)?;
            if !ids.insert((track.provider.clone(), track.provider_track_id.clone())) {
                return Err(LibraryError::Storage);
            }
        }
        Ok(Self {
            path,
            _lock: lock,
            tracks: Mutex::new(tracks),
        })
    }
    pub fn list(&self) -> Result<Vec<SavedTrack>, LibraryError> {
        Ok(self
            .tracks
            .lock()
            .map_err(|_| LibraryError::Storage)?
            .clone())
    }
    pub fn set_like(
        &self,
        mut track: SavedTrack,
        liked: bool,
    ) -> Result<Vec<SavedTrack>, LibraryError> {
        track.validate()?;
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = tracks.clone();
        next.retain(|t| {
            t.provider != track.provider || t.provider_track_id != track.provider_track_id
        });
        if liked {
            next.insert(0, track)
        }
        self.commit(&mut tracks, next)
    }
    /// Replaces the whole list with `incoming`, keeping valid tracks only and dropping duplicates. The caller owns the
    /// order (the server sends newest first). The cap and size limit are enforced by `commit`.
    pub fn replace(&self, incoming: Vec<SavedTrack>) -> Result<Vec<SavedTrack>, LibraryError> {
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = Vec::new();
        let mut keys = std::collections::HashSet::new();
        for mut track in incoming {
            if next.len() >= 5000 {
                break;
            }
            if track.validate().is_err()
                || !keys.insert((track.provider.clone(), track.provider_track_id.clone()))
            {
                continue;
            }
            next.push(track);
        }
        self.commit(&mut tracks, next)
    }
    pub fn remove_like(
        &self,
        provider: &ProviderId,
        provider_track_id: &str,
    ) -> Result<Vec<SavedTrack>, LibraryError> {
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = tracks.clone();
        next.retain(|t| t.provider != *provider || t.provider_track_id != provider_track_id);
        if next.len() == tracks.len() {
            return Ok(tracks.clone());
        }
        self.commit(&mut tracks, next)
    }
    pub fn get(
        &self,
        provider: &ProviderId,
        provider_track_id: &str,
    ) -> Result<Option<SavedTrack>, LibraryError> {
        Ok(self
            .tracks
            .lock()
            .map_err(|_| LibraryError::Storage)?
            .iter()
            .find(|t| t.provider == *provider && t.provider_track_id == provider_track_id)
            .cloned())
    }
    /// Adds tracks that are not in the list yet, after the ones already there, up to `cap` entries in total.
    /// Invalid tracks are skipped. Returns how many were added.
    pub fn merge_append(
        &self,
        incoming: Vec<SavedTrack>,
        cap: usize,
    ) -> Result<usize, LibraryError> {
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = tracks.clone();
        let mut added = 0;
        for mut track in incoming {
            if next.len() >= cap.min(5000) {
                break;
            }
            if track.validate().is_err()
                || next.iter().any(|t| {
                    t.provider == track.provider && t.provider_track_id == track.provider_track_id
                })
            {
                continue;
            }
            next.push(track);
            added += 1;
        }
        if added > 0 {
            self.commit(&mut tracks, next)?;
        }
        Ok(added)
    }
    /// Removes the YouTube tracks with these ids. Returns how many were removed.
    pub fn remove_youtube_ids(
        &self,
        ids: &std::collections::HashSet<String>,
    ) -> Result<usize, LibraryError> {
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        let mut next = tracks.clone();
        next.retain(|t| t.provider != ProviderId::YouTube || !ids.contains(&t.provider_track_id));
        let removed = tracks.len() - next.len();
        if removed > 0 {
            self.commit(&mut tracks, next)?;
        }
        Ok(removed)
    }
    pub fn record_play(&self, mut track: SavedTrack) -> Result<Vec<SavedTrack>, LibraryError> {
        track.validate()?;
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        if tracks.first().is_some_and(|t| {
            t.provider == track.provider && t.provider_track_id == track.provider_track_id
        }) {
            return Ok(tracks.clone());
        }
        let mut next = tracks.clone();
        next.retain(|t| {
            t.provider != track.provider || t.provider_track_id != track.provider_track_id
        });
        next.insert(0, track);
        next.truncate(100);
        self.commit(&mut tracks, next)
    }
    pub fn clear(&self) -> Result<Vec<SavedTrack>, LibraryError> {
        let mut tracks = self.tracks.lock().map_err(|_| LibraryError::Storage)?;
        self.commit(&mut tracks, vec![])
    }
    fn commit(
        &self,
        tracks: &mut Vec<SavedTrack>,
        next: Vec<SavedTrack>,
    ) -> Result<Vec<SavedTrack>, LibraryError> {
        if next.len() > 5000 {
            return Err(LibraryError::Full);
        }
        let bytes = serde_json::to_vec(&next).map_err(|_| LibraryError::Storage)?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(LibraryError::Full);
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
        *tracks = next;
        Ok(tracks.clone())
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistProfile {
    pub provider: ProviderId,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub artwork_url: Option<String>,
    pub subscribers: Option<u64>,
    pub tracks: Vec<SavedTrack>,
    pub albums: Vec<ArtistAlbum>,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistAlbum {
    pub id: String,
    pub name: String,
    pub year: Option<u16>,
    pub artwork_url: Option<String>,
}
/// A YouTube Music track item as a saved-track record. `fallback_cover` fills in when the item has no cover (album pages).
pub(crate) fn saved_track(
    t: innerlight::music::Track,
    fallback_cover: Option<&str>,
) -> Option<SavedTrack> {
    if !extract::valid_id(&t.id) {
        return None;
    }
    Some(SavedTrack {
        source: Some(super::search::hit::SearchSource::YouTubeMusic),
        provider: ProviderId::YouTube,
        provider_track_id: t.id.clone(),
        canonical_url: format!("https://www.youtube.com/watch?v={}", t.id),
        title: t.title,
        artists: t.artists.iter().map(|a| a.name.clone()).collect(),
        artist_refs: t
            .artists
            .into_iter()
            .map(|a| ArtistRef {
                provider: ProviderId::YouTube,
                id: a.id,
                name: a.name,
            })
            .collect(),
        artwork_url: t
            .thumbnails
            .into_iter()
            .max_by_key(|c| c.width)
            .and_then(|c| artwork_url(&c.url))
            .or_else(|| fallback_cover.and_then(artwork_url)),
        duration_ms: t.duration_ms,
    })
}
/// YouTube Music album ids look like `MPREb_` plus url-safe characters.
pub fn album_id(provider: &ProviderId, id: &str) -> bool {
    *provider == ProviderId::YouTube
        && id.starts_with("MPREb_")
        && id.len() <= 40
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlbumProfile {
    pub provider: ProviderId,
    pub id: String,
    pub name: String,
    pub kind: String,
    pub year: Option<u16>,
    pub artists: Vec<ArtistRef>,
    pub artwork_url: Option<String>,
    pub tracks: Vec<SavedTrack>,
}
pub async fn album(yt: &Yt, provider: ProviderId, id: &str) -> Result<AlbumProfile, LibraryError> {
    if provider != ProviderId::YouTube {
        return Err(LibraryError::UnsupportedProvider);
    }
    if !album_id(&provider, id) {
        return Err(LibraryError::Validation);
    }
    let a = tokio::time::timeout(std::time::Duration::from_secs(20), yt.music().album(id))
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)?;
    let cover = a
        .thumbnails
        .into_iter()
        .max_by_key(|c| c.width)
        .and_then(|c| artwork_url(&c.url));
    let tracks = a
        .tracks
        .into_iter()
        .take(200)
        .filter_map(|t| saved_track(t, cover.as_deref()))
        .collect();
    Ok(AlbumProfile {
        provider,
        id: a.id,
        name: a.title,
        kind: match a.kind {
            AlbumKind::Album => "Album",
            AlbumKind::Single => "Single",
            AlbumKind::Ep => "Ep",
            AlbumKind::Unknown => "",
        }
        .into(),
        year: a.year,
        artists: a
            .artists
            .into_iter()
            .map(|x| ArtistRef {
                provider: ProviderId::YouTube,
                id: x.id,
                name: x.name,
            })
            .collect(),
        artwork_url: cover,
        tracks,
    })
}
/// "1.2M subscribers", "34K", "1,234" as a number. English abbreviations only; anything else is `None`.
fn subscriber_count(text: &str) -> Option<u64> {
    let text = text.trim();
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.' || c == ','))
        .unwrap_or(text.len());
    let number: f64 = text[..end].replace(',', "").parse().ok()?;
    let scale = match text[end..].trim_start().chars().next() {
        Some('K' | 'k') => 1e3,
        Some('M' | 'm') => 1e6,
        Some('B' | 'b') => 1e9,
        _ => 1.0,
    };
    let value = (number * scale).round();
    (value.is_finite() && (0.0..9e15).contains(&value)).then_some(value as u64)
}
pub async fn profile(
    yt: &Yt,
    provider: ProviderId,
    id: &str,
) -> Result<ArtistProfile, LibraryError> {
    if !artist_id(&provider, id) {
        return Err(LibraryError::Validation);
    }
    if provider != ProviderId::YouTube {
        return Err(LibraryError::UnsupportedProvider);
    }
    let a = tokio::time::timeout(std::time::Duration::from_secs(20), yt.music().artist(id))
        .await
        .map_err(|_| LibraryError::Unavailable)?
        .map_err(|_| LibraryError::Unavailable)?;
    let tracks = a
        .top_songs
        .into_iter()
        .take(30)
        .filter_map(|t| saved_track(t, None))
        .collect();
    Ok(ArtistProfile {
        provider,
        id: a.id,
        name: a.name,
        description: a.description,
        artwork_url: a
            .thumbnails
            .into_iter()
            .max_by_key(|c| c.width)
            .and_then(|c| artwork_url(&c.url)),
        subscribers: a.subscribers.as_deref().and_then(subscriber_count),
        tracks,
        albums: a
            .albums
            .into_iter()
            .chain(a.singles)
            .take(30)
            .map(|a| ArtistAlbum {
                id: a.id,
                name: a.title,
                year: a.year,
                artwork_url: a
                    .thumbnails
                    .into_iter()
                    .max_by_key(|c| c.width)
                    .and_then(|c| artwork_url(&c.url)),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subscriber_text_becomes_a_count() {
        assert_eq!(subscriber_count("1.2M subscribers"), Some(1_200_000));
        assert_eq!(subscriber_count("34K"), Some(34_000));
        assert_eq!(subscriber_count("1,234 subscribers"), Some(1234));
        assert_eq!(subscriber_count("2.5B"), Some(2_500_000_000));
        assert_eq!(subscriber_count("No subscribers"), None);
        assert_eq!(subscriber_count(""), None);
        assert_eq!(subscriber_count("....M"), None);
    }
    #[test]
    fn a_track_row_becomes_a_saved_track_with_the_widest_cover() {
        use innerlight::music::{ArtistRef as MusicArtist, Thumbnail, Track, TrackKind};
        let thumb = |url: &str, width| Thumbnail {
            url: url.into(),
            width,
            height: width,
        };
        let row = |id: &str| Track {
            id: id.into(),
            title: "Song".into(),
            artists: vec![MusicArtist {
                id: Some("UCuAXFkgsw1L7xaCfnd5JJOw".into()),
                name: "Artist".into(),
            }],
            album: None,
            duration_ms: Some(166_000),
            thumbnails: vec![
                thumb("https://i.ytimg.com/vi/a/small.jpg", 60),
                thumb("https://i.ytimg.com/vi/a/large.jpg", 544),
                thumb("https://evil.test/huge.jpg", 9999),
            ],
            is_explicit: false,
            kind: TrackKind::Audio,
        };
        // The widest cover is on a host the app does not accept, so the fallback cover is used.
        let saved = saved_track(
            row("r-G2pKr2_n8"),
            Some("https://i.ytimg.com/vi/b/cover.jpg"),
        )
        .unwrap();
        assert_eq!(saved.title, "Song");
        assert_eq!(saved.duration_ms, Some(166_000));
        assert_eq!(
            saved.artist_refs[0].id.as_deref(),
            Some("UCuAXFkgsw1L7xaCfnd5JJOw")
        );
        assert_eq!(
            saved.artwork_url.as_deref(),
            Some("https://i.ytimg.com/vi/b/cover.jpg")
        );
        assert!(saved_track(row("bad id"), None).is_none());
    }
    fn track() -> SavedTrack {
        SavedTrack {
            source: None,
            provider: ProviderId::YouTube,
            provider_track_id: "r-G2pKr2_n8".into(),
            canonical_url: "https://www.youtube.com/watch?v=r-G2pKr2_n8".into(),
            title: "BU".into(),
            artists: vec!["Artist".into()],
            artist_refs: vec![],
            artwork_url: None,
            duration_ms: Some(166000),
        }
    }
    #[test]
    fn album_ids_are_youtube_music_browse_ids_only() {
        let yt = ProviderId::YouTube;
        assert!(album_id(&yt, "MPREb_O2gXCdCVGsZ"));
        assert!(!album_id(&yt, "UCRD-INDaHvHlO8K_33uKet"));
        assert!(!album_id(&yt, "MPREb_bad id"));
        assert!(!album_id(&yt, &format!("MPREb_{}", "a".repeat(40))));
        assert!(!album_id(&ProviderId::Spotify, "MPREb_O2gXCdCVGsZ"));
    }
    #[test]
    fn merge_append_adds_new_tracks_after_existing_and_skips_duplicates_and_invalid() {
        let lib = SavedLibrary::open(path()).unwrap();
        lib.set_like(track(), true).unwrap();
        let mut second = track();
        second.provider_track_id = "aqz-KE-bpKQ".into();
        second.canonical_url = "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into();
        let mut invalid = track();
        invalid.provider_track_id = "bad id".into();
        let added = lib
            .merge_append(vec![track(), second, invalid], 5000)
            .unwrap();
        assert_eq!(added, 1);
        let list = lib.list().unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].provider_track_id, "r-G2pKr2_n8");
        assert_eq!(list[1].provider_track_id, "aqz-KE-bpKQ");
        assert_eq!(lib.merge_append(list.clone(), 5000).unwrap(), 0);
        assert_eq!(lib.merge_append(vec![track()], 1).unwrap(), 0);
    }
    fn path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("lunor-likes-{:032x}", rand::random::<u128>()))
            .join("likes.json")
    }
    #[test]
    fn likes_persist_and_explicit_updates_are_idempotent() {
        let path = path();
        {
            let lib = SavedLibrary::open(path.clone()).unwrap();
            assert_eq!(lib.set_like(track(), true).unwrap().len(), 1);
            assert_eq!(lib.set_like(track(), true).unwrap().len(), 1);
        }
        {
            let lib = SavedLibrary::open(path.clone()).unwrap();
            assert_eq!(lib.list().unwrap()[0].title, "BU");
            assert!(lib.set_like(track(), false).unwrap().is_empty());
            assert!(lib.set_like(track(), false).unwrap().is_empty());
        }
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn replace_drops_invalid_and_duplicate_rows_and_removal_is_keyed() {
        let path = path();
        let lib = SavedLibrary::open(path.clone()).unwrap();
        let mut second = track();
        second.provider_track_id = "aqz-KE-bpKQ".into();
        second.canonical_url = "https://www.youtube.com/watch?v=aqz-KE-bpKQ".into();
        let mut invalid = track();
        invalid.provider_track_id = "bad id".into();
        let list = lib
            .replace(vec![track(), invalid, second.clone(), track()])
            .unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].provider_track_id, "r-G2pKr2_n8");
        assert_eq!(list[1].provider_track_id, "aqz-KE-bpKQ");
        assert!(
            lib.get(&ProviderId::YouTube, "aqz-KE-bpKQ")
                .unwrap()
                .is_some()
        );
        let left = lib
            .remove_like(&ProviderId::YouTube, "r-G2pKr2_n8")
            .unwrap();
        assert_eq!(left.len(), 1);
        assert!(
            lib.get(&ProviderId::YouTube, "r-G2pKr2_n8")
                .unwrap()
                .is_none()
        );
        assert!(lib.replace(vec![]).unwrap().is_empty());
        drop(lib);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn second_library_open_is_in_use_and_recovers_after_release() {
        let path = path();
        let first = SavedLibrary::open(path.clone()).unwrap();
        assert!(matches!(
            SavedLibrary::open(path.clone()),
            Err(LibraryError::InUse)
        ));
        drop(first);
        drop(SavedLibrary::open(path.clone()).unwrap());
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn listening_history_is_recent_unique_bounded_and_persistent() {
        let path = path();
        let history = SavedLibrary::open(path.clone()).unwrap();
        for i in 0..105 {
            let mut t = track();
            t.provider_track_id = format!("{i:011}");
            t.canonical_url = format!("https://www.youtube.com/watch?v={}", t.provider_track_id);
            history.record_play(t).unwrap();
        }
        assert_eq!(history.list().unwrap().len(), 100);
        let mut old = track();
        old.provider_track_id = "00000000020".into();
        old.canonical_url = "https://www.youtube.com/watch?v=00000000020".into();
        history.record_play(old.clone()).unwrap();
        history.record_play(old).unwrap();
        let saved = history.list().unwrap();
        assert_eq!(saved[0].provider_track_id, "00000000020");
        assert_eq!(saved.len(), 100);
        drop(history);
        let history = SavedLibrary::open(path.clone()).unwrap();
        assert_eq!(history.list().unwrap()[0].provider_track_id, "00000000020");
        history.clear().unwrap();
        drop(history);
        assert!(
            SavedLibrary::open(path.clone())
                .unwrap()
                .list()
                .unwrap()
                .is_empty()
        );
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    #[test]
    fn invalid_identity_and_foreign_artist_are_rejected() {
        let mut t = track();
        t.canonical_url = "https://evil.test".into();
        assert!(matches!(t.validate(), Err(LibraryError::Validation)));
        let mut t = track();
        t.artist_refs.push(ArtistRef {
            provider: ProviderId::Spotify,
            id: None,
            name: "Artist".into(),
        });
        assert!(matches!(t.validate(), Err(LibraryError::Validation)));
        assert!(!artist_id(&ProviderId::YouTube, "../../private"));
        assert!(artist_id(&ProviderId::YouTube, "UCabcdefghijklmnopqrstuv"));
    }
    #[test]
    fn failed_write_preserves_memory() {
        let path = path();
        let lib = SavedLibrary::open(path.clone()).unwrap();
        lib.set_like(track(), true).unwrap();
        fs::remove_file(&path).unwrap();
        fs::create_dir(&path).unwrap();
        assert!(matches!(
            lib.set_like(track(), false),
            Err(LibraryError::Storage)
        ));
        assert_eq!(lib.list().unwrap().len(), 1);
        drop(lib);
        fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
