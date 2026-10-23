//! Live check of the YouTube adapter against the real Data API.
//! Runs only when LUNORX_LIVE_YOUTUBE_KEY is set; otherwise it returns early. Costs one search.list unit.
//! The key is read from the environment, held in memory, and never printed.

use std::sync::{Arc, Mutex};

use lunorx_lib::core::search::SearchProvider;
use lunorx_lib::core::search::youtube::YouTubeSearch;
use lunorx_lib::sync::credentials::{TokenStore, TokenStoreError};

struct Key(Mutex<Option<String>>);

impl TokenStore for Key {
    fn get(&self) -> Result<Option<String>, TokenStoreError> {
        Ok(self.0.lock().unwrap().clone())
    }
    fn set(&self, t: &str) -> Result<(), TokenStoreError> {
        *self.0.lock().unwrap() = Some(t.into());
        Ok(())
    }
    fn clear(&self) -> Result<(), TokenStoreError> {
        *self.0.lock().unwrap() = None;
        Ok(())
    }
}

#[tokio::test]
async fn lofi_search_returns_a_hit_with_an_11_char_id() {
    let Ok(key) = std::env::var("LUNORX_LIVE_YOUTUBE_KEY") else {
        eprintln!("skipped: LUNORX_LIVE_YOUTUBE_KEY not set");
        return;
    };
    let provider = YouTubeSearch::new(Arc::new(Key(Mutex::new(Some(key))))).unwrap();
    let hits = provider
        .search("lofi", 5)
        .await
        .unwrap_or_else(|e| panic!("live search failed: {e}"));
    assert!(!hits.is_empty(), "expected at least one hit");
    for h in &hits {
        assert_eq!(h.provider_track_id().len(), 11);
        assert!(
            h.canonical_url()
                .starts_with("https://www.youtube.com/watch?v=")
        );
    }
}
