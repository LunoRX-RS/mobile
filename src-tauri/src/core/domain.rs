use std::str::FromStr;

use serde::{Deserialize, Serialize};

use crate::core::errors::CoreError;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String")] // deserialize via FromStr so unknown providers are CoreError::Validation
pub enum ProviderId {
    Spotify,
    YouTube,
    /// Podcast episodes read straight from a show's RSS feed (see `core::rss_podcast`).
    Podcast,
}

impl ProviderId {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Spotify => "Spotify",
            Self::YouTube => "YouTube",
            Self::Podcast => "Podcast",
        }
    }
}

impl FromStr for ProviderId {
    type Err = CoreError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Spotify" => Ok(Self::Spotify),
            "YouTube" => Ok(Self::YouTube),
            "Podcast" => Ok(Self::Podcast),
            _ => Err(CoreError::Validation("unknown provider".into())),
        }
    }
}

impl TryFrom<String> for ProviderId {
    type Error = CoreError;

    fn try_from(s: String) -> Result<Self, Self::Error> {
        s.parse()
    }
}

// Fields are pub(crate): TrackReference and UnifiedPlaylist validate in new()/TryFrom; playlist.rs mutates UnifiedPlaylist entries under its own invariants. Read accessors arrive with the Tauri IPC plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "TrackReferenceWire")]
pub struct TrackReference {
    pub(crate) provider: ProviderId,
    pub(crate) provider_track_id: String,
    pub(crate) canonical_url: String,
    pub(crate) title: String,
    pub(crate) artists: Vec<String>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) duration_ms: Option<u64>,
}

impl TrackReference {
    pub fn new(
        provider: ProviderId,
        provider_track_id: String,
        canonical_url: String,
        title: String,
    ) -> Result<Self, CoreError> {
        if provider_track_id.trim().is_empty() {
            return Err(CoreError::Validation("empty provider track id".into()));
        }
        if canonical_url.trim().is_empty() {
            return Err(CoreError::Validation("empty canonical url".into()));
        }
        Ok(Self {
            provider,
            provider_track_id,
            canonical_url,
            title,
            artists: Vec::new(),
            artwork_url: None,
            duration_ms: None,
        })
    }
}

// Deserialize target: TryFrom runs new() so JSON cannot bypass validation.
#[derive(Deserialize)]
struct TrackReferenceWire {
    provider: ProviderId,
    provider_track_id: String,
    canonical_url: String,
    title: String,
    artists: Vec<String>,
    artwork_url: Option<String>,
    duration_ms: Option<u64>,
}

impl TryFrom<TrackReferenceWire> for TrackReference {
    type Error = CoreError;

    fn try_from(w: TrackReferenceWire) -> Result<Self, Self::Error> {
        let mut t = Self::new(w.provider, w.provider_track_id, w.canonical_url, w.title)?;
        t.artists = w.artists;
        t.artwork_url = w.artwork_url;
        t.duration_ms = w.duration_ms;
        Ok(t)
    }
}

// Fields are pub(crate); read accessors arrive with the Tauri IPC plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "PlaylistEntryWire")]
pub struct PlaylistEntry {
    pub(crate) id: String, // entry identity, separate from track identity
    pub(crate) track: TrackReference,
    pub(crate) position: u32,
}

// Deserialize target: TryFrom rejects an empty entry id so JSON cannot bypass it.
#[derive(Deserialize)]
struct PlaylistEntryWire {
    id: String,
    track: TrackReference,
    position: u32,
}

impl TryFrom<PlaylistEntryWire> for PlaylistEntry {
    type Error = CoreError;

    fn try_from(w: PlaylistEntryWire) -> Result<Self, Self::Error> {
        if w.id.trim().is_empty() {
            return Err(CoreError::Validation("empty entry id".into()));
        }
        Ok(Self {
            id: w.id,
            track: w.track,
            position: w.position,
        })
    }
}

// Fields are pub(crate); read accessors arrive with the Tauri IPC plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "UnifiedPlaylistWire")]
pub struct UnifiedPlaylist {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) entries: Vec<PlaylistEntry>,
    pub(crate) version: u64,
}

impl UnifiedPlaylist {
    pub fn new(id: String, name: String) -> Result<Self, CoreError> {
        // Checked here, not only in TryFrom, so a constructed playlist always deserializes.
        if id.trim().is_empty() {
            return Err(CoreError::Validation("empty playlist id".into()));
        }
        if name.trim().is_empty() {
            return Err(CoreError::Validation("empty playlist name".into()));
        }
        Ok(Self {
            id,
            name,
            description: None,
            entries: Vec::new(),
            version: 0,
        })
    }
}

// Deserialize target: TryFrom runs new() so an empty name cannot arrive via JSON.
#[derive(Deserialize)]
struct UnifiedPlaylistWire {
    id: String,
    name: String,
    description: Option<String>,
    entries: Vec<PlaylistEntry>,
    version: u64,
}

impl TryFrom<UnifiedPlaylistWire> for UnifiedPlaylist {
    type Error = CoreError;

    fn try_from(w: UnifiedPlaylistWire) -> Result<Self, Self::Error> {
        let mut p = Self::new(w.id, w.name)?;
        let mut ids = std::collections::HashSet::new();
        for (i, e) in w.entries.iter().enumerate() {
            if !ids.insert(e.id.as_str()) {
                return Err(CoreError::Validation("duplicate entry id".into()));
            }
            let index =
                u32::try_from(i).map_err(|_| CoreError::Validation("too many entries".into()))?;
            if e.position != index {
                return Err(CoreError::Validation("position mismatch".into()));
            }
        }
        p.description = w.description;
        p.entries = w.entries;
        p.version = w.version;
        Ok(p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spotify_parses_as_provider_id() {
        assert_eq!("Spotify".parse::<ProviderId>(), Ok(ProviderId::Spotify));
    }

    #[test]
    fn unknown_provider_is_validation_error() {
        let err = "Deezer".parse::<ProviderId>().unwrap_err();
        assert!(matches!(err, CoreError::Validation(_)));
    }

    #[test]
    fn provider_id_serializes_as_variant_name() {
        assert_eq!(
            serde_json::to_string(&ProviderId::YouTube).unwrap(),
            "\"YouTube\""
        );
    }

    #[test]
    fn track_reference_rejects_empty_provider_track_id() {
        let r = TrackReference::new(
            ProviderId::Spotify,
            " ".into(),
            "https://x".into(),
            "t".into(),
        );
        assert!(matches!(r, Err(CoreError::Validation(_))));
    }

    #[test]
    fn track_reference_rejects_empty_canonical_url() {
        let r = TrackReference::new(ProviderId::Spotify, "id".into(), "".into(), "t".into());
        assert!(matches!(r, Err(CoreError::Validation(_))));
    }

    #[test]
    fn unified_playlist_rejects_empty_name() {
        let r = UnifiedPlaylist::new("p1".into(), " ".into());
        assert!(matches!(r, Err(CoreError::Validation(_))));
    }

    fn sample_track() -> TrackReference {
        let mut t = TrackReference::new(
            ProviderId::YouTube,
            "vid1".into(),
            "https://youtu.be/vid1".into(),
            "Title".into(),
        )
        .unwrap();
        t.artists = vec!["Artist".into()];
        t.artwork_url = Some("https://img/x.jpg".into());
        t.duration_ms = Some(1234);
        t
    }

    #[test]
    fn track_reference_json_round_trips() {
        let t = sample_track();
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<TrackReference>(&json).unwrap(), t);
    }

    #[test]
    fn track_reference_json_rejects_empty_provider_track_id() {
        let json = r#"{"provider":"Spotify","provider_track_id":"","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}"#;
        assert!(serde_json::from_str::<TrackReference>(json).is_err());
    }

    #[test]
    fn unified_playlist_json_round_trips() {
        let mut p = UnifiedPlaylist::new("p1".into(), "Mix".into()).unwrap();
        p.description = Some("d".into());
        p.entries = vec![PlaylistEntry {
            id: "e1".into(),
            track: sample_track(),
            position: 0,
        }];
        p.version = 3;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<UnifiedPlaylist>(&json).unwrap(), p);
    }

    #[test]
    fn unified_playlist_json_rejects_empty_name() {
        let json = r#"{"id":"p1","name":"  ","description":null,"entries":[],"version":0}"#;
        assert!(serde_json::from_str::<UnifiedPlaylist>(json).is_err());
    }

    #[test]
    fn unified_playlist_json_rejects_duplicate_entry_ids() {
        let track = r#"{"provider":"Spotify","provider_track_id":"a","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}"#;
        let json = format!(
            r#"{{"id":"p1","name":"Mix","description":null,"entries":[{{"id":"e1","track":{track},"position":0}},{{"id":"e1","track":{track},"position":1}}],"version":2}}"#
        );
        assert!(serde_json::from_str::<UnifiedPlaylist>(&json).is_err());
    }

    #[test]
    fn provider_id_json_rejects_unknown_provider_as_validation() {
        let err = serde_json::from_str::<ProviderId>("\"Deezer\"").unwrap_err();
        assert!(err.to_string().contains("unknown provider"), "got: {err}");
        assert!(matches!(
            ProviderId::try_from("Deezer".to_string()),
            Err(CoreError::Validation(_))
        ));
    }

    #[test]
    fn unified_playlist_json_rejects_position_mismatch() {
        let track = r#"{"provider":"Spotify","provider_track_id":"a","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}"#;
        let json = format!(
            r#"{{"id":"p1","name":"Mix","description":null,"entries":[{{"id":"e1","track":{track},"position":0}},{{"id":"e2","track":{track},"position":5}}],"version":2}}"#
        );
        let err = serde_json::from_str::<UnifiedPlaylist>(&json).unwrap_err();
        assert!(err.to_string().contains("position mismatch"), "got: {err}");
    }

    #[test]
    fn unified_playlist_json_rejects_empty_playlist_id() {
        let json = r#"{"id":"  ","name":"Mix","description":null,"entries":[],"version":0}"#;
        let err = serde_json::from_str::<UnifiedPlaylist>(json).unwrap_err();
        assert!(err.to_string().contains("empty playlist id"), "got: {err}");
    }

    #[test]
    fn unified_playlist_json_rejects_empty_entry_id() {
        let track = r#"{"provider":"Spotify","provider_track_id":"a","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}"#;
        let json = format!(
            r#"{{"id":"p1","name":"Mix","description":null,"entries":[{{"id":"","track":{track},"position":0}}],"version":1}}"#
        );
        let err = serde_json::from_str::<UnifiedPlaylist>(&json).unwrap_err();
        assert!(err.to_string().contains("empty entry id"), "got: {err}");
    }

    #[test]
    fn constructor_valid_playlist_with_entries_round_trips() {
        let mut p = UnifiedPlaylist::new("p1".into(), "Mix".into()).unwrap();
        for (i, id) in ["e1", "e2"].iter().enumerate() {
            p.entries.push(PlaylistEntry {
                id: (*id).into(),
                track: sample_track(),
                position: i as u32,
            });
        }
        p.version = 2;
        let json = serde_json::to_string(&p).unwrap();
        assert_eq!(serde_json::from_str::<UnifiedPlaylist>(&json).unwrap(), p);
    }

    #[test]
    fn playlist_entry_json_rejects_empty_id() {
        let track = r#"{"provider":"Spotify","provider_track_id":"a","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}"#;
        let json = format!(r#"{{"id":"  ","track":{track},"position":0}}"#);
        let err = serde_json::from_str::<PlaylistEntry>(&json).unwrap_err();
        assert!(err.to_string().contains("empty entry id"), "got: {err}");
    }

    #[test]
    fn playlist_entry_json_round_trips() {
        let e = PlaylistEntry {
            id: "e1".into(),
            track: sample_track(),
            position: 3,
        };
        let json = serde_json::to_string(&e).unwrap();
        assert_eq!(serde_json::from_str::<PlaylistEntry>(&json).unwrap(), e);
    }

    #[test]
    fn provider_id_json_still_uses_variant_names() {
        assert_eq!(
            serde_json::from_str::<ProviderId>("\"Spotify\"").unwrap(),
            ProviderId::Spotify
        );
    }

    #[test]
    fn provider_id_as_str_returns_display_names() {
        assert_eq!(ProviderId::Spotify.as_str(), "Spotify");
        assert_eq!(ProviderId::YouTube.as_str(), "YouTube");
    }
}
