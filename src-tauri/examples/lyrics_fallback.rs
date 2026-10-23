//! Live LRCLIB check without Google cookies; lyric text is never printed.
use lunorx_lib::core::playback::lyrics;
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root =
        std::env::temp_dir().join(format!("lunor-live-lyrics-{:032x}", rand::random::<u128>()));
    let path = root.join("song.json");
    let result = lyrics::fallback_cached(
        "Never Gonna Give You Up",
        &["Rick Astley".into()],
        Some("Whenever You Need Somebody"),
        Some(213),
        Some(&path),
    )
    .await
    .map_err(|_| "LRCLIB request failed")?
    .ok_or("no matching LRCLIB record")?;
    assert!(!result.instrumental);
    let lines = result
        .timed
        .ok_or("LRCLIB record has no synced timestamps")?;
    assert!(lines.len() >= 4);
    let text = result.text.ok_or("no lyric text")?;
    assert!(!text.trim().is_empty());
    let info = lunorx_lib::core::playback::track_info::cached_lrclib("dQw4w9WgXcQ", &path)
        .ok_or("local karaoke cache miss")?;
    assert_eq!(info.lyrics_lines.as_ref(), Some(&lines));
    println!(
        "LRCLIB karaoke OK: {} timed lines; local cache restored; no Google cookies used",
        lines.len()
    );
    let _ = std::fs::remove_dir_all(root);
    Ok(())
}
