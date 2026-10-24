//! The app's one handle on `innerlight`: the HTTP client, the cached player script and the helpers every call site
//! shares. Nothing here talks to a provider other than through `innerlight`.
use innerlight::{
    Client, Error,
    account::Account,
    decipher::{ScriptCache, ScriptDecipher},
    music::Music,
    youtube::Youtube,
};
use std::{future::Future, sync::Arc, time::Duration};

/// Cheap to clone; every clone shares the session cookie, the visitor data and the script cache.
#[derive(Clone)]
pub struct Yt {
    client: Client,
    scripts: Arc<ScriptCache>,
    /// One script download and analysis at a time; the others wait and then hit the cache.
    gate: Arc<tokio::sync::Mutex<()>>,
}

/// Why a cookie was not accepted as a signed-in session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionError {
    /// Not a usable cookie string, or the service refused it.
    Rejected,
    /// The check could not reach the service.
    Unreachable,
}

impl Yt {
    pub fn new(timeout: Duration) -> Result<Self, Error> {
        Ok(Self::from_client(
            // innerlight's default policy: a bounded retry for transient failures (429/5xx/timeout), never for a
            // sign-in wall, a refusal or a challenge.
            Client::builder().timeout(timeout).build()?,
        ))
    }

    /// A handle on an already built client. Tests pass one built on a fixture transport.
    pub fn from_client(client: Client) -> Self {
        Self {
            client,
            scripts: Arc::new(ScriptCache::new()),
            gate: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn client(&self) -> &Client {
        &self.client
    }
    pub fn music(&self) -> Music<'_> {
        Music::new(&self.client)
    }
    pub fn youtube(&self) -> Youtube<'_> {
        Youtube::new(&self.client)
    }
    pub fn account(&self) -> Account<'_> {
        Account::new(&self.client)
    }

    /// True while a cookie with a usable `SAPISID` is stored.
    pub fn signed_in(&self) -> bool {
        self.client.is_signed_in()
    }

    pub fn clear_session(&self) {
        self.client.clear_session();
    }

    /// Stores the cookie, then asks for the first page of the signed-in library once. The cookie is dropped again when
    /// the service refuses it or cannot be reached. A reply this build cannot read still counts as accepted: the
    /// service answered a signed request with JSON, and every later call reports its own failure.
    pub async fn connect(&self, cookie: &str) -> Result<(), SessionError> {
        self.client
            .set_cookie(cookie)
            .map_err(|_| SessionError::Rejected)?;
        if !self.signed_in() {
            self.client.clear_session();
            return Err(SessionError::Rejected);
        }
        let verdict = match self.music().saved_playlists().await {
            Ok(_) | Err(Error::Parse) => Ok(()),
            Err(Error::Network | Error::TooLarge | Error::RateLimited { .. }) => {
                Err(SessionError::Unreachable)
            }
            Err(_) => Err(SessionError::Rejected),
        };
        if verdict.is_err() {
            self.client.clear_session();
        }
        verdict
    }

    /// The deciphering for the current player script, or `None` when the script cannot be fetched or understood.
    /// Direct stream URLs still work without it; ciphered ones then fail with a typed error.
    pub async fn decipher(&self) -> Option<Arc<ScriptDecipher>> {
        let _one_at_a_time = self.gate.lock().await;
        self.scripts.get(&self.client).await.ok()
    }

    /// Call after a `DecipherFailed`: the next `decipher()` fetches and analyses the script again.
    pub fn forget_script(&self) {
        self.scripts.invalidate();
    }
}

/// Appends pages to `items` until `limit` items are held or the list ends. `more` turns a continuation into the next
/// `(items, continuation)`. Stops on a repeated token, so a service that loops cannot loop this.
pub async fn extend<T, F, Fut>(
    items: &mut Vec<T>,
    mut next: Option<String>,
    limit: usize,
    mut more: F,
) -> Result<(), Error>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<(Vec<T>, Option<String>), Error>>,
{
    let mut last = None;
    while items.len() < limit
        && let Some(token) = next.take()
    {
        if last.as_ref() == Some(&token) {
            break;
        }
        let (page, after) = more(token.clone()).await?;
        last = Some(token);
        if page.is_empty() {
            break;
        }
        items.extend(page);
        next = after;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn extend_follows_tokens_up_to_the_limit_and_survives_loops() {
        let mut items = vec![1];
        extend(&mut items, Some("a".into()), 4, |t| async move {
            Ok(match t.as_str() {
                "a" => (vec![2, 3], Some("b".into())),
                "b" => (vec![4, 5], Some("c".into())),
                _ => panic!("past the limit"),
            })
        })
        .await
        .unwrap();
        assert_eq!(items, [1, 2, 3, 4, 5]);

        let mut looping = Vec::new();
        let mut calls = 0;
        extend(&mut looping, Some("same".into()), 100, |_| {
            calls += 1;
            async { Ok((vec![0], Some("same".into()))) }
        })
        .await
        .unwrap();
        assert_eq!((looping.len(), calls), (1, 1));

        let mut failing = vec![9];
        let err = extend(&mut failing, Some("x".into()), 10, |_| async {
            Err::<(Vec<i32>, Option<String>), _>(Error::Network)
        })
        .await;
        assert_eq!(err, Err(Error::Network));
        assert_eq!(failing, [9]);
    }

    #[tokio::test]
    async fn a_cookie_without_sapisid_is_never_kept() {
        let yt = Yt::from_client(Client::builder().build().unwrap());
        assert_eq!(yt.connect("SID=only").await, Err(SessionError::Rejected));
        assert_eq!(
            yt.connect("bad\r\ncookie").await,
            Err(SessionError::Rejected)
        );
        assert!(!yt.signed_in());
    }

    fn with_answer(status: u16, body: &[u8]) -> Yt {
        let transport = innerlight::FixtureTransport::new().with_raw("browse", status, body);
        Yt::from_client(
            Client::builder()
                .transport(Arc::new(transport))
                .build()
                .unwrap(),
        )
    }

    #[tokio::test]
    async fn connect_keeps_the_session_only_when_the_service_does_not_refuse_it() {
        let accepted = with_answer(200, b"{}");
        assert_eq!(accepted.connect("SAPISID=abc; SID=x").await, Ok(()));
        assert!(accepted.signed_in());
        for status in [401, 403] {
            let refused = with_answer(status, b"{}");
            assert_eq!(
                refused.connect("SAPISID=abc; SID=x").await,
                Err(SessionError::Rejected)
            );
            assert!(!refused.signed_in(), "a refused cookie is dropped again");
        }
        let down = with_answer(503, b"{}");
        assert_eq!(
            down.connect("SAPISID=abc; SID=x").await,
            Err(SessionError::Unreachable)
        );
        assert!(!down.signed_in());
    }
}
