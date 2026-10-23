//! Session token storage in the OS credential store, behind a trait.

use std::fmt;

#[cfg(target_os = "android")]
use crate::platform::android_keyring as keyring;
#[cfg(target_os = "ios")]
use crate::platform::ios_keyring as keyring;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenStoreError {
    Unavailable,
    Access,
}

impl fmt::Display for TokenStoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Reason only: backend messages and the token are never copied into the error.
        let reason = match self {
            Self::Unavailable => "OS credential store is unavailable",
            Self::Access => "OS credential store access failed",
        };
        f.write_str(reason)
    }
}

impl std::error::Error for TokenStoreError {}

pub trait TokenStore: Send + Sync {
    fn get(&self) -> Result<Option<String>, TokenStoreError>;
    fn set(&self, token: &str) -> Result<(), TokenStoreError>;
    /// Idempotent: clearing an empty store succeeds.
    fn clear(&self) -> Result<(), TokenStoreError>;
}

const SERVICE: &str = "io.lunorx.desktop";
const ACCOUNT: &str = "backend-session";

/// macOS Keychain, Windows Credential Manager, Linux Secret Service (D-Bus; GNOME Keyring,
/// KWallet, ...). The keyring crate picks the backend per target OS; no plaintext fallback.
/// Linux needs libdbus-1 at build time. Without a running Secret Service, get/set/clear
/// return `Unavailable`.
pub struct KeyringTokenStore;

impl KeyringTokenStore {
    fn entry(&self) -> Result<keyring::Entry, TokenStoreError> {
        keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| TokenStoreError::Unavailable)
    }
}

/// Backend error to store category. Backend text is dropped on purpose.
/// PlatformFailure means the store could not be reached (no daemon, bus down, unknown
/// backend). NoStorageAccess means it is reachable but locked or refused.
fn map_backend_error(err: keyring::Error) -> TokenStoreError {
    match err {
        keyring::Error::PlatformFailure(_) => TokenStoreError::Unavailable,
        _ => TokenStoreError::Access,
    }
}

impl TokenStore for KeyringTokenStore {
    fn get(&self) -> Result<Option<String>, TokenStoreError> {
        match self.entry()?.get_password() {
            Ok(token) => Ok(Some(token)),
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    const SENTINEL: &str = "SENTINEL-TOKEN-7f3a9c";

    /// In-process store. Tests never touch the real user Keychain.
    #[derive(Default)]
    struct MemoryTokenStore {
        slot: Mutex<Option<String>>,
        fail: bool,
    }

    impl TokenStore for MemoryTokenStore {
        fn get(&self) -> Result<Option<String>, TokenStoreError> {
            if self.fail {
                return Err(TokenStoreError::Access);
            }
            Ok(self.slot.lock().unwrap().clone())
        }

        fn set(&self, token: &str) -> Result<(), TokenStoreError> {
            if self.fail {
                return Err(TokenStoreError::Access);
            }
            *self.slot.lock().unwrap() = Some(token.to_string());
            Ok(())
        }

        fn clear(&self) -> Result<(), TokenStoreError> {
            if self.fail {
                return Err(TokenStoreError::Access);
            }
            *self.slot.lock().unwrap() = None;
            Ok(())
        }
    }

    fn assert_no_token(err: &TokenStoreError) {
        let debug = format!("{err:?}");
        let display = err.to_string();
        assert!(!debug.contains(SENTINEL), "{debug}");
        assert!(!display.contains(SENTINEL), "{display}");
    }

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryTokenStore::default();
        assert_eq!(store.get(), Ok(None));
        store.set(SENTINEL).unwrap();
        assert_eq!(store.get(), Ok(Some(SENTINEL.to_string())));
        store.clear().unwrap();
        assert_eq!(store.get(), Ok(None));
    }

    #[test]
    fn clear_is_idempotent() {
        let store = MemoryTokenStore::default();
        assert_eq!(store.clear(), Ok(()));
        store.set(SENTINEL).unwrap();
        assert_eq!(store.clear(), Ok(()));
        assert_eq!(store.clear(), Ok(()));
        assert_eq!(store.get(), Ok(None));
    }

    #[test]
    fn error_text_never_contains_token() {
        let store = MemoryTokenStore {
            fail: true,
            ..Default::default()
        };
        assert_no_token(&store.set(SENTINEL).unwrap_err());
        assert_no_token(&store.get().unwrap_err());
        assert_no_token(&store.clear().unwrap_err());
        assert_no_token(&TokenStoreError::Unavailable);
        assert_no_token(&TokenStoreError::Access);
    }

    /// Runs without any keyring: the backend error is built by hand, nothing is queried.
    #[test]
    fn unreachable_backend_maps_to_unavailable() {
        let unreachable = keyring::Error::PlatformFailure(Box::new(std::io::Error::other(
            "no Secret Service on the bus",
        )));
        assert_eq!(map_backend_error(unreachable), TokenStoreError::Unavailable);
    }

    #[test]
    fn locked_or_refused_backend_maps_to_access() {
        let locked = keyring::Error::NoStorageAccess(Box::new(std::io::Error::other("locked")));
        assert_eq!(map_backend_error(locked), TokenStoreError::Access);
        let invalid = keyring::Error::Invalid("target".into(), "bad".into());
        assert_eq!(map_backend_error(invalid), TokenStoreError::Access);
    }
}
