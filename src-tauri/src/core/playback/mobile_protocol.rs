//! What the lunor-native plugin's `poll` command returns, and how it maps to engine events. No platform code here, so
//! it is compiled and tested on every target (the host that owns `mobile.rs` is a phone).
use super::host::PlayerEvent;
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Remote {
    pub kind: String,
    #[serde(default)]
    pub position_ms: f64,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Poll {
    pub status: String,
    pub position_ms: f64,
    pub duration_ms: f64,
    #[serde(default)]
    pub remote: Vec<Remote>,
}

/// A file that stops more than 2 s short is a failure, not a normal end (same rule as desktop).
pub fn end_event(position_ms: u64, duration_ms: u64) -> PlayerEvent {
    if duration_ms > 0 && duration_ms.saturating_sub(position_ms) > 2000 {
        PlayerEvent::Failed
    } else {
        PlayerEvent::Ended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn truncated_audio_is_failure_not_normal_end() {
        assert!(matches!(end_event(24_000, 166_000), PlayerEvent::Failed));
        assert!(matches!(end_event(165_000, 166_000), PlayerEvent::Ended));
        assert!(matches!(end_event(0, 0), PlayerEvent::Ended));
    }

    #[test]
    fn poll_payload_parses_with_and_without_remote_events() {
        let full = json!({"status":"playing","positionMs":1500.0,"durationMs":90000.0,
            "remote":[{"kind":"seek","positionMs":4000.0},{"kind":"next"}]});
        let poll: Poll = serde_json::from_value(full).unwrap();
        assert_eq!(poll.remote.len(), 2);
        assert_eq!(poll.remote[0].position_ms, 4000.0);
        assert_eq!(poll.remote[1].kind, "next");
        let bare = json!({"status":"idle","positionMs":0,"durationMs":0});
        assert!(
            serde_json::from_value::<Poll>(bare)
                .unwrap()
                .remote
                .is_empty()
        );
    }

    #[test]
    fn poll_payload_without_a_status_is_rejected() {
        assert!(serde_json::from_value::<Poll>(json!({"positionMs":1,"durationMs":2})).is_err());
    }
}
