use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::search::SearchHit;

#[derive(Debug, Clone, PartialEq)]
pub enum SkipReason {
    CapabilityNotSupported,
    QuotaExhausted,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq)]
pub enum GroupStatus {
    Ok(Vec<SearchHit>),
    Failed(CoreError),
    Skipped(SkipReason),
}

#[derive(Debug, Clone, PartialEq)]
pub struct SourceGroup {
    pub(crate) provider: ProviderId,
    pub(crate) status: GroupStatus,
}

impl SourceGroup {
    pub fn new(provider: ProviderId, status: GroupStatus) -> Self {
        Self { provider, status }
    }

    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }

    pub fn status(&self) -> &GroupStatus {
        &self.status
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchResponse {
    pub(crate) query: String,
    pub(crate) groups: Vec<SourceGroup>,
}

impl SearchResponse {
    pub fn new(query: String, groups: Vec<SourceGroup>) -> Self {
        Self { query, groups }
    }

    pub fn query(&self) -> &str {
        &self.query
    }

    pub fn combined_hits(&self) -> Vec<&SearchHit> {
        let groups: Vec<_> = self
            .groups
            .iter()
            .filter_map(|g| match &g.status {
                GroupStatus::Ok(h) => Some(h),
                _ => None,
            })
            .collect();
        let mut seen = std::collections::HashSet::new();
        let mut hits = Vec::new();
        for rank in 0..groups.iter().map(|g| g.len()).max().unwrap_or(0) {
            for group in &groups {
                if let Some(hit) = group.get(rank)
                    && seen.insert((hit.provider(), hit.provider_track_id().to_owned()))
                {
                    hits.push(hit);
                }
            }
        }
        hits
    }

    pub fn groups(&self) -> &[SourceGroup] {
        &self.groups
    }
}

// Exhaustive match: a new ProviderId variant fails to compile here instead of being dropped.
fn rank(p: &ProviderId) -> u8 {
    match p {
        ProviderId::Spotify => 0,
        ProviderId::YouTube => 1,
        ProviderId::Podcast => 2,
    }
}

pub fn ordered(providers: &[ProviderId]) -> Vec<ProviderId> {
    let mut v = providers.to_vec();
    v.sort_by_key(rank);
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::domain::ProviderId::{Spotify, YouTube};
    #[test]
    fn combined_keeps_provider_identity_and_rank() {
        let hit = |p, id: &str| {
            SearchHit::new(
                p,
                id.into(),
                "https://example.test".into(),
                "Same title".into(),
                vec![],
            )
            .unwrap()
        };
        let response = SearchResponse::new(
            "same".into(),
            vec![
                SourceGroup::new(
                    Spotify,
                    GroupStatus::Ok(vec![
                        hit(Spotify, "one"),
                        hit(Spotify, "two"),
                        hit(Spotify, "one"),
                    ]),
                ),
                SourceGroup::new(YouTube, GroupStatus::Ok(vec![hit(YouTube, "one")])),
            ],
        );
        let hits = response.combined_hits();
        assert_eq!(hits.len(), 3);
        assert_eq!(hits[0].provider(), Spotify);
        assert_eq!(hits[1].provider(), YouTube);
        assert_eq!(hits[2].provider_track_id(), "two");
    }

    #[test]
    fn ordered_sorts_into_declaration_order_and_dedups() {
        assert_eq!(
            ordered(&[YouTube, Spotify, YouTube]),
            vec![Spotify, YouTube]
        );
    }

    #[test]
    fn ordered_empty_is_empty() {
        assert!(ordered(&[]).is_empty());
    }

    #[test]
    fn response_keeps_group_order() {
        let r = SearchResponse::new(
            "rock".into(),
            vec![
                SourceGroup::new(Spotify, GroupStatus::Ok(vec![])),
                SourceGroup::new(YouTube, GroupStatus::Failed(CoreError::Search("x".into()))),
            ],
        );
        let order: Vec<ProviderId> = r.groups.iter().map(|g| g.provider.clone()).collect();
        assert_eq!(order, vec![Spotify, YouTube]);
    }

    #[test]
    fn group_statuses_are_distinct() {
        let ok = GroupStatus::Ok(vec![]);
        let failed = GroupStatus::Failed(CoreError::Search("x".into()));
        let cancelled = GroupStatus::Skipped(SkipReason::Cancelled);
        assert_ne!(ok, failed);
        assert_ne!(ok, cancelled);
        assert_ne!(failed, cancelled);
        assert_ne!(
            GroupStatus::Skipped(SkipReason::CapabilityNotSupported),
            GroupStatus::Skipped(SkipReason::QuotaExhausted)
        );
    }

    #[test]
    fn source_group_getters_return_new_args() {
        let g = SourceGroup::new(YouTube, GroupStatus::Skipped(SkipReason::QuotaExhausted));
        assert_eq!(g.provider(), &YouTube);
        assert_eq!(
            g.status(),
            &GroupStatus::Skipped(SkipReason::QuotaExhausted)
        );
    }

    #[test]
    fn search_response_getters_return_new_args() {
        let r = SearchResponse::new(
            "rock".into(),
            vec![SourceGroup::new(Spotify, GroupStatus::Ok(vec![]))],
        );
        assert_eq!(r.query(), "rock");
        assert_eq!(r.groups().len(), 1);
        assert_eq!(r.groups()[0].provider(), &Spotify);
    }
}
