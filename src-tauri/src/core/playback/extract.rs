//! YouTube resolution stays on the desktop; signed URLs never enter IPC or storage.
//!
//! One `player` request per attempt, made the way the site's own pages make it (web music client, the user's own
//! session cookie when connected). A sign-in or verification challenge is reported as a typed error and nothing is
//! retried, rotated or worked around.
use super::audio::AudioQuality;
use super::diag::stage_log;
use crate::core::{
    errors::{HostRejectKind, PlaybackErrorKind},
    yt::Yt,
};
use innerlight::{
    ClientKind,
    player::{Player, PlayerError, Quality, ResolveOptions, Resolved, request_body},
};
use serde_json::Value;
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
        PlayerError::LoginRequired { .. } | PlayerError::Transport(innerlight::Error::BotCheck) => {
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
    let stream = match tokio::time::timeout(Duration::from_secs(40), player(yt, id, quality)).await
    {
        Err(_) => {
            stage_log!("[debug] resolve: timed out after 40s for {id}");
            return Err(PlaybackErrorKind::HostRejected(HostRejectKind::Timeout));
        }
        Ok(Err(e)) => {
            stage_log!("[debug] resolve: player error for {id}: {e:?}");
            return Err(rejection(e));
        }
        Ok(Ok(stream)) => stream,
    };
    let url = match checked_url(&stream.url) {
        Ok(url) => url,
        Err(kind) => {
            stage_log!("[debug] resolve: stream url rejected for {id}");
            return Err(kind);
        }
    };
    stage_log!(
        "[debug] resolve: ok {id} host={} size={}",
        reqwest::Url::parse(&url)
            .ok()
            .and_then(|u| u.host_str().map(str::to_string))
            .unwrap_or_else(|| "-".into()),
        stream.size
    );
    Ok(ResolvedAudio {
        url,
        size: stream.size,
    })
}

/// The service request, plus the client's bounded retry when the failure is transient (a rate limit, a 5xx, an expired
/// deadline). A sign-in wall, a refusal or a challenge is reported at once and never sent again. The one repeat this
/// function adds is for a stale decipher script: it refetches the script, not the service's answer.
async fn player(yt: &Yt, id: &str, quality_wish: AudioQuality) -> Result<Resolved, PlayerError> {
    player_select(yt, id, quality_wish, cfg!(target_os = "ios")).await
}

/// `aac_only` drops WebM/Opus candidates before selection: iOS plays AAC in MP4 only.
async fn player_select(
    yt: &Yt,
    id: &str,
    quality_wish: AudioQuality,
    aac_only: bool,
) -> Result<Resolved, PlayerError> {
    let mut script_refreshed = false;
    loop {
        let script = yt.decipher().await;
        if script.is_none() {
            stage_log!("[debug] resolve: no decipher script available for {id}");
        }
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
        let body = request_body(id, options.signature_timestamp);
        let mut answer = if options.authed {
            yt.client()
                .post_authed(options.kind, "player", body)
                .await?
        } else {
            yt.client().post(options.kind, "player", body).await?
        };
        if aac_only {
            keep_audio_mp4(&mut answer);
        }
        match handle.resolve_response(&answer, options.quality) {
            Err(PlayerError::DecipherFailed) if script.is_some() && !script_refreshed => {
                stage_log!("[debug] resolve: decipher failed for {id}; fetching the script again");
                script_refreshed = true;
                yt.forget_script();
            }
            other => return other,
        }
    }
}

fn keep_audio_mp4(answer: &mut Value) {
    let Some(streaming) = answer
        .get_mut("streamingData")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    for key in ["adaptiveFormats", "formats"] {
        if let Some(list) = streaming.get_mut(key).and_then(Value::as_array_mut) {
            list.retain(|format| {
                format
                    .get("mimeType")
                    .and_then(Value::as_str)
                    .is_some_and(|mime| {
                        mime.trim_start()
                            .to_ascii_lowercase()
                            .starts_with("audio/mp4")
                    })
            });
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

    #[test]
    fn a_challenge_body_is_reported_as_a_verification_need() {
        assert_eq!(
            rejection(PlayerError::Transport(innerlight::Error::BotCheck)),
            PlaybackErrorKind::HostRejected(HostRejectKind::VerificationRequired)
        );
    }

    #[tokio::test]
    async fn ios_selection_never_picks_an_opus_stream() {
        let mut v = formats();
        v["streamingData"]["adaptiveFormats"]
            .as_array_mut()
            .unwrap()
            .push(json!({
                "itag": 251, "mimeType": "audio/webm; codecs=\"opus\"", "bitrate": 320_000,
                "contentLength": "100", "url": "https://rr1.googlevideo.com/opus"
            }));
        let (yt, _) = yt_with(offline_script().with("player", v.clone()));
        let best = player_select(&yt, "a_B-9zZ0-_x", AudioQuality::High, false)
            .await
            .unwrap();
        assert!(best.url.ends_with("/opus"), "default: {}", best.url);
        let (yt, _) = yt_with(offline_script().with("player", v));
        let aac = player_select(&yt, "a_B-9zZ0-_x", AudioQuality::High, true)
            .await
            .unwrap();
        assert!(aac.url.ends_with("/high"), "ios: {}", aac.url);
        assert_eq!(aac.mime_type, "audio/mp4; codecs=\"mp4a.40.2\"");
    }

    #[test]
    fn the_mp4_filter_only_touches_format_lists() {
        for mut v in [
            json!({}),
            json!({"streamingData": "not-an-object"}),
            json!({"streamingData": {"adaptiveFormats": "not-an-array"}}),
            json!({"streamingData": {"adaptiveFormats": [{"mimeType": "audio/mp4; codecs=\"mp4a.40.2\""}, {"mimeType": "audio/webm"}, {"bitrate": 1}]}}),
        ] {
            keep_audio_mp4(&mut v);
        }
        let mut v = json!({"streamingData": {"adaptiveFormats": [
            {"mimeType": "AUDIO/MP4; codecs=\"mp4a.40.2\""}, {"mimeType": " audio/mp4"}, {"mimeType": "audio/webm"}
        ]}});
        keep_audio_mp4(&mut v);
        assert_eq!(
            v["streamingData"]["adaptiveFormats"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
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
        // A 5xx is transient, so the client's bounded policy sends it again (three sends, its ceiling) and the answer
        // stays a failure. Refusals and challenges verified above are sent exactly once.
        assert_eq!(
            transport
                .requests()
                .iter()
                .filter(|r| r.endpoint == "player")
                .count(),
            3
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
