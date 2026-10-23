use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::Semaphore;
use tokio::task::JoinSet;

use crate::core::capabilities::CapabilityStatus;
use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::registry::ProviderRegistry;
use crate::core::search::{
    CancelToken, GroupStatus, QuotaBudget, SearchProvider, SearchRequest, SearchResponse,
    SkipReason, SourceGroup, ordered,
};

// shortcut: cancel is observed by polling every 10 ms because CancelToken is a bare AtomicBool, upgrade to a Notify in CancelToken when cancel latency matters
const CANCEL_POLL: Duration = Duration::from_millis(10);

pub struct SearchEngine {
    pub(crate) registry: ProviderRegistry,
    pub(crate) budgets: HashMap<ProviderId, QuotaBudget>,
    pub(crate) providers: Vec<Arc<dyn SearchProvider>>,
    pub(crate) timeout: Duration,
}

impl SearchEngine {
    // The caller decides which providers are registered (e.g. Spotify only in debug builds).
    pub fn new(
        registry: ProviderRegistry,
        budgets: HashMap<ProviderId, QuotaBudget>,
        providers: Vec<Arc<dyn SearchProvider>>,
        timeout: Duration,
    ) -> Self {
        Self {
            registry,
            budgets,
            providers,
            timeout,
        }
    }

    pub async fn search(
        &mut self,
        req: SearchRequest,
        cancel: CancelToken,
        today: u64,
    ) -> SearchResponse {
        let ids = if req.providers.is_empty() {
            ordered(&self.providers.iter().map(|p| p.id()).collect::<Vec<_>>())
        } else {
            ordered(&req.providers)
        };
        let sem = Arc::new(Semaphore::new(2));
        let mut slots: Vec<Option<SourceGroup>> = vec![None; ids.len()];
        let mut set = JoinSet::new();
        for (i, id) in ids.iter().enumerate() {
            let adapter = self.providers.iter().find(|p| p.id() == *id).cloned();
            // Decision table order: cancel, then capability, then quota (quota is consumed only when the call will run).
            let skip = if cancel.is_cancelled() {
                Some(SkipReason::Cancelled)
            } else if adapter.is_none()
                || self.registry.capabilities_for(id.clone()).search != CapabilityStatus::Supported
            {
                Some(SkipReason::CapabilityNotSupported)
            } else if !self
                .budgets
                .get_mut(id)
                .is_some_and(|b| b.try_consume(today))
            {
                Some(SkipReason::QuotaExhausted)
            } else {
                None
            };
            match (skip, adapter) {
                (Some(reason), _) => {
                    slots[i] = Some(SourceGroup::new(id.clone(), GroupStatus::Skipped(reason)))
                }
                (None, Some(provider)) => {
                    set.spawn(run_group(
                        i,
                        provider,
                        req.query.clone(),
                        req.limit,
                        sem.clone(),
                        cancel.clone(),
                        self.timeout,
                    ));
                }
                (None, None) => unreachable!("a missing adapter is always a capability skip"),
            }
        }
        while let Some(joined) = set.join_next().await {
            if let Ok((i, status)) = joined {
                slots[i] = Some(SourceGroup::new(ids[i].clone(), status));
            }
        }
        // A slot is empty only if its task panicked; report that group as Failed, keep the rest.
        let groups = slots
            .into_iter()
            .zip(ids)
            .map(|(slot, id)| {
                slot.unwrap_or_else(|| {
                    SourceGroup::new(
                        id,
                        GroupStatus::Failed(CoreError::Provider("adapter task panicked".into())),
                    )
                })
            })
            .collect();
        SearchResponse::new(req.query, groups)
    }
}

async fn run_group(
    index: usize,
    provider: Arc<dyn SearchProvider>,
    query: String,
    limit: u8,
    sem: Arc<Semaphore>,
    cancel: CancelToken,
    timeout: Duration,
) -> (usize, GroupStatus) {
    // The semaphore is owned by the engine for the whole search and never closed.
    let _permit = sem
        .acquire()
        .await
        .expect("search semaphore is never closed");
    if cancel.is_cancelled() {
        return (index, GroupStatus::Skipped(SkipReason::Cancelled));
    }
    (
        index,
        run_adapter(&*provider, &query, limit, &cancel, timeout).await,
    )
}

async fn run_adapter(
    provider: &dyn SearchProvider,
    query: &str,
    limit: u8,
    cancel: &CancelToken,
    timeout: Duration,
) -> GroupStatus {
    let work = tokio::time::timeout(timeout, provider.search(query, limit));
    tokio::pin!(work);
    loop {
        tokio::select! {
            // biased: a result that is already ready wins over a cancel observed in the same wakeup.
            biased;
            res = &mut work => {
                return match res {
                    Ok(Ok(hits)) => GroupStatus::Ok(hits),
                    Ok(Err(e)) => GroupStatus::Failed(e),
                    Err(_elapsed) => GroupStatus::Failed(CoreError::Network("timeout".into())),
                };
            }
            _ = tokio::time::sleep(CANCEL_POLL) => {
                if cancel.is_cancelled() {
                    return GroupStatus::Skipped(SkipReason::Cancelled);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::capabilities::{CapabilityStatus, ProviderCapabilities};
    use crate::core::domain::ProviderId::{Spotify, YouTube};
    use crate::core::search::{GroupStatus, SearchFuture, SearchHit, SearchProvider, SkipReason};
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Fake {
        id: ProviderId,
        delay: Duration,
        // None hangs forever: drives the timeout and cancel-while-running rows.
        reply: Option<Result<Vec<SearchHit>, CoreError>>,
        calls: Arc<AtomicUsize>,
    }

    impl SearchProvider for Fake {
        fn id(&self) -> ProviderId {
            self.id.clone()
        }

        fn search<'a>(&'a self, _query: &'a str, _limit: u8) -> SearchFuture<'a> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let (delay, reply) = (self.delay, self.reply.clone());
            Box::pin(async move {
                match reply {
                    None => std::future::pending().await,
                    Some(r) => {
                        tokio::time::sleep(delay).await;
                        r
                    }
                }
            })
        }
    }

    fn fake(
        id: ProviderId,
        delay_ms: u64,
        reply: Option<Result<Vec<SearchHit>, CoreError>>,
    ) -> Fake {
        Fake {
            id,
            delay: Duration::from_millis(delay_ms),
            reply,
            calls: Arc::new(AtomicUsize::new(0)),
        }
    }

    fn hit(id: ProviderId, track: &str) -> SearchHit {
        SearchHit::new(
            id,
            track.into(),
            format!("https://example.test/{track}"),
            "title".into(),
            vec![],
        )
        .unwrap()
    }

    fn engine(fakes: Vec<Fake>, search: CapabilityStatus, limit: u32) -> SearchEngine {
        let mut registry = ProviderRegistry::new();
        let mut budgets = HashMap::new();
        let mut providers: Vec<Arc<dyn SearchProvider>> = Vec::new();
        for f in fakes {
            registry
                .register(
                    f.id.clone(),
                    ProviderCapabilities {
                        search,
                        ..ProviderCapabilities::unverified()
                    },
                )
                .unwrap();
            budgets.insert(f.id.clone(), QuotaBudget::new(limit));
            providers.push(Arc::new(f));
        }
        SearchEngine::new(registry, budgets, providers, Duration::from_secs(8))
    }

    fn req(ids: Vec<ProviderId>) -> SearchRequest {
        SearchRequest::new("rock", ids, 5).unwrap()
    }

    #[tokio::test(start_paused = true)]
    async fn row1_cancelled_before_start_skips_without_calling_adapter() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let calls = f.calls.clone();
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let cancel = CancelToken::new();
        cancel.cancel();
        let res = e.search(req(vec![YouTube]), cancel, 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::Cancelled)
            )]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn row2_capability_not_supported_skips_without_calling_adapter() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let calls = f.calls.clone();
        let mut e = engine(vec![f], CapabilityStatus::Unsupported, 10);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::CapabilityNotSupported)
            )]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn row3_quota_exhausted_skips_without_calling_adapter() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let calls = f.calls.clone();
        let mut e = engine(vec![f], CapabilityStatus::Supported, 0);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::QuotaExhausted)
            )]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn missing_budget_denies_like_quota_exhausted() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let calls = f.calls.clone();
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        e.budgets.clear();
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::QuotaExhausted)
            )]
        );
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test(start_paused = true)]
    async fn row4_adapter_ok_in_time_returns_hits() {
        let f = fake(YouTube, 5, Some(Ok(vec![hit(YouTube, "a")])));
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Ok(vec![hit(YouTube, "a")])
            )]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn row5_adapter_error_becomes_failed() {
        let err = CoreError::Provider("boom".into());
        let f = fake(YouTube, 0, Some(Err(err.clone())));
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(YouTube, GroupStatus::Failed(err))]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn row6_timeout_becomes_network_failed() {
        let f = fake(YouTube, 0, None);
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Failed(CoreError::Network("timeout".into()))
            )]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn row7_cancelled_while_running_skips_and_discards() {
        let f = fake(YouTube, 0, None);
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let cancel = CancelToken::new();
        let (res, ()) = tokio::join!(e.search(req(vec![YouTube]), cancel.clone(), 1), async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            cancel.cancel();
        });
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::Cancelled)
            )]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn reversed_completion_keeps_ordered_output() {
        let spotify = fake(Spotify, 200, Some(Ok(vec![hit(Spotify, "s")])));
        let youtube = fake(YouTube, 10, Some(Ok(vec![hit(YouTube, "y")])));
        let mut e = engine(vec![spotify, youtube], CapabilityStatus::Supported, 10);
        let res = e
            .search(req(vec![YouTube, Spotify]), CancelToken::new(), 1)
            .await;
        assert_eq!(
            res.groups,
            vec![
                SourceGroup::new(Spotify, GroupStatus::Ok(vec![hit(Spotify, "s")])),
                SourceGroup::new(YouTube, GroupStatus::Ok(vec![hit(YouTube, "y")])),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn empty_request_means_all_registered_providers() {
        let spotify = fake(Spotify, 0, Some(Ok(vec![])));
        let youtube = fake(YouTube, 0, Some(Ok(vec![])));
        let mut e = engine(vec![youtube, spotify], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![]), CancelToken::new(), 1).await;
        let order: Vec<ProviderId> = res.groups.iter().map(|g| g.provider.clone()).collect();
        assert_eq!(order, vec![Spotify, YouTube]);
    }

    #[tokio::test(start_paused = true)]
    async fn all_providers_succeed_give_one_ok_group_each_in_order() {
        let spotify = fake(Spotify, 0, Some(Ok(vec![hit(Spotify, "s")])));
        let youtube = fake(YouTube, 0, Some(Ok(vec![hit(YouTube, "y")])));
        let mut e = engine(vec![youtube, spotify], CapabilityStatus::Supported, 10);
        let res = e
            .search(req(vec![YouTube, Spotify]), CancelToken::new(), 1)
            .await;
        assert_eq!(
            res.groups,
            vec![
                SourceGroup::new(Spotify, GroupStatus::Ok(vec![hit(Spotify, "s")])),
                SourceGroup::new(YouTube, GroupStatus::Ok(vec![hit(YouTube, "y")])),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn one_provider_failing_leaves_the_other_group_ok() {
        let err = CoreError::Provider("boom".into());
        let spotify = fake(Spotify, 0, Some(Err(err.clone())));
        let youtube = fake(YouTube, 0, Some(Ok(vec![hit(YouTube, "y")])));
        let mut e = engine(vec![spotify, youtube], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![
                SourceGroup::new(Spotify, GroupStatus::Failed(err)),
                SourceGroup::new(YouTube, GroupStatus::Ok(vec![hit(YouTube, "y")])),
            ]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn empty_results_are_ok_and_not_failed() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let mut e = engine(vec![f], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![SourceGroup::new(YouTube, GroupStatus::Ok(vec![]))]
        );
    }

    #[tokio::test(start_paused = true)]
    async fn same_title_from_two_providers_stays_two_groups() {
        let titled = |id: ProviderId| {
            SearchHit::new(
                id,
                "t1".into(),
                "https://example.test/t1".into(),
                "Same Title".into(),
                vec![],
            )
            .unwrap()
        };
        let spotify = fake(Spotify, 0, Some(Ok(vec![titled(Spotify)])));
        let youtube = fake(YouTube, 0, Some(Ok(vec![titled(YouTube)])));
        let mut e = engine(vec![spotify, youtube], CapabilityStatus::Supported, 10);
        let res = e.search(req(vec![]), CancelToken::new(), 1).await;
        assert_eq!(
            res.groups,
            vec![
                SourceGroup::new(Spotify, GroupStatus::Ok(vec![titled(Spotify)])),
                SourceGroup::new(YouTube, GroupStatus::Ok(vec![titled(YouTube)])),
            ]
        );
    }

    // The engine never receives an invalid request: SearchRequest::new rejects it first,
    // so no search runs and the registered fake is never called.
    #[tokio::test(start_paused = true)]
    async fn invalid_query_or_limit_fails_before_any_adapter_call() {
        let f = fake(YouTube, 0, Some(Ok(vec![])));
        let calls = f.calls.clone();
        let _e = engine(vec![f], CapabilityStatus::Supported, 10);
        assert!(matches!(
            SearchRequest::new("  ", vec![YouTube], 5),
            Err(CoreError::Validation(_))
        ));
        assert!(matches!(
            SearchRequest::new("rock", vec![YouTube], 0),
            Err(CoreError::Validation(_))
        ));
        assert!(matches!(
            SearchRequest::new("rock", vec![YouTube], 11),
            Err(CoreError::Validation(_))
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
    }

    // The engine takes only SearchHit values, so a hit with an empty id cannot reach it.
    #[tokio::test(start_paused = true)]
    async fn malformed_hit_is_rejected_at_construction() {
        let r = SearchHit::new(
            YouTube,
            "  ".into(),
            "https://example.test/x".into(),
            "title".into(),
            vec![],
        );
        assert!(matches!(r, Err(CoreError::Validation(_))));
    }

    // Review item 4: a provider that already returned Ok keeps its Ok when cancel fires later.
    #[tokio::test(start_paused = true)]
    async fn cancel_after_a_provider_returned_keeps_its_ok_group() {
        let spotify = fake(Spotify, 0, Some(Ok(vec![hit(Spotify, "s")])));
        let youtube = fake(YouTube, 0, None);
        let mut e = engine(vec![spotify, youtube], CapabilityStatus::Supported, 10);
        let cancel = CancelToken::new();
        let (res, ()) = tokio::join!(e.search(req(vec![]), cancel.clone(), 1), async {
            tokio::time::sleep(Duration::from_secs(1)).await;
            cancel.cancel();
        });
        assert_eq!(
            res.groups,
            vec![
                SourceGroup::new(Spotify, GroupStatus::Ok(vec![hit(Spotify, "s")])),
                SourceGroup::new(YouTube, GroupStatus::Skipped(SkipReason::Cancelled)),
            ]
        );
    }

    // Review item 5, engine level: the quota budget resets when the day number changes.
    #[tokio::test(start_paused = true)]
    async fn quota_exhausted_on_day_n_succeeds_on_day_n_plus_1() {
        let f = fake(YouTube, 0, Some(Ok(vec![hit(YouTube, "a")])));
        let mut e = engine(vec![f], CapabilityStatus::Supported, 1);
        let first = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            first.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Ok(vec![hit(YouTube, "a")])
            )]
        );
        let exhausted = e.search(req(vec![YouTube]), CancelToken::new(), 1).await;
        assert_eq!(
            exhausted.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Skipped(SkipReason::QuotaExhausted)
            )]
        );
        let next_day = e.search(req(vec![YouTube]), CancelToken::new(), 2).await;
        assert_eq!(
            next_day.groups,
            vec![SourceGroup::new(
                YouTube,
                GroupStatus::Ok(vec![hit(YouTube, "a")])
            )]
        );
    }
}
