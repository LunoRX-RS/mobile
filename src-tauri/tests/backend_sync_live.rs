//! Live integration: the desktop sync client against a running backend.
//! Runs only when LUNORX_LIVE_BACKEND_URL is set (for example http://127.0.0.1:18080); otherwise it returns early.
//! Synthetic accounts only. Tokens and passwords are never printed.

use std::sync::{Arc, Mutex};

use lunorx_lib::sync::api::{ApiClient, Entry, EntryAddRequest, Provider, SyncError};
use lunorx_lib::sync::credentials::{TokenStore, TokenStoreError};
use lunorx_lib::sync::engine::{ErrorCategory, SyncEngine, SyncState};
use lunorx_lib::sync::local_store::LocalEntry;
use lunorx_lib::sync::persist;
use lunorx_lib::sync::queue::WriteOp;

/// In-process token slot. The test keeps a handle so it can see what the engine stored.
#[derive(Clone, Default)]
struct Tokens(Arc<Mutex<Option<String>>>);

impl Tokens {
    fn slot(&self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }
}

impl TokenStore for Tokens {
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

fn engine(base: &str, tokens: &Tokens, path: &std::path::Path) -> SyncEngine {
    SyncEngine::new(
        ApiClient::new(base).unwrap(),
        Box::new(tokens.clone()),
        path.to_path_buf(),
    )
    .unwrap()
}

fn youtube(track: &str, title: &str) -> LocalEntry {
    LocalEntry {
        provider: Provider::YouTube,
        provider_track_id: track.into(),
        provider_url: format!("https://www.youtube.com/watch?v={track}"),
        title: Some(title.into()),
        artists: Some(vec!["Synthetic Band".into()]),
        artwork_url: None,
        duration_ms: Some(180_000),
        remote_entry_id: None,
    }
}

fn track_ids(entries: &[Entry]) -> Vec<String> {
    entries
        .iter()
        .map(|e| e.provider_track_id.clone())
        .collect()
}

/// Raw POST: ApiClient has no register method (reported as a gap).
async fn register(base: &str, email: &str, password: &str) -> u16 {
    reqwest::Client::new()
        .post(format!("{base}/api/v1/auth/register"))
        .json(&serde_json::json!({ "email": email, "password": password }))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16()
}

#[tokio::test]
async fn desktop_sync_client_against_live_backend() {
    let Ok(base) = std::env::var("LUNORX_LIVE_BACKEND_URL") else {
        eprintln!("SKIP: LUNORX_LIVE_BACKEND_URL is not set");
        return;
    };
    let base = base.trim_end_matches('/').to_string();
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let run = format!("{}-{nanos}", std::process::id());
    let dir = std::env::temp_dir().join(format!("lunorx-it10-{run}"));
    std::fs::create_dir_all(&dir).unwrap();
    let snap = dir.join("sync-snapshot.json");
    let email_a = format!("it10-{run}-a@example.test");
    let email_b = format!("it10-{run}-b@example.test");
    let password = format!("It10-{run}-Passw0rd!");
    let api = ApiClient::new(&base).unwrap();
    // Port nothing listens on: every request is refused, which is the offline path.
    let closed = format!(
        "http://127.0.0.1:{}",
        std::net::TcpListener::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap()
            .port()
    );

    // Register and sign in.
    assert_eq!(
        register(&base, &email_a, &password).await,
        201,
        "register A"
    );
    println!("PASS: register account A (raw POST, 201)");
    let tokens_a = Tokens::default();
    let mut e1 = engine(&base, &tokens_a, &snap);
    assert_eq!(e1.status().await.state, SyncState::SignedOut);
    println!("PASS: status is SignedOut before sign-in");
    e1.sign_in(&email_a, &password).await.expect("sign_in A");
    assert_eq!(e1.status().await.state, SyncState::Synced);
    println!("PASS: sign_in A stores the token and status is Synced");
    let token_a = tokens_a.slot().expect("token stored");
    let session = api.get_session(&token_a).await.expect("get_session");
    assert!(!session.user_id.is_empty());
    println!("PASS: get_session returns a user id");

    // Create a local playlist with two YouTube entries and upload it.
    let local = e1.create_local("Live Mix", Some("it10 live")).unwrap();
    e1.add_local_entry(&local, youtube("it10yt01", "Track One"))
        .unwrap();
    e1.add_local_entry(&local, youtube("it10yt02", "Track Two"))
        .unwrap();
    let remote = e1.upload(&local).await.expect("upload");
    let listed = api.list_playlists(&token_a).await.unwrap();
    assert!(
        listed
            .iter()
            .any(|p| p.id == remote && p.name == "Live Mix")
    );
    let entries = api.list_entries(&token_a, &remote).await.unwrap();
    assert_eq!(track_ids(&entries), ["it10yt01", "it10yt02"]);
    println!("PASS: upload creates the playlist with 2 YouTube entries on the server");

    // Offline path: an add to an uploaded playlist is queued locally (never sent inline), then flushed on reconnect.
    let mut e2 = engine(&closed, &tokens_a, &snap);
    e2.add_local_entry(&local, youtube("it10yt03", "Track Three"))
        .unwrap();
    let s2 = e2.status().await;
    assert_eq!((s2.state, s2.pending_writes), (SyncState::Offline, 1));
    println!("PASS: offline add to uploaded playlist is queued (pending 1); status Offline");
    let entries = api.list_entries(&token_a, &remote).await.unwrap();
    assert_eq!(track_ids(&entries), ["it10yt01", "it10yt02"]);
    println!("PASS: server still has 2 entries while offline");
    let mut e3 = engine(&base, &tokens_a, &snap);
    assert_eq!(e3.flush_pending().await, Ok(1));
    assert_eq!(e3.upload(&local).await, Ok(remote.clone()));
    let entries = api.list_entries(&token_a, &remote).await.unwrap();
    assert_eq!(track_ids(&entries), ["it10yt01", "it10yt02", "it10yt03"]);
    assert_eq!(e3.status().await.pending_writes, 0);
    println!(
        "PASS: flush_pending on reconnect sends the queued add once; server has 3 entries in order"
    );

    // Spotify with a title is refused locally and never reaches the server.
    let spotify_id = "4uLU6hMCjMI75M1A2tAKUQ";
    let spotify_titled = LocalEntry {
        provider: Provider::Spotify,
        provider_track_id: spotify_id.into(),
        provider_url: format!("https://open.spotify.com/track/{spotify_id}"),
        title: Some("Spotify Title".into()),
        artists: None,
        artwork_url: None,
        duration_ms: None,
        remote_entry_id: None,
    };
    assert_eq!(
        e3.add_local_entry(&local, spotify_titled),
        Err(ErrorCategory::Validation)
    );
    let direct = api
        .add_entry(
            &token_a,
            &remote,
            &EntryAddRequest {
                expected_version: 4,
                provider: Provider::Spotify,
                provider_track_id: spotify_id.into(),
                provider_url: format!("https://open.spotify.com/track/{spotify_id}"),
                title: Some("Spotify Title".into()),
                artists: None,
                artwork_url: None,
                duration_ms: None,
            },
        )
        .await;
    assert!(matches!(direct, Err(SyncError::Local(_))), "{direct:?}");
    let entries = api.list_entries(&token_a, &remote).await.unwrap();
    assert_eq!(entries.len(), 3);
    assert!(!track_ids(&entries).iter().any(|t| t == spotify_id));
    println!(
        "PASS: Spotify entry with a title refused by engine and ApiClient; server still has 3 entries"
    );

    // Owner-only: a second account cannot see the playlist.
    assert_eq!(
        register(&base, &email_b, &password).await,
        201,
        "register B"
    );
    let token_b = api.login(&email_b, &password).await.expect("login B").token;
    let get_b = api.get_playlist(&token_b, &remote).await;
    println!("INFO: non-owner get_playlist -> {get_b:?}");
    assert!(
        matches!(get_b, Err(SyncError::NotFound(_) | SyncError::Forbidden(_))),
        "{get_b:?}"
    );
    let entries_b = api.list_entries(&token_b, &remote).await;
    println!("INFO: non-owner list_entries -> {entries_b:?}");
    assert!(
        matches!(
            entries_b,
            Err(SyncError::NotFound(_) | SyncError::Forbidden(_))
        ),
        "{entries_b:?}"
    );
    assert!(
        !api.list_playlists(&token_b)
            .await
            .unwrap()
            .iter()
            .any(|p| p.id == remote)
    );
    api.get_playlist(&token_a, &remote)
        .await
        .expect("owner still sees it");
    println!("PASS: non-owner is refused and does not list the playlist; owner still sees it");

    // Sign-out refused while a write is pending. The pending write is seeded through the public persist API.
    let (store, mut queue) = persist::restore(persist::load(&snap).unwrap());
    queue
        .push(
            &remote,
            WriteOp::AddEntry(EntryAddRequest {
                expected_version: 4,
                provider: Provider::YouTube,
                provider_track_id: "it10yt04".into(),
                provider_url: "https://www.youtube.com/watch?v=it10yt04".into(),
                title: Some("Pending".into()),
                artists: None,
                artwork_url: None,
                duration_ms: None,
            }),
        )
        .unwrap();
    persist::save(&snap, &persist::capture(&store, &queue)).unwrap();
    let mut e4 = engine(&base, &tokens_a, &snap);
    assert_eq!(e4.sign_out().await, Err(ErrorCategory::Validation));
    assert_eq!(tokens_a.slot().as_deref(), Some(token_a.as_str()));
    api.get_session(&token_a)
        .await
        .expect("token still valid after refused sign-out");
    println!("PASS: sign_out refused with 1 pending write; token kept, session still valid");
    assert_eq!(e4.discard_pending(), Ok(1));
    assert_eq!(e4.sign_out().await, Ok(None));
    assert_eq!(tokens_a.slot(), None);
    assert!(
        persist::load(&snap).unwrap().playlists[0]
            .remote_id
            .is_none()
    );
    println!(
        "PASS: discard_pending drops 1 write; sign_out succeeds with no warning; remote bindings cleared"
    );

    // The old token is now rejected as unauthenticated.
    assert!(matches!(
        api.get_session(&token_a).await,
        Err(SyncError::Unauthenticated(_))
    ));
    assert!(matches!(
        api.list_playlists(&token_a).await,
        Err(SyncError::Unauthenticated(_))
    ));
    assert_eq!(e4.status().await.state, SyncState::SignedOut);
    println!("PASS: old token rejected as Unauthenticated; engine status is SignedOut");

    let _ = std::fs::remove_dir_all(&dir);
}
