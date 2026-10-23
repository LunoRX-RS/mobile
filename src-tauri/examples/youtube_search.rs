//! Manual direct-search check; does not load saved credentials or need an API key.
use lunorx_lib::core::{
    playback::extract,
    search::{provider::SearchProvider, youtube_music::YouTubeMusicSearch},
};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let query = std::env::args().nth(1).unwrap_or_else(|| "lofi".into());
    let provider = YouTubeMusicSearch::new(extract::resolver()?);
    let hits = provider.search(&query, 10).await?;
    assert!(
        !hits.is_empty(),
        "direct YouTube Music search returned no songs"
    );
    assert!(
        hits.iter()
            .all(|hit| extract::valid_id(hit.provider_track_id()))
    );
    let music = hits
        .iter()
        .filter(|h| h.source() == Some(lunorx_lib::core::search::hit::SearchSource::YouTubeMusic))
        .count();
    let videos = hits
        .iter()
        .filter(|h| h.source() == Some(lunorx_lib::core::search::hit::SearchSource::YouTube))
        .count();
    println!(
        "Unified search returned {music} YouTube Music and {videos} YouTube results without an API key"
    );
    Ok(())
}
