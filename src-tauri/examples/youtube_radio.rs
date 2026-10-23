//! Manual radio/preload smoke check; uses no saved credentials and never prints signed URLs.
use lunorx_lib::core::playback::{audio::AudioQuality, autoplay, cache::DiskCache, extract};
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let seed = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "r-G2pKr2_n8".into());
    assert!(extract::valid_id(&seed));
    let pipe = extract::resolver()?;
    let candidates = autoplay::related(
        &pipe,
        &seed,
        &std::collections::VecDeque::from([seed.clone()]),
        vec![],
    )
    .await;
    println!("Radio returned {} unique upcoming songs", candidates.len());
    assert_eq!(
        candidates.len(),
        50,
        "expected a 50-song radio queue for the smoke-test seed"
    );
    assert!(candidates.iter().all(|t| t.id != seed));
    assert_eq!(
        candidates
            .iter()
            .map(|t| &t.id)
            .collect::<std::collections::HashSet<_>>()
            .len(),
        50
    );
    let root =
        std::env::temp_dir().join(format!("lunor-radio-check-{:032x}", rand::random::<u128>()));
    let result = async {
        let cache = DiskCache::open(root.clone()).map_err(|_| "cache open failed")?;
        for track in candidates.into_iter().take(3) {
            if let Ok(Ok(file)) = tokio::time::timeout(
                std::time::Duration::from_secs(90),
                cache.ensure(&pipe, track, AudioQuality::High),
            )
            .await
            {
                assert!(file.size > 0);
                assert_eq!(std::fs::metadata(&file.path).unwrap().len(), file.size);
                println!("Prepared complete related-track file: {} bytes", file.size);
                return Ok(());
            }
        }
        Err("no candidate could be preloaded")
    }
    .await;
    let _ = std::fs::remove_dir_all(root);
    result.map_err(Into::into)
}
