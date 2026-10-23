//! YouTube resolution stays on the desktop; signed URLs never enter IPC or storage.
//!
//! One `player` request per attempt, made the way the site's own pages make it (web music client, the user's own
//! session cookie when connected). A sign-in or verification challenge is reported as a typed error and nothing is
//! retried, rotated or worked around.
use super::audio::AudioQuality;
use crate::core::{
    errors::{HostRejectKind, PlaybackErrorKind},
    yt::Yt,
};
use innerlight::{
    ClientKind,
    player::{Player, PlayerError, Quality, ResolveOptions, Resolved},
};
use std::time::Duration;

pub fn valid_id(id: &str) -> bool {
    id.len() == 11
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
pub(super) fn failed() -> PlaybackErrorKind {
    PlaybackErrorKind::HostRejected(HostRejectKind::Failed)
}
pub fn resolver() -> Result<Yt, PlaybackErrorKind> {
    Yt::new(Duration::from_secs(12)).map_err(|_| failed())
}
pub struct ResolvedAudio {
    pub url: String,
    pub size: u64,
}

fn quality(q: AudioQuality) -> Quality {
    match q {
        AudioQuality::Low => Quality::Low,
        AudioQuality::Normal => Quality::Normal,
        AudioQuality::High => Quality::High,
    }
}

/// Only a sign-in or bot challenge has its own category, so the UI can say what to do. Everything else is "failed".
fn rejection(error: PlayerError) -> PlaybackErrorKind {
    match error {
        PlayerError::LoginRequired { .. } => {
            PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired)
        }
        _ => failed(),
    }
}

pub async fn resolve(yt: &Yt, id: &str) -> Result<ResolvedAudio, PlaybackErrorKind> {
    resolve_with_quality(yt, id, AudioQuality::High).await
}

pub async fn resolve_with_quality(
    yt: &Yt,
    id: &str,
    quality: AudioQuality,
) -> Result<ResolvedAudio, PlaybackErrorKind> {
    if !valid_id(id) {
        return Err(PlaybackErrorKind::Validation("invalid YouTube id"));
    }
    let stream = tokio::time::timeout(Duration::from_secs(40), player(yt, id, quality))
        .await
        .map_err(|_| PlaybackErrorKind::HostRejected(HostRejectKind::Timeout))?
        .map_err(rejection)?;
    Ok(ResolvedAudio {
        url: checked_url(&stream.url)?,
        size: stream.size,
    })
}

/// The one service request, plus at most one repeat when the cached player script no longer deciphers (that repeat
/// fetches the script again; it is not a retry against the service's answer).
async fn player(yt: &Yt, id: &str, quality_wish: AudioQuality) -> Result<Resolved, PlayerError> {
    let mut script_refreshed = false;
    loop {
        let script = yt.decipher().await;
        let mut handle = Player::new(yt.client());
        if let Some(script) = &script {
            handle = handle.with_decipher(script.clone());
        }
        let options = ResolveOptions {
            kind: ClientKind::WebMusic,
            authed: yt.signed_in(),
            quality: quality(quality_wish),
            signature_timestamp: script.as_ref().and_then(|s| s.signature_timestamp().ok()),
        };
        match handle.resolve(id, options).await {
            Err(PlayerError::DecipherFailed) if script.is_some() && !script_refreshed => {
                script_refreshed = true;
                yt.forget_script();
            }
            other => return other,
        }
    }
}
pub(super) fn checked_url(raw: &str) -> Result<String, PlaybackErrorKind> {
    let url = reqwest::Url::parse(raw).map_err(|_| failed())?;
    if raw.len() > 16_384
        || raw.contains(['\n', '\r', '"', '\\'])
        || url.scheme() != "https"
        || !url
            .host_str()
            .is_some_and(|h| h.ends_with(".googlevideo.com"))
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return Err(failed());
    }
    Ok(url.into())
}
#[cfg(test)]
mod tests {
    use super::*;
    use innerlight::{Client, FixtureTransport};
    use serde_json::json;
    use std::sync::Arc;

    fn offline_script() -> FixtureTransport {
        // The player-script loader answers 404, so no deciphering is available and no GET reaches a network.
        FixtureTransport::new().with_raw("iframe_api", 404, b"")
    }
    fn yt_with(transport: FixtureTransport) -> (Yt, Arc<FixtureTransport>) {
        let transport = Arc::new(transport);
        let client = Client::builder()
            .transport(transport.clone())
            .build()
            .unwrap();
        (Yt::from_client(client), transport)
    }
    fn formats() -> serde_json::Value {
        let format = |bitrate: u32, url: &str, extra: serde_json::Value| {
            let mut f = json!({
                "itag": 140 + bitrate / 1000, "mimeType": "audio/mp4; codecs=\"mp4a.40.2\"",
                "bitrate": bitrate, "contentLength": "100", "url": url
            });
            if let (Some(f), Some(extra)) = (f.as_object_mut(), extra.as_object()) {
                f.extend(extra.clone());
            }
            f
        };
        json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"adaptiveFormats": [
            format(48_000, "https://rr1.googlevideo.com/low", json!({})),
            format(96_000, "https://rr1.googlevideo.com/mid", json!({})),
            format(160_000, "https://rr1.googlevideo.com/high", json!({})),
            format(256_000, "https://rr1.googlevideo.com/dub", json!({"audioTrack": {"id": "x", "displayName": "Dub", "audioIsDefault": false}})),
        ]}})
    }

    #[tokio::test]
    async fn quality_picks_the_bitrate_and_skips_non_default_tracks() {
        for (wish, suffix) in [
            (AudioQuality::Low, "/low"),
            (AudioQuality::Normal, "/mid"),
            (AudioQuality::High, "/high"),
        ] {
            let (yt, _) = yt_with(offline_script().with("player", formats()));
            let audio = resolve_with_quality(&yt, "a_B-9zZ0-_x", wish)
                .await
                .unwrap();
            assert!(audio.url.ends_with(suffix), "{wish:?}: {}", audio.url);
            assert_eq!(audio.size, 100);
        }
    }

    #[tokio::test]
    async fn a_url_outside_the_media_hosts_is_never_returned() {
        let mut v = formats();
        v["streamingData"]["adaptiveFormats"] = json!([{
            "itag": 140, "mimeType": "audio/mp4", "bitrate": 128000, "contentLength": "100",
            "url": "https://evil.test/audio"
        }]);
        let (yt, _) = yt_with(offline_script().with("player", v));
        assert!(
            resolve_with_quality(&yt, "a_B-9zZ0-_x", AudioQuality::High)
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn a_bot_check_is_a_typed_error_after_exactly_one_request() {
        let transport = offline_script().with_file(
            "player",
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/player_login_bot_check.json"
            ),
        );
        let (yt, transport) = yt_with(transport);
        let err = resolve_with_quality(&yt, "a_B-9zZ0-_x", AudioQuality::High)
            .await
            .map(|_| ())
            .unwrap_err();
        assert_eq!(
            err,
            PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired)
        );
        let posts: Vec<_> = transport
            .requests()
            .into_iter()
            .filter(|r| r.endpoint == "player")
            .collect();
        assert_eq!(posts.len(), 1, "no retry, no second client identity");
        assert_eq!(
            posts[0].body["context"]["client"]["clientName"],
            "WEB_REMIX"
        );
    }

    #[tokio::test]
    async fn other_refusals_stay_plain_failures() {
        for body in [
            json!({"playabilityStatus": {"status": "UNPLAYABLE", "reason": "Video unavailable"}}),
            json!({"playabilityStatus": {"status": "OK"}, "streamingData": {"adaptiveFormats": []}}),
            json!({"unrelated": true}),
        ] {
            let (yt, _) = yt_with(offline_script().with("player", body));
            assert_eq!(
                resolve_with_quality(&yt, "a_B-9zZ0-_x", AudioQuality::High)
                    .await
                    .map(|_| ())
                    .unwrap_err(),
                failed()
            );
        }
        let (yt, transport) = yt_with(offline_script().with_raw("player", 500, b"{}"));
        assert!(
            resolve_with_quality(&yt, "a_B-9zZ0-_x", AudioQuality::High)
                .await
                .is_err()
        );
        assert_eq!(
            transport
                .requests()
                .iter()
                .filter(|r| r.endpoint == "player")
                .count(),
            1
        );
    }

    #[tokio::test]
    async fn a_bad_id_sends_nothing() {
        let (yt, transport) = yt_with(offline_script());
        assert!(matches!(
            resolve_with_quality(&yt, "--exec=evil", AudioQuality::High)
                .await
                .map(|_| ())
                .unwrap_err(),
            PlaybackErrorKind::Validation(_)
        ));
        assert!(transport.requests().is_empty());
    }

    #[test]
    fn ids_and_stream_hosts_are_restricted() {
        assert!(valid_id("a_B-9zZ0-_x"));
        for id in ["", "--exec=evil", "dQw4w9WgXcQ&x", "dQw4w9WgX Q"] {
            assert!(!valid_id(id));
        }
        assert!(checked_url("https://rr1.googlevideo.com/videoplayback?expire=123").is_ok());
        for url in [
            "http://rr1.googlevideo.com/x",
            "https://googlevideo.com.evil.test/x",
            "https://evil.test/x",
            "file:///etc/passwd",
            "https://u@rr1.googlevideo.com/x",
            "https://rr1.googlevideo.com:444/x",
            "https://rr1.googlevideo.com/x\nhttps://rr2.googlevideo.com/x",
        ] {
            assert!(checked_url(url).is_err(), "{url}");
        }
    }
}
