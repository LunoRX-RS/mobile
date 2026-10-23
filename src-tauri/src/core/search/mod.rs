//! Search engine core (Phase 3). Submodules and re-exports are added per task.
pub mod cancel;
pub mod engine;
pub mod hit;
pub mod provider;
pub mod quota;
pub mod request;
pub mod response;
pub mod youtube;
pub mod youtube_music;

pub use cancel::CancelToken;
pub use engine::SearchEngine;
pub use hit::SearchHit;
pub use provider::{SearchFuture, SearchProvider};
pub use quota::QuotaBudget;
pub use request::SearchRequest;
pub use response::{GroupStatus, SearchResponse, SkipReason, SourceGroup, ordered};
