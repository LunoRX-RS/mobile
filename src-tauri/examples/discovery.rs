//! Live provider check; prints counts only, never lyrics, tokens or signed audio URLs.
use lunorx_lib::core::{discovery, playback::extract};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipe = extract::resolver()?;
    for (kind, topic, seed) in [
        ("home", "charts", Some("r-G2pKr2_n8")),
        ("explore", "charts", None),
        ("explore", "focus", None),
    ] {
        let feed = discovery::feed(&pipe, kind, topic, seed)
            .await
            .map_err(|_| "provider feed unavailable")?;
        let count: usize = feed.iter().map(|s| s.tracks.len()).sum();
        assert!(count > 0);
        assert!(
            feed.iter()
                .flat_map(|s| &s.tracks)
                .all(|t| extract::valid_id(&t.provider_track_id))
        );
        println!("{kind}/{topic}: {count} real tracks");
    }
    let suggestions = discovery::suggestions(&pipe, "Rick Astley")
        .await
        .map_err(|_| "suggestions unavailable")?;
    assert!(!suggestions.is_empty());
    println!("Search suggestions: {}", suggestions.len());
    Ok(())
}
