//! Cross-device remote control client: one WebSocket to the backend (`/api/v1/devices/ws`, see backend
//! `docs/remote-control.md`). It only moves JSON text frames; what a frame means is decided by the webview, which
//! owns the player. The session token is read from the token store for each connection attempt, sent in the
//! `Authorization` header only, and never logged or handed to the webview.
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::{Error as WsError, Message};

use super::credentials::TokenStore;

/// Largest frame in either direction; the backend allows 4096 inbound.
pub const MAX_FRAME_BYTES: usize = 4096;
/// The backend pings every 25 s; silence this long means the link is dead.
const IDLE_LIMIT: Duration = Duration::from_secs(70);
const BACKOFF_MAX: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkStatus {
    Connecting,
    Online,
    Offline,
    /// No session token, or the backend refused it. The task has ended.
    SignedOut,
}

/// Percent-encodes everything except unreserved characters, so a device name cannot add query parameters.
fn encode(input: &str) -> String {
    let mut out = String::new();
    for b in input.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// `http(s)://host` becomes `ws(s)://host/api/v1/devices/ws?...`. `None` for any other scheme.
pub fn socket_url(base: &str, device_id: &str, name: &str, kind: &str) -> Option<String> {
    let rest = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else {
        format!("ws://{}", base.strip_prefix("http://")?)
    };
    Some(format!(
        "{}/api/v1/devices/ws?deviceId={}&name={}&kind={}",
        rest.trim_end_matches('/'),
        encode(device_id),
        encode(name),
        encode(kind)
    ))
}

/// `http(s)://host` becomes `ws(s)://host/api/v1/liked-songs/ws`. No query: the bearer token is the only credential and
/// it travels in the header. `None` for any other scheme.
pub fn likes_socket_url(base: &str) -> Option<String> {
    let rest = if let Some(rest) = base.strip_prefix("https://") {
        format!("wss://{rest}")
    } else {
        format!("ws://{}", base.strip_prefix("http://")?)
    };
    Some(format!(
        "{}/api/v1/liked-songs/ws",
        rest.trim_end_matches('/')
    ))
}

enum Ended {
    /// The caller stopped the link (the outbound channel closed).
    Stopped,
    /// The backend refused the token.
    Refused,
    /// Anything else: connect failure, dropped link, idle timeout.
    Lost { was_online: bool },
}

/// Keeps the link up until the outbound channel closes or the backend refuses the token. `inbound` gets every
/// text frame; `status` gets each change. Reconnects with a growing pause.
pub async fn run(
    url: String,
    tokens: &dyn TokenStore,
    mut outbound: mpsc::Receiver<String>,
    inbound: impl Fn(String),
    status: impl Fn(LinkStatus),
) {
    let mut pause = Duration::from_secs(1);
    loop {
        let token = match tokens.get() {
            Ok(Some(token)) => token,
            _ => return status(LinkStatus::SignedOut),
        };
        status(LinkStatus::Connecting);
        let started = Instant::now();
        match once(&url, &token, &mut outbound, &inbound, &status).await {
            Ended::Stopped => return,
            Ended::Refused => return status(LinkStatus::SignedOut),
            Ended::Lost { was_online } => {
                status(LinkStatus::Offline);
                // A link that held for a while earns a fresh short pause.
                if was_online && started.elapsed() > Duration::from_secs(30) {
                    pause = Duration::from_secs(1);
                }
            }
        }
        tokio::time::sleep(pause).await;
        pause = (pause * 2).min(BACKOFF_MAX);
    }
}

async fn once(
    url: &str,
    token: &str,
    outbound: &mut mpsc::Receiver<String>,
    inbound: &impl Fn(String),
    status: &impl Fn(LinkStatus),
) -> Ended {
    let lost = |was_online| Ended::Lost { was_online };
    let Ok(mut request) = url.into_client_request() else {
        return lost(false);
    };
    let Ok(auth) = format!("Bearer {token}").parse() else {
        return lost(false);
    };
    request.headers_mut().insert("authorization", auth);
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_FRAME_BYTES))
        .max_frame_size(Some(MAX_FRAME_BYTES));
    let mut socket =
        match tokio_tungstenite::connect_async_with_config(request, Some(config), false).await {
            Ok((socket, _)) => socket,
            Err(WsError::Http(response)) if response.status() == 401 => return Ended::Refused,
            Err(_) => return lost(false),
        };
    status(LinkStatus::Online);
    loop {
        tokio::select! {
            out = outbound.recv() => {
                let Some(text) = out else { return Ended::Stopped };
                if socket.send(Message::text(text)).await.is_err() { return lost(true) }
            }
            frame = tokio::time::timeout(IDLE_LIMIT, socket.next()) => match frame {
                Ok(Some(Ok(Message::Text(text)))) => inbound(text.as_str().to_owned()),
                Ok(Some(Ok(Message::Close(_)))) | Ok(None) | Ok(Some(Err(_))) | Err(_) => return lost(true),
                Ok(Some(Ok(_))) => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_maps_scheme_and_encodes_the_name() {
        let url = socket_url(
            "https://api.example.test",
            "id",
            "Ron's Mac & co",
            "computer",
        )
        .unwrap();
        assert_eq!(
            url,
            "wss://api.example.test/api/v1/devices/ws?deviceId=id&name=Ron%27s%20Mac%20%26%20co&kind=computer"
        );
        assert!(
            socket_url("http://127.0.0.1:8080/", "i", "n", "phone")
                .unwrap()
                .starts_with("ws://127.0.0.1:8080/api/")
        );
        assert_eq!(socket_url("ftp://x", "i", "n", "k"), None);
    }

    #[test]
    fn likes_url_has_no_query_and_switches_scheme() {
        assert_eq!(
            likes_socket_url("https://api.example.test/").unwrap(),
            "wss://api.example.test/api/v1/liked-songs/ws"
        );
        assert_eq!(
            likes_socket_url("http://127.0.0.1:8080").unwrap(),
            "ws://127.0.0.1:8080/api/v1/liked-songs/ws"
        );
        assert_eq!(likes_socket_url("ftp://x"), None);
    }

    use std::sync::{Arc, Mutex};

    use tokio::net::TcpListener;
    use tokio_tungstenite::tungstenite::handshake::server::{ErrorResponse, Request, Response};

    use super::super::credentials::TokenStoreError;

    struct Fixed(Option<&'static str>);

    impl TokenStore for Fixed {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            Ok(self.0.map(str::to_owned))
        }
        fn set(&self, _: &str) -> Result<(), TokenStoreError> {
            Ok(())
        }
        fn clear(&self) -> Result<(), TokenStoreError> {
            Ok(())
        }
    }

    // The handshake callback type fixes the large Err variant.
    #[allow(clippy::result_large_err)]
    #[tokio::test]
    async fn link_sends_the_token_in_the_header_and_moves_frames_both_ways() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!(
            "ws://{}/api/v1/devices/ws?deviceId=x",
            listener.local_addr().unwrap()
        );
        let seen = Arc::new(Mutex::new((String::new(), String::new())));
        let server_seen = seen.clone();
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_hdr_async(
                tcp,
                |req: &Request, res: Response| -> Result<Response, ErrorResponse> {
                    let auth = req
                        .headers()
                        .get("authorization")
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned();
                    *server_seen.lock().unwrap() = (auth, req.uri().to_string());
                    Ok(res)
                },
            )
            .await
            .unwrap();
            ws.send(Message::text(r#"{"type":"devices","devices":[]}"#))
                .await
                .unwrap();
            let Some(Ok(Message::Text(got))) = ws.next().await else {
                panic!("no frame")
            };
            got.as_str().to_owned()
        });
        let (tx, rx) = mpsc::channel(4);
        let events = Arc::new(Mutex::new(Vec::new()));
        let (e_in, e_st) = (events.clone(), events.clone());
        let link = tokio::spawn(async move {
            run(
                url,
                &Fixed(Some("tok")),
                rx,
                move |t| e_in.lock().unwrap().push(t),
                move |s| e_st.lock().unwrap().push(format!("{s:?}")),
            )
            .await;
        });
        tx.send(r#"{"type":"state"}"#.to_owned()).await.unwrap();
        assert_eq!(server.await.unwrap(), r#"{"type":"state"}"#);
        drop(tx);
        link.await.unwrap();
        assert_eq!(seen.lock().unwrap().0, "Bearer tok");
        let events = events.lock().unwrap();
        assert!(events.iter().any(|e| e.contains("devices")), "{events:?}");
        assert!(events.contains(&"Online".to_owned()), "{events:?}");
    }

    #[tokio::test]
    async fn no_token_ends_the_link_as_signed_out_without_connecting() {
        let (_tx, rx) = mpsc::channel(1);
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        run(
            "ws://127.0.0.1:1/x".into(),
            &Fixed(None),
            rx,
            |_| {},
            move |st| s.lock().unwrap().push(st),
        )
        .await;
        assert_eq!(*seen.lock().unwrap(), vec![LinkStatus::SignedOut]);
    }
}
