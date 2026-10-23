//! Write-only provider credential commands (blueprint 17: provider credentials stay on this device in
//! OS-protected storage and are never sent to the backend). The value goes in; only `{ present }` comes out.

use serde::{Deserialize, Serialize};

#[cfg(target_os = "android")]
use crate::platform::android_keyring as keyring;
#[cfg(target_os = "ios")]
use crate::platform::ios_keyring as keyring;

use crate::commands::state::SearchState;
use crate::core::domain::ProviderId;
use crate::sync::credentials::{TokenStore, TokenStoreError};

const SERVICE: &str = "io.lunorx.desktop";
// Distinct from the sync session slot ("backend-session").
const YOUTUBE_ACCOUNT: &str = "provider-youtube-api-key";
const KEY_MIN: usize = 20;
const KEY_MAX: usize = 128;

/// OS keyring slot for one provider credential: same backends as the sync token store (macOS Keychain,
/// Windows Credential Manager, Linux Secret Service), no plaintext fallback.
pub struct KeyringKeyStore {
    account: &'static str,
}

impl KeyringKeyStore {
    pub fn youtube() -> Self {
        Self {
            account: YOUTUBE_ACCOUNT,
        }
    }

    pub fn youtube_session() -> Self {
        Self {
            account: "provider-youtube-session-cookie",
        }
    }

    fn entry(&self) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(SERVICE, self.account).map_err(|_| TokenStoreError::Unavailable)
    }
}

// Backend text is dropped on purpose. PlatformFailure = store unreachable, anything else = refused/locked.
fn map_backend_error(err: keyring::Error) -> TokenStoreError {
    match err {
        keyring::Error::PlatformFailure(_) => TokenStoreError::Unavailable,
        _ => TokenStoreError::Access,
    }
}

impl TokenStore for KeyringKeyStore {
    fn get(&self) -> Result<Option<String>, TokenStoreError> {
        match self.entry()?.get_password() {
            Ok(v) => Ok(Some(v)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(map_backend_error(e)),
        }
    }

    fn set(&self, token: &str) -> Result<(), TokenStoreError> {
        self.entry()?.set_password(token).map_err(map_backend_error)
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(map_backend_error(e)),
        }
    }
}

/// Test double for the keyring; tests never touch the real Keychain.
#[cfg(test)]
#[derive(Default)]
pub(crate) struct MemoryKeyStore {
    slot: std::sync::Mutex<Option<String>>,
    pub(crate) fail: std::sync::atomic::AtomicBool,
}

#[cfg(test)]
impl MemoryKeyStore {
    fn failing(&self) -> Result<(), TokenStoreError> {
        if self.fail.load(std::sync::atomic::Ordering::SeqCst) {
            return Err(TokenStoreError::Access);
        }
        Ok(())
    }
}

#[cfg(test)]
impl TokenStore for MemoryKeyStore {
    fn get(&self) -> Result<Option<String>, TokenStoreError> {
        self.failing()?;
        Ok(self.slot.lock().unwrap().clone())
    }

    fn set(&self, token: &str) -> Result<(), TokenStoreError> {
        self.failing()?;
        *self.slot.lock().unwrap() = Some(token.to_owned());
        Ok(())
    }

    fn clear(&self) -> Result<(), TokenStoreError> {
        self.failing()?;
        *self.slot.lock().unwrap() = None;
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct CredentialSetArgs {
    pub provider: String,
    pub kind: String,
    pub value: String,
}

// Manual Debug: the value must never reach a log through `{:?}`.
impl std::fmt::Debug for CredentialSetArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CredentialSetArgs")
            .field("provider", &self.provider)
            .field("kind", &self.kind)
            .field("value", &"<redacted>")
            .finish()
    }
}

#[derive(Debug, Deserialize)]
pub struct CredentialProviderArgs {
    pub provider: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub struct CredentialStatusDto {
    pub present: bool,
}

/// Fixed categories only. No variant carries text, so no secret or backend message can ride along.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "category", rename_all = "camelCase")]
pub enum CredentialError {
    UnsupportedProvider,
    UnsupportedKind,
    InvalidValue,
    StoreUnavailable,
    StoreAccess,
}

impl From<TokenStoreError> for CredentialError {
    fn from(e: TokenStoreError) -> Self {
        match e {
            TokenStoreError::Unavailable => Self::StoreUnavailable,
            TokenStoreError::Access => Self::StoreAccess,
        }
    }
}

// Only YouTube takes a credential today, and only an "apiKey".
fn require_youtube(provider: &str) -> Result<(), CredentialError> {
    match provider.parse::<ProviderId>() {
        Ok(ProviderId::YouTube) => Ok(()),
        _ => Err(CredentialError::UnsupportedProvider),
    }
}

fn valid_api_key(raw: &str) -> Result<&str, CredentialError> {
    let key = raw.trim();
    let ok = (KEY_MIN..=KEY_MAX).contains(&key.len())
        && key
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if ok {
        Ok(key)
    } else {
        Err(CredentialError::InvalidValue)
    }
}

pub fn credential_set(
    state: &SearchState,
    args: &CredentialSetArgs,
) -> Result<(), CredentialError> {
    require_youtube(&args.provider)?;
    if args.kind != "apiKey" {
        return Err(CredentialError::UnsupportedKind);
    }
    let key = valid_api_key(&args.value)?;
    state.youtube_key().set(key)?;
    Ok(())
}

pub fn credential_status(
    state: &SearchState,
    provider: &str,
) -> Result<CredentialStatusDto, CredentialError> {
    require_youtube(provider)?;
    let present = state.youtube_key().get()?.is_some_and(|k| !k.is_empty());
    Ok(CredentialStatusDto { present })
}

pub fn credential_clear(state: &SearchState, provider: &str) -> Result<(), CredentialError> {
    require_youtube(provider)?;
    state.youtube_key().clear()?;
    Ok(())
}

// Async so the blocking keyring call runs on the async runtime, not the Tauri main thread.
#[tauri::command]
pub async fn provider_credential_set(
    state: tauri::State<'_, SearchState>,
    args: CredentialSetArgs,
) -> Result<(), CredentialError> {
    credential_set(&state, &args)
}

#[tauri::command]
pub async fn provider_credential_status(
    state: tauri::State<'_, SearchState>,
    args: CredentialProviderArgs,
) -> Result<CredentialStatusDto, CredentialError> {
    credential_status(&state, &args.provider)
}

#[tauri::command]
pub async fn provider_credential_clear(
    state: tauri::State<'_, SearchState>,
    args: CredentialProviderArgs,
) -> Result<(), CredentialError> {
    credential_clear(&state, &args.provider)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::Ordering;

    const KEY: &str = "SENTINEL-KEY_abcdefghij0123456789";

    fn state() -> (SearchState, Arc<MemoryKeyStore>) {
        let store = Arc::new(MemoryKeyStore::default());
        (SearchState::with_key_store(store.clone()), store)
    }

    fn set_args(provider: &str, kind: &str, value: &str) -> CredentialSetArgs {
        CredentialSetArgs {
            provider: provider.into(),
            kind: kind.into(),
            value: value.into(),
        }
    }

    #[test]
    fn set_status_clear_round_trip_and_status_hides_the_value() {
        let (s, store) = state();
        assert!(!credential_status(&s, "YouTube").unwrap().present);
        credential_set(&s, &set_args("YouTube", "apiKey", &format!("  {KEY}\n"))).unwrap();
        // Trimmed before storing.
        assert_eq!(store.get().unwrap().as_deref(), Some(KEY));
        let status = credential_status(&s, "YouTube").unwrap();
        assert_eq!(status, CredentialStatusDto { present: true });
        let json = serde_json::to_string(&status).unwrap();
        assert_eq!(json, r#"{"present":true}"#);
        credential_clear(&s, "YouTube").unwrap();
        credential_clear(&s, "YouTube").unwrap(); // idempotent
        assert!(!credential_status(&s, "YouTube").unwrap().present);
    }

    #[test]
    fn length_and_charset_are_enforced() {
        let (s, store) = state();
        let bad = [
            "a".repeat(19),
            "a".repeat(129),
            format!("{}!", "a".repeat(25)),
            format!("{} {}", "a".repeat(15), "b".repeat(15)),
            format!("{}é", "a".repeat(25)),
            String::new(),
            " ".repeat(30),
        ];
        for v in bad {
            assert_eq!(
                credential_set(&s, &set_args("YouTube", "apiKey", &v)),
                Err(CredentialError::InvalidValue),
                "{v:?}"
            );
        }
        assert_eq!(store.get(), Ok(None));
        for v in ["a".repeat(20), "A-_9".repeat(32)] {
            credential_set(&s, &set_args("YouTube", "apiKey", &v)).unwrap();
        }
    }

    #[test]
    fn unsupported_provider_and_kind_are_rejected_before_storing() {
        let (s, store) = state();
        for p in ["Spotify", "Deezer", "", "youtube"] {
            assert_eq!(
                credential_set(&s, &set_args(p, "apiKey", KEY)),
                Err(CredentialError::UnsupportedProvider),
                "{p}"
            );
            assert_eq!(
                credential_status(&s, p),
                Err(CredentialError::UnsupportedProvider)
            );
            assert_eq!(
                credential_clear(&s, p),
                Err(CredentialError::UnsupportedProvider)
            );
        }
        assert_eq!(
            credential_set(&s, &set_args("YouTube", "oauth", KEY)),
            Err(CredentialError::UnsupportedKind)
        );
        assert_eq!(store.get(), Ok(None));
    }

    #[test]
    fn store_failures_map_to_fixed_categories_and_never_contain_the_key() {
        let (s, store) = state();
        store.fail.store(true, Ordering::SeqCst);
        let args = set_args("YouTube", "apiKey", KEY);
        let errs = [
            credential_set(&s, &args).unwrap_err(),
            credential_status(&s, "YouTube").unwrap_err(),
            credential_clear(&s, "YouTube").unwrap_err(),
            credential_set(&s, &set_args("YouTube", "apiKey", "short")).unwrap_err(),
            credential_set(&s, &set_args("Nope", "apiKey", KEY)).unwrap_err(),
            CredentialError::from(TokenStoreError::Unavailable),
        ];
        for e in errs {
            let text = format!("{e:?}");
            let json = serde_json::to_string(&e).unwrap();
            assert!(
                !text.contains("SENTINEL") && !json.contains("SENTINEL"),
                "{text} {json}"
            );
        }
        assert_eq!(credential_set(&s, &args), Err(CredentialError::StoreAccess));
        assert_eq!(
            serde_json::to_string(&CredentialError::StoreAccess).unwrap(),
            r#"{"category":"storeAccess"}"#
        );
    }

    #[test]
    fn args_debug_redacts_the_value() {
        let a = set_args("YouTube", "apiKey", KEY);
        let text = format!("{a:?}");
        assert!(!text.contains(KEY) && text.contains("redacted"), "{text}");
    }

    #[test]
    fn args_deserialize_camel_case_payload() {
        let a: CredentialSetArgs = serde_json::from_value(serde_json::json!({
            "provider": "YouTube", "kind": "apiKey", "value": KEY
        }))
        .unwrap();
        assert_eq!(a.kind, "apiKey");
    }

    #[test]
    fn keyring_slot_is_not_the_sync_session_slot() {
        assert_ne!(YOUTUBE_ACCOUNT, "backend-session");
        let session = KeyringKeyStore::youtube_session();
        assert_ne!(session.account, YOUTUBE_ACCOUNT);
        assert_ne!(session.account, "backend-session");
    }
}
