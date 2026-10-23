//! Manual network check: cargo run --example youtube_resolve -- <YouTube video ID>
use lunorx_lib::core::playback::extract;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args().nth(1).ok_or("pass a YouTube video ID")?;
    let pipe = extract::resolver()?;
    let audio = extract::resolve(&pipe, &id).await?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()?;
    let mut response = client
        .get(audio.url)
        .header("Range", "bytes=0-1023")
        .send()
        .await?;
    println!("stream HTTP status: {}", response.status());
    response.error_for_status_ref()?;
    let chunk = response.chunk().await?.ok_or("empty audio response")?;
    assert!(!chunk.is_empty(), "empty audio response");
    println!(
        "received {} audio bytes; signed URL kept private",
        chunk.len()
    );
    Ok(())
}
