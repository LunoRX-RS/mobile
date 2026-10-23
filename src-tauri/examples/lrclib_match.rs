//! Live LRCLIB matching check without Google cookies. Never prints lyric text: only counts and flags.
use lunorx_lib::core::playback::lyrics;
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let cases: [(&str, &[&str], u32); 3] = [
        ("หนุ่มบ้านนอก (Remix)", &["SURIYA MQT", "P6ICK"], 244),
        ("BU", &["SURIYA MQT", "P6ICK"], 167),
        ("Blinding Lights", &["The Weeknd"], 200),
    ];
    let root =
        std::env::temp_dir().join(format!("lunor-live-match-{:032x}", rand::random::<u128>()));
    for (i, (title, names, secs)) in cases.into_iter().enumerate() {
        let artists: Vec<String> = names.iter().map(|n| (*n).into()).collect();
        let path = root.join(format!("{i}.json"));
        let started = std::time::Instant::now();
        let result = lyrics::fallback_cached(title, &artists, None, Some(secs), Some(&path)).await;
        let ms = started.elapsed().as_millis();
        match result {
            Err(_) => {
                println!("{title}: request failed (network, timeout or rate limit) after {ms} ms")
            }
            Ok(None) => println!(
                "{title}: no usable match after {ms} ms (cleaned query title: {})",
                lyrics::clean_title(title)
            ),
            Ok(Some(found)) => println!(
                "{title}: match in {ms} ms, cleaned={:?}, text_chars={}, timed_lines={}, timing_mismatch={}, instrumental={}",
                lyrics::clean_title(title),
                found.text.as_deref().map_or(0, |t| t.chars().count()),
                found.timed.as_ref().map_or(0, Vec::len),
                found.timing_mismatch,
                found.instrumental
            ),
        }
    }
    let _ = std::fs::remove_dir_all(root);
}
