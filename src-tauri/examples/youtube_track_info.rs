//! Live metadata/lyrics/artwork check; never prints lyrics or credentials.
use lunorx_lib::core::{
    playback::{extract, track_info},
    search::{provider::SearchProvider, youtube_music::YouTubeMusicSearch},
};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let pipe = extract::resolver()?;
    let hits = YouTubeMusicSearch::new(pipe.clone())
        .search("Rick Astley Never Gonna Give You Up", 1)
        .await?;
    let hit = hits.first().ok_or("no search result")?;
    let info = tokio::time::timeout(
        std::time::Duration::from_secs(25),
        track_info::fetch(&pipe, hit.provider_track_id()),
    )
    .await??;
    assert!(!info.title.is_empty());
    let cover = info.artwork_url.as_ref().ok_or("no artwork returned")?;
    let response = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?
        .get(cover)
        .send()
        .await?;
    response.error_for_status_ref()?;
    assert!(
        response
            .headers()
            .get("content-type")
            .and_then(|value| value.to_str().ok())
            .is_some_and(|value| value.starts_with("image/"))
    );
    println!(
        "metadata OK; artwork HTTP {}; lyrics status={}; lyrics characters={}",
        response.status(),
        info.lyrics_status,
        info.lyrics.as_ref().map_or(0, |text| text.chars().count())
    );
    Ok(())
}
