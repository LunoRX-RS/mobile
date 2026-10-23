//! Live probe of the extra lyric sources for two test tracks. Prints status, counts and latency per
//! source; lyric text is never printed. Sends only title, artist and duration (no credentials).
use lunorx_lib::core::playback::lyric_sources::{Ctx, Outcome, Query, SourceId, entry, run_one};
use std::time::Instant;

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let tracks = [
        ("Blinding Lights", "The Weeknd", 200_000),
        ("BU", "SURIYA MQT", 167_000),
    ];
    let ctx = Ctx::default();
    for (title, artist, duration_ms) in tracks {
        println!("== {title} / {artist} / {} s", duration_ms / 1000);
        let query = Query {
            title: title.into(),
            artists: vec![artist.into()],
            album: None,
            duration_ms,
        };
        for kind in SourceId::ALL {
            let started = Instant::now();
            let outcome = run_one(&ctx, kind, &query, None, true).await;
            let ms = started.elapsed().as_millis();
            let e = entry(kind, &outcome);
            let lines = e.lines.as_ref().map_or(0, Vec::len);
            let words: usize = e
                .lines
                .iter()
                .flatten()
                .map(|l| l.words.as_ref().map_or(0, Vec::len))
                .sum();
            let chars = e.text.as_ref().map_or(0, |t| t.chars().count());
            let note = if matches!(outcome, Outcome::Found(_)) {
                ""
            } else {
                "-"
            };
            println!(
                "{:<10} status={:<14} timestamps={:<5} words={:<5} lines={lines:<3} wordCount={words:<4} chars={chars:<5} {ms} ms {note}",
                kind.id(),
                e.status,
                e.synced,
                e.words,
            );
        }
    }
}
