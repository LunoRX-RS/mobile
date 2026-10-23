use serde::{Deserialize, Serialize};

use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SearchSource {
    #[serde(rename = "YouTube Music")]
    YouTubeMusic,
    #[serde(rename = "YouTube")]
    YouTube,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ArtistRef {
    pub provider: ProviderId,
    pub id: Option<String>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "SearchHitWire")]
pub struct SearchHit {
    pub(crate) provider: ProviderId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<SearchSource>,
    pub(crate) provider_track_id: String,
    pub(crate) canonical_url: String,
    pub(crate) title: String,
    pub(crate) artists: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) artist_refs: Vec<ArtistRef>,
    pub(crate) artwork_url: Option<String>,
    pub(crate) duration_ms: Option<u64>,
}

impl SearchHit {
    pub fn new(
        provider: ProviderId,
        provider_track_id: String,
        canonical_url: String,
        title: String,
        artists: Vec<String>,
    ) -> Result<Self, CoreError> {
        if provider_track_id.trim().is_empty() {
            return Err(CoreError::Validation("empty provider track id".into()));
        }
        if canonical_url.trim().is_empty() {
            return Err(CoreError::Validation("empty canonical url".into()));
        }
        Ok(Self {
            provider,
            source: None,
            provider_track_id,
            canonical_url,
            title,
            artists,
            artist_refs: Vec::new(),
            artwork_url: None,
            duration_ms: None,
        })
    }

    pub fn source(&self) -> Option<SearchSource> {
        self.source
    }

    pub fn provider(&self) -> ProviderId {
        self.provider.clone()
    }

    pub fn provider_track_id(&self) -> &str {
        &self.provider_track_id
    }

    pub fn canonical_url(&self) -> &str {
        &self.canonical_url
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn artists(&self) -> &[String] {
        &self.artists
    }

    pub fn artist_refs(&self) -> &[ArtistRef] {
        &self.artist_refs
    }

    pub fn artwork_url(&self) -> Option<&str> {
        self.artwork_url.as_deref()
    }

    pub fn duration_ms(&self) -> Option<u64> {
        self.duration_ms
    }
}

// Deserialize target: TryFrom runs new() so JSON cannot bypass validation.
#[derive(Deserialize)]
struct SearchHitWire {
    #[serde(default)]
    source: Option<SearchSource>,
    provider: ProviderId,
    provider_track_id: String,
    canonical_url: String,
    title: String,
    artists: Vec<String>,
    #[serde(default)]
    artist_refs: Vec<ArtistRef>,
    artwork_url: Option<String>,
    duration_ms: Option<u64>,
}

impl TryFrom<SearchHitWire> for SearchHit {
    type Error = CoreError;

    fn try_from(w: SearchHitWire) -> Result<Self, Self::Error> {
        let mut h = Self::new(
            w.provider,
            w.provider_track_id,
            w.canonical_url,
            w.title,
            w.artists,
        )?;
        h.source = w.source;
        h.artist_refs = w.artist_refs;
        h.artwork_url = w.artwork_url;
        h.duration_ms = w.duration_ms;
        Ok(h)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_validation<T>(r: &Result<T, CoreError>) -> bool {
        matches!(r, Err(CoreError::Validation(_)))
    }

    #[test]
    fn empty_provider_track_id_is_rejected() {
        let r = SearchHit::new(
            ProviderId::Spotify,
            String::new(),
            "https://x".into(),
            "t".into(),
            vec![],
        );
        assert!(is_validation(&r));
    }

    #[test]
    fn whitespace_provider_track_id_is_rejected() {
        let r = SearchHit::new(
            ProviderId::Spotify,
            " ".into(),
            "https://x".into(),
            "t".into(),
            vec![],
        );
        assert!(is_validation(&r));
    }

    #[test]
    fn empty_canonical_url_is_rejected() {
        let r = SearchHit::new(
            ProviderId::Spotify,
            "id".into(),
            String::new(),
            "t".into(),
            vec![],
        );
        assert!(is_validation(&r));
    }

    #[test]
    fn valid_hit_keeps_provider_and_title() {
        let h = SearchHit::new(
            ProviderId::YouTube,
            "vid1".into(),
            "https://youtu.be/vid1".into(),
            "Title".into(),
            vec![],
        )
        .unwrap();
        assert_eq!(h.provider, ProviderId::YouTube);
        assert_eq!(h.title, "Title");
        assert!(h.artists.is_empty());
        assert_eq!(h.artwork_url, None);
        assert_eq!(h.duration_ms, None);
    }

    #[test]
    fn empty_title_is_accepted() {
        // Documented gap from the Phase 2 review, same as TrackReference.
        let r = SearchHit::new(
            ProviderId::Spotify,
            "id".into(),
            "https://x".into(),
            String::new(),
            vec![],
        );
        assert!(r.is_ok());
    }

    #[test]
    fn deserialize_rejects_empty_provider_track_id() {
        let json = |id: &str| {
            format!(
                r#"{{"provider":"Spotify","provider_track_id":"{id}","canonical_url":"https://x","title":"t","artists":[],"artwork_url":null,"duration_ms":null}}"#
            )
        };
        // Control: the same JSON with a real id parses, so the Err below is the id check.
        assert!(serde_json::from_str::<SearchHit>(&json("id")).is_ok());
        let err = serde_json::from_str::<SearchHit>(&json("")).unwrap_err();
        assert!(err.to_string().contains("empty provider track id"));
    }

    #[test]
    fn getters_return_values_given_to_new() {
        let mut h = SearchHit::new(
            ProviderId::Spotify,
            "id".into(),
            "https://x".into(),
            "Title".into(),
            vec!["A".into()],
        )
        .unwrap();
        assert_eq!(h.provider(), ProviderId::Spotify);
        assert_eq!(h.provider_track_id(), "id");
        assert_eq!(h.canonical_url(), "https://x");
        assert_eq!(h.title(), "Title");
        assert_eq!(h.artists(), vec!["A".to_string()].as_slice());
        assert_eq!(h.artwork_url(), None);
        assert_eq!(h.duration_ms(), None);
        // new() does not take these two, so set the fields directly to test their getters.
        h.artwork_url = Some("https://img/x.jpg".into());
        h.duration_ms = Some(42);
        assert_eq!(h.artwork_url(), Some("https://img/x.jpg"));
        assert_eq!(h.duration_ms(), Some(42));
    }
}
