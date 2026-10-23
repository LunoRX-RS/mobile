use serde::{Deserialize, Serialize};

use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::search::{GroupStatus, SearchHit, SearchResponse, SkipReason, SourceGroup};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchQueryArgs {
    pub query: String,
    pub providers: Vec<String>, // "Spotify" | "YouTube"; empty = all
    pub limit: u8,
    pub cancellation_id: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchResponseDto {
    pub query: String,
    pub groups: Vec<SourceGroupDto>,
    pub hits: Vec<SearchHitDto>,
}

#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SourceGroupDto {
    Ok {
        provider: String,
        hits: Vec<SearchHitDto>,
    },
    Failed {
        provider: String,
        category: String,
    },
    Skipped {
        provider: String,
        reason: String,
    },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHitDto {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<crate::core::search::hit::SearchSource>,
    pub provider: String,
    pub provider_track_id: String,
    pub canonical_url: String,
    pub title: String,
    pub artists: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub artist_refs: Vec<crate::core::search::hit::ArtistRef>,
    pub artwork_url: Option<String>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderStatusDto {
    pub provider: String,
    pub search: String,
    pub playback: String,
    pub external_playback: String,
}

#[derive(Debug, Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum IpcError {
    Validation { message: String },
    DuplicateCancellationId,
    Internal,
}

impl From<&SearchHit> for SearchHitDto {
    fn from(h: &SearchHit) -> Self {
        Self {
            provider: h.provider().as_str().to_owned(),
            source: h.source(),
            provider_track_id: h.provider_track_id().to_owned(),
            canonical_url: h.canonical_url().to_owned(),
            title: h.title().to_owned(),
            artists: h.artists().to_vec(),
            artist_refs: h.artist_refs().to_vec(),
            artwork_url: h.artwork_url().map(str::to_owned),
            duration_ms: h.duration_ms(),
        }
    }
}

// Category names only: a CoreError message can echo provider text, so it never reaches the DTO.
fn category(e: &CoreError) -> &'static str {
    match e {
        CoreError::Search(_) => "searchError",
        CoreError::Provider(_) => "providerError",
        CoreError::ProviderUnavailable => "providerUnavailable",
        CoreError::AuthenticationRequired => "authenticationRequired",
        CoreError::QuotaExceeded => "quotaExceeded",
        CoreError::UnsupportedCapability => "unsupportedCapability",
        CoreError::Playback(_) => "playbackError",
        CoreError::Queue(_) => "queueError",
        CoreError::Playlist(_) => "playlistError",
        CoreError::Validation(_) => "validation",
        CoreError::Cache(_) => "cacheError",
        CoreError::Network(_) => "networkError",
    }
}

impl From<&SourceGroup> for SourceGroupDto {
    fn from(g: &SourceGroup) -> Self {
        let provider = g.provider().as_str().to_owned();
        match g.status() {
            GroupStatus::Ok(hits) => Self::Ok {
                provider,
                hits: hits.iter().map(SearchHitDto::from).collect(),
            },
            GroupStatus::Failed(e) => Self::Failed {
                provider,
                category: category(e).to_owned(),
            },
            GroupStatus::Skipped(r) => Self::Skipped {
                provider,
                reason: match r {
                    SkipReason::CapabilityNotSupported => "capabilityNotSupported",
                    SkipReason::QuotaExhausted => "quotaExhausted",
                    SkipReason::Cancelled => "cancelled",
                }
                .to_owned(),
            },
        }
    }
}

impl From<&SearchResponse> for SearchResponseDto {
    fn from(r: &SearchResponse) -> Self {
        Self {
            query: r.query().to_owned(),
            groups: r.groups().iter().map(SourceGroupDto::from).collect(),
            hits: r
                .combined_hits()
                .into_iter()
                .map(SearchHitDto::from)
                .collect(),
        }
    }
}

impl From<(ProviderId, &ProviderCapabilities)> for ProviderStatusDto {
    fn from((p, caps): (ProviderId, &ProviderCapabilities)) -> Self {
        Self {
            provider: p.as_str().to_owned(),
            search: status_name(caps.search).to_owned(),
            playback: status_name(caps.playback).to_owned(),
            external_playback: status_name(caps.external_playback).to_owned(),
        }
    }
}

fn status_name(s: CapabilityStatus) -> &'static str {
    match s {
        CapabilityStatus::Supported => "Supported",
        CapabilityStatus::Unsupported => "Unsupported",
        CapabilityStatus::RequiresAuth => "RequiresAuth",
        CapabilityStatus::RequiresEntitlement => "RequiresEntitlement",
        CapabilityStatus::TemporarilyUnavailable => "TemporarilyUnavailable",
        CapabilityStatus::Unverified => "Unverified",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn hit() -> SearchHit {
        SearchHit::new(
            ProviderId::Spotify,
            "t1".into(),
            "https://open.spotify.com/track/t1".into(),
            "Song".into(),
            vec!["Artist".into()],
        )
        .unwrap()
    }

    fn to_json(r: &SearchResponse) -> serde_json::Value {
        serde_json::to_value(SearchResponseDto::from(r)).unwrap()
    }

    #[test]
    fn ok_group_golden_json() {
        let r = SearchResponse::new(
            "song".into(),
            vec![SourceGroup::new(
                ProviderId::Spotify,
                GroupStatus::Ok(vec![hit()]),
            )],
        );
        assert_eq!(
            to_json(&r),
            json!({
                "query": "song",
                "hits": [{"provider":"Spotify","providerTrackId":"t1","canonicalUrl":"https://open.spotify.com/track/t1","title":"Song","artists":["Artist"],"artworkUrl":null,"durationMs":null}],
                "groups": [{
                    "kind": "ok",
                    "provider": "Spotify",
                    "hits": [{
                        "provider": "Spotify",
                        "providerTrackId": "t1",
                        "canonicalUrl": "https://open.spotify.com/track/t1",
                        "title": "Song",
                        "artists": ["Artist"],
                        "artworkUrl": null,
                        "durationMs": null
                    }]
                }]
            })
        );
    }

    #[test]
    fn failed_group_uses_category_and_hides_message() {
        let r = SearchResponse::new(
            "song".into(),
            vec![SourceGroup::new(
                ProviderId::YouTube,
                GroupStatus::Failed(CoreError::Network("secret".into())),
            )],
        );
        let v = to_json(&r);
        assert_eq!(
            v["groups"][0],
            json!({"kind": "failed", "provider": "YouTube", "category": "networkError"})
        );
        assert!(!v.to_string().contains("secret"));
    }

    #[test]
    fn skipped_group_golden_json() {
        let r = SearchResponse::new(
            "song".into(),
            vec![SourceGroup::new(
                ProviderId::Spotify,
                GroupStatus::Skipped(SkipReason::CapabilityNotSupported),
            )],
        );
        assert_eq!(
            to_json(&r)["groups"][0],
            json!({"kind": "skipped", "provider": "Spotify", "reason": "capabilityNotSupported"})
        );
    }

    #[test]
    fn validation_error_serializes_with_category() {
        let v = serde_json::to_value(IpcError::Validation {
            message: "empty query".into(),
        })
        .unwrap();
        assert_eq!(
            v,
            json!({"category": "validation", "message": "empty query"})
        );
    }

    #[test]
    fn duplicate_cancellation_id_serializes_with_category() {
        let v = serde_json::to_value(IpcError::DuplicateCancellationId).unwrap();
        assert_eq!(v, json!({"category": "duplicateCancellationId"}));
    }

    #[test]
    fn search_query_args_deserialize_camel_case() {
        let a: SearchQueryArgs = serde_json::from_value(json!({
            "query": "x",
            "providers": [],
            "limit": 5,
            "cancellationId": "c1"
        }))
        .unwrap();
        assert_eq!(a.query, "x");
        assert!(a.providers.is_empty());
        assert_eq!(a.limit, 5);
        assert_eq!(a.cancellation_id, "c1");
    }

    #[test]
    fn provider_status_maps_each_capability_status() {
        let s = |st| {
            let caps = ProviderCapabilities {
                search: st,
                ..ProviderCapabilities::unverified()
            };
            ProviderStatusDto::from((ProviderId::YouTube, &caps))
        };
        assert_eq!(s(CapabilityStatus::Unverified).search, "Unverified");
        assert_eq!(s(CapabilityStatus::Supported).search, "Supported");
        assert_eq!(s(CapabilityStatus::RequiresAuth).provider, "YouTube");
    }
}
