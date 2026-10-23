//! App-account sign-in over the session token store.

use super::api::{ApiClient, SyncError};
use super::credentials::{TokenStore, TokenStoreError};

#[derive(Debug, thiserror::Error)]
pub enum AuthError {
    #[error(transparent)]
    Sync(#[from] SyncError),
    #[error(transparent)]
    Store(#[from] TokenStoreError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignedInUser {
    /// None after sign_in: the login response carries no user id.
    pub user_id: Option<String>,
    pub expires_at: String,
}

#[derive(Debug, PartialEq, Eq)]
pub enum AuthState {
    SignedOut,
    SignedIn(SignedInUser),
}

#[derive(Debug)]
pub struct SignOut {
    /// Set when the server-side logout failed; local state is signed out either way.
    pub warning: Option<SyncError>,
}

pub async fn sign_in(
    api: &ApiClient,
    store: &dyn TokenStore,
    email: &str,
    password: &str,
) -> Result<SignedInUser, AuthError> {
    let res = api.login(email, password).await?;
    store.set(&res.token)?;
    Ok(SignedInUser {
        user_id: None,
        expires_at: res.expires_at,
    })
}

pub async fn restore_session(
    api: &ApiClient,
    store: &dyn TokenStore,
) -> Result<AuthState, AuthError> {
    let Some(token) = store.get()? else {
        return Ok(AuthState::SignedOut);
    };
    match api.get_session(&token).await {
        Ok(s) => Ok(AuthState::SignedIn(SignedInUser {
            user_id: Some(s.user_id),
            expires_at: s.expires_at,
        })),
        // Only a server-rejected token is cleared; transient failures keep it for retry.
        Err(SyncError::Unauthenticated(_)) => {
            store.clear()?;
            Ok(AuthState::SignedOut)
        }
        Err(e) => Err(e.into()),
    }
}

pub async fn sign_out(api: &ApiClient, store: &dyn TokenStore) -> Result<SignOut, AuthError> {
    let Some(token) = store.get()? else {
        return Ok(SignOut { warning: None });
    };
    // Clear even when logout fails: the local state must be signed out regardless.
    let warning = api.logout(&token).await.err();
    store.clear()?;
    Ok(SignOut { warning })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{TcpListener, TcpStream};

    const SENTINEL: &str = "SENTINEL-AUTH-TOKEN-9b2c";
    const USER: &str = "6f1d2c3a-0000-4000-8000-0000000000aa";

    /// In-process store. Tests never touch the real user Keychain.
    #[derive(Default)]
    struct MemoryStore(Mutex<Option<String>>);

    impl MemoryStore {
        fn with(token: &str) -> Self {
            Self(Mutex::new(Some(token.to_string())))
        }

        fn slot(&self) -> Option<String> {
            self.0.lock().unwrap().clone()
        }
    }

    impl TokenStore for MemoryStore {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            Ok(self.slot())
        }

        fn set(&self, token: &str) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = Some(token.to_string());
            Ok(())
        }

        fn clear(&self) -> Result<(), TokenStoreError> {
            *self.0.lock().unwrap() = None;
            Ok(())
        }
    }

    /// (request line, Authorization header) for each request the stub received.
    type Req = (String, Option<String>);

    struct Stub {
        base: String,
        seen: Arc<Mutex<Vec<Req>>>,
    }

    impl Stub {
        fn count(&self) -> usize {
            self.seen.lock().unwrap().len()
        }

        fn last(&self) -> Req {
            self.seen
                .lock()
                .unwrap()
                .last()
                .cloned()
                .expect("no request recorded")
        }
    }

    /// Serves `response` (raw HTTP/1.1) to every connection on 127.0.0.1:0 and records each request.
    async fn stub(response: String) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                let raw = read_request(&mut sock).await;
                log.lock().unwrap().push(parse_request(&raw));
                let _ = sock.write_all(response.as_bytes()).await;
                let _ = sock.shutdown().await;
            }
        });
        Stub { base, seen }
    }

    async fn read_request(sock: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut chunk = [0u8; 1024];
        loop {
            let n = sock.read(&mut chunk).await.unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.extend_from_slice(&chunk[..n]);
            if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buf[..end]).to_lowercase();
                let len = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length:"))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if buf.len() >= end + 4 + len {
                    break;
                }
            }
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    fn parse_request(raw: &str) -> Req {
        let line = raw.lines().next().unwrap_or("").to_string();
        let auth = raw.lines().find_map(|l| {
            let (k, v) = l.split_once(':')?;
            k.trim()
                .eq_ignore_ascii_case("authorization")
                .then(|| v.trim().to_string())
        });
        (line, auth)
    }

    fn raw(status: &str, body: &str) -> String {
        format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    /// Base URL of a port nothing listens on: connections are refused.
    fn closed_base() -> String {
        let port = std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port();
        format!("http://127.0.0.1:{port}")
    }

    fn login_ok() -> String {
        raw(
            "200 OK",
            &format!(r#"{{"token":"{SENTINEL}","expiresAt":"2026-11-18T00:00:00Z"}}"#),
        )
    }

    fn session_ok() -> String {
        raw(
            "200 OK",
            &format!(r#"{{"userId":"{USER}","expiresAt":"2026-11-18T00:00:00Z"}}"#),
        )
    }

    const UNAUTHENTICATED: &str =
        r#"{"error":{"code":"UNAUTHENTICATED","message":"Authentication required."}}"#;

    #[tokio::test]
    async fn sign_in_success_stores_token() {
        let s = stub(login_ok()).await;
        let store = MemoryStore::default();
        let user = sign_in(
            &ApiClient::new(&s.base).unwrap(),
            &store,
            "a@example.test",
            "pw",
        )
        .await
        .unwrap();
        assert_eq!(store.slot(), Some(SENTINEL.to_string()));
        assert_eq!(user.user_id, None);
        assert_eq!(user.expires_at, "2026-11-18T00:00:00Z");
        assert_eq!(s.count(), 1);
        assert_eq!(s.last().0, "POST /api/v1/auth/login HTTP/1.1");
    }

    #[tokio::test]
    async fn sign_in_failure_stores_nothing() {
        let s = stub(raw("401 Unauthorized", UNAUTHENTICATED)).await;
        let store = MemoryStore::default();
        let err = sign_in(
            &ApiClient::new(&s.base).unwrap(),
            &store,
            "a@example.test",
            "bad",
        )
        .await
        .unwrap_err();
        assert!(
            matches!(err, AuthError::Sync(SyncError::Unauthenticated(_))),
            "{err:?}"
        );
        assert_eq!(store.slot(), None);
    }

    #[tokio::test]
    async fn restore_without_token_is_signed_out_with_no_requests() {
        let s = stub(session_ok()).await;
        let store = MemoryStore::default();
        let state = restore_session(&ApiClient::new(&s.base).unwrap(), &store)
            .await
            .unwrap();
        assert_eq!(state, AuthState::SignedOut);
        assert_eq!(s.count(), 0);
    }

    #[tokio::test]
    async fn restore_with_valid_token_is_signed_in() {
        let s = stub(session_ok()).await;
        let store = MemoryStore::with(SENTINEL);
        let state = restore_session(&ApiClient::new(&s.base).unwrap(), &store)
            .await
            .unwrap();
        assert_eq!(
            state,
            AuthState::SignedIn(SignedInUser {
                user_id: Some(USER.to_string()),
                expires_at: "2026-11-18T00:00:00Z".to_string(),
            })
        );
        assert_eq!(s.last().0, "GET /api/v1/auth/session HTTP/1.1");
        assert_eq!(s.last().1, Some(format!("Bearer {SENTINEL}")));
        assert_eq!(store.slot(), Some(SENTINEL.to_string()));
    }

    #[tokio::test]
    async fn restore_rejected_token_clears_it_and_is_signed_out() {
        let s = stub(raw("401 Unauthorized", UNAUTHENTICATED)).await;
        let store = MemoryStore::with(SENTINEL);
        let state = restore_session(&ApiClient::new(&s.base).unwrap(), &store)
            .await
            .unwrap();
        assert_eq!(state, AuthState::SignedOut);
        assert_eq!(store.slot(), None);
    }

    #[tokio::test]
    async fn restore_transient_failure_keeps_token() {
        let store = MemoryStore::with(SENTINEL);
        let err = restore_session(&ApiClient::new(&closed_base()).unwrap(), &store)
            .await
            .unwrap_err();
        assert!(
            matches!(err, AuthError::Sync(SyncError::Network)),
            "{err:?}"
        );
        assert_eq!(store.slot(), Some(SENTINEL.to_string()));
    }

    #[tokio::test]
    async fn sign_out_success_calls_logout_and_clears_token() {
        let s = stub(raw("200 OK", "")).await;
        let store = MemoryStore::with(SENTINEL);
        let out = sign_out(&ApiClient::new(&s.base).unwrap(), &store)
            .await
            .unwrap();
        assert!(out.warning.is_none(), "{out:?}");
        assert_eq!(s.count(), 1);
        assert_eq!(s.last().0, "POST /api/v1/auth/logout HTTP/1.1");
        assert_eq!(s.last().1, Some(format!("Bearer {SENTINEL}")));
        assert_eq!(store.slot(), None);
    }

    #[tokio::test]
    async fn sign_out_network_failure_still_clears_token_and_warns() {
        let store = MemoryStore::with(SENTINEL);
        let out = sign_out(&ApiClient::new(&closed_base()).unwrap(), &store)
            .await
            .unwrap();
        assert!(matches!(out.warning, Some(SyncError::Network)), "{out:?}");
        assert_eq!(store.slot(), None);
    }

    #[tokio::test]
    async fn sign_out_without_token_makes_no_requests() {
        let s = stub(raw("200 OK", "")).await;
        let store = MemoryStore::default();
        let out = sign_out(&ApiClient::new(&s.base).unwrap(), &store)
            .await
            .unwrap();
        assert!(out.warning.is_none(), "{out:?}");
        assert_eq!(s.count(), 0);
    }

    #[tokio::test]
    async fn sentinel_token_never_appears_in_debug_or_display() {
        let login = stub(login_ok()).await;
        let user = sign_in(
            &ApiClient::new(&login.base).unwrap(),
            &MemoryStore::default(),
            "a@example.test",
            "pw",
        )
        .await
        .unwrap();

        let session = stub(session_ok()).await;
        let state = restore_session(
            &ApiClient::new(&session.base).unwrap(),
            &MemoryStore::with(SENTINEL),
        )
        .await
        .unwrap();

        let logout = stub(raw("200 OK", "")).await;
        let out = sign_out(
            &ApiClient::new(&logout.base).unwrap(),
            &MemoryStore::with(SENTINEL),
        )
        .await
        .unwrap();

        let mut texts = vec![
            format!("{user:?}"),
            format!("{state:?}"),
            format!("{out:?}"),
        ];
        let err = restore_session(
            &ApiClient::new(&closed_base()).unwrap(),
            &MemoryStore::with(SENTINEL),
        )
        .await
        .unwrap_err();
        texts.push(format!("{err:?}"));
        texts.push(err.to_string());
        for text in texts {
            assert!(!text.contains(SENTINEL), "{text}");
        }
    }
}
