use std::future::Future;
use std::pin::Pin;

use crate::core::domain::ProviderId;
use crate::core::errors::CoreError;
use crate::core::search::hit::SearchHit;

// Boxed future instead of async-trait: keeps the trait object-safe with no new crate.
pub type SearchFuture<'a> =
    Pin<Box<dyn Future<Output = Result<Vec<SearchHit>, CoreError>> + Send + 'a>>;

pub trait SearchProvider: Send + Sync {
    fn id(&self) -> ProviderId;
    fn search<'a>(&'a self, query: &'a str, limit: u8) -> SearchFuture<'a>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeProvider {
        id: ProviderId,
    }

    impl SearchProvider for FakeProvider {
        fn id(&self) -> ProviderId {
            self.id.clone()
        }

        fn search<'a>(&'a self, _query: &'a str, _limit: u8) -> SearchFuture<'a> {
            Box::pin(async move { Ok(vec![]) })
        }
    }

    #[tokio::test]
    async fn boxed_provider_reports_id_and_resolves_empty() {
        let providers: Vec<Box<dyn SearchProvider>> = vec![Box::new(FakeProvider {
            id: ProviderId::YouTube,
        })];
        let p = &providers[0];
        assert_eq!(p.id(), ProviderId::YouTube);
        assert_eq!(p.search("q", 5).await, Ok(vec![]));
    }
}
