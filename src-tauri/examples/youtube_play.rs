//! Manual native audio check: cargo run --example youtube_play -- <YouTube video ID> [seconds, default 2; muted] [--session] [--cache-dir <path>] [--offline: network requests time out]
use lunorx_lib::core::{
    capabilities::{CapabilityStatus, ProviderCapabilities},
    domain::{ProviderId, TrackReference},
    playback::{
        command::{PlaybackCommand, Volume},
        engine::PlaybackEngine,
        extract,
        host::PlayerHost,
        native::NativeHost,
    },
    registry::ProviderRegistry,
};
use std::{collections::HashMap, time::Duration};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let id = std::env::args().nth(1).ok_or("pass a YouTube video ID")?;
    let seconds = std::env::args()
        .nth(2)
        .map(|s| s.parse::<u64>())
        .transpose()?
        .unwrap_or(2)
        .clamp(2, 600);
    let (sender, mut reports) = tokio::sync::mpsc::unbounded_channel();
    let host = if std::env::args().any(|arg| arg == "--session") {
        let yt = extract::resolver()?;
        let cookie = keyring::Entry::new("io.lunorx.desktop", "provider-youtube-session-cookie")
            .map_err(|_| "session storage unavailable")?
            .get_password()
            .map_err(|error| match error {
                keyring::Error::NoEntry => "session not found in this keyring slot",
                _ => "session keyring access refused or unavailable",
            })?;
        tokio::time::timeout(Duration::from_secs(20), yt.connect(&cookie))
            .await?
            .map_err(|_| "session validation failed")?;
        println!("local YouTube session validated; credentials kept private");
        NativeHost::with_yt(yt, sender)?
    } else if std::env::args().any(|a| a == "--offline") {
        let yt = lunorx_lib::core::yt::Yt::new(Duration::from_nanos(1))?;
        NativeHost::with_yt(yt, sender)?
    } else {
        NativeHost::new(sender)?
    };
    let args: Vec<_> = std::env::args().collect();
    let host = if let Some(index) = args.iter().position(|a| a == "--cache-dir") {
        let path = args.get(index + 1).ok_or("pass a cache directory")?;
        let cache = lunorx_lib::core::playback::cache::DiskCache::open(path.into())
            .map_err(|_| "cache unavailable")?;
        host.with_cache(cache)
    } else {
        host
    };
    let mut registry = ProviderRegistry::new();
    registry.register(
        ProviderId::YouTube,
        ProviderCapabilities {
            playback: CapabilityStatus::Supported,
            ..ProviderCapabilities::unverified()
        },
    )?;
    let hosts: HashMap<_, Box<dyn PlayerHost>> =
        HashMap::from([(ProviderId::YouTube, Box::new(host) as _)]);
    let mut engine = PlaybackEngine::new(registry, hosts, Duration::from_secs(55));
    let track = TrackReference::new(
        ProviderId::YouTube,
        id.clone(),
        format!("https://www.youtube.com/watch?v={id}"),
        "Live playback test".into(),
    )?;
    engine.dispatch(PlaybackCommand::Load(track)).await?;
    println!("native load confirmed");
    engine
        .dispatch(PlaybackCommand::SetVolume(Volume::new(0)?))
        .await?;
    engine.dispatch(PlaybackCommand::Play).await?;
    println!("native play confirmed muted");
    let started = std::time::Instant::now();
    let mut last = 0;
    let mut duration_ms = 0;
    tokio::time::timeout(Duration::from_secs(seconds + 30), async {
        while let Some(report) = reports.recv().await {
            if report.position_ms / 5000 > last {
                last = report.position_ms / 5000;
                println!(
                    "progress={}ms wall={}s",
                    report.position_ms,
                    started.elapsed().as_secs()
                );
            }
            if matches!(
                report.event,
                Some(
                    lunorx_lib::core::playback::host::PlayerEvent::Failed
                        | lunorx_lib::core::playback::host::PlayerEvent::Ended
                )
            ) {
                panic!(
                    "stream stopped at {}ms: {:?}",
                    report.position_ms, report.event
                );
            }
            if report.position_ms >= seconds * 1000 && report.duration_ms > 0 {
                duration_ms = report.duration_ms;
                println!(
                    "native position={}ms duration={}ms",
                    report.position_ms, report.duration_ms
                );
                return;
            }
        }
        panic!("native player ended without progress");
    })
    .await?;
    engine.dispatch(PlaybackCommand::Pause).await?;
    engine.dispatch(PlaybackCommand::Resume).await?;
    engine
        .dispatch(PlaybackCommand::Seek {
            position_ms: 10_000,
        })
        .await?;
    engine
        .dispatch(PlaybackCommand::Seek {
            position_ms: duration_ms.saturating_sub(3000),
        })
        .await?;
    tokio::time::timeout(Duration::from_secs(12), async {
        while let Some(report) = reports.recv().await {
            match report.event {
                Some(lunorx_lib::core::playback::host::PlayerEvent::Failed) => {
                    panic!("tail decoding failed")
                }
                Some(lunorx_lib::core::playback::host::PlayerEvent::Ended) => {
                    println!("final audio bytes decoded; natural end confirmed");
                    return;
                }
                _ => {}
            }
        }
        panic!("native player ended without tail confirmation");
    })
    .await?;
    engine.dispatch(PlaybackCommand::Stop).await?;
    println!(
        "pause/resume/seek/stop confirmed; final state {:?}",
        engine.state()
    );
    Ok(())
}
