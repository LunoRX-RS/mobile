//! Android has no `keyring` backend. This mirrors the slice of the `keyring` API the app uses, so the stores compile
//! unchanged, and routes it to the Android Keystore through the lunor-native plugin (AES-GCM key held in the Keystore,
//! ciphertext in app-private storage). Before `init` runs, or when the plugin call fails, the store is unreachable:
//! callers map that to `TokenStoreError::Unavailable`. There is no plaintext fallback.
use serde::{Deserialize, Serialize};
use std::{fmt, sync::OnceLock};
use tauri::{AppHandle, Wry};
use tauri_plugin_lunor_native::LunorNativeExt;

static APP: OnceLock<AppHandle<Wry>> = OnceLock::new();

/// Call once from `setup`, after the plugin is registered.
pub fn init(app: &AppHandle<Wry>) {
    let _ = APP.set(app.clone());
}

#[derive(Debug)]
#[allow(dead_code)] // mirrors the keyring::Error variants the shared stores match on
pub enum Error {
    NoEntry,
    PlatformFailure(Box<dyn std::error::Error + Send + Sync>),
    NoStorageAccess(Box<dyn std::error::Error + Send + Sync>),
    Invalid(String, String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("android credential store")
    }
}
impl std::error::Error for Error {}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Slot<'a> {
    service: &'a str,
    account: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    value: Option<&'a str>,
}

#[derive(Deserialize)]
struct Value {
    value: Option<String>,
}

pub struct Entry {
    service: String,
    account: String,
}

impl Entry {
    pub fn new(service: &str, account: &str) -> Result<Self, Error> {
        Ok(Self {
            service: service.to_owned(),
            account: account.to_owned(),
        })
    }

    fn call<T: serde::de::DeserializeOwned>(
        &self,
        command: &str,
        value: Option<&str>,
    ) -> Result<T, Error> {
        let unreachable = || {
            Error::PlatformFailure(Box::new(std::io::Error::other(
                "Keystore bridge unavailable",
            )))
        };
        let app = APP.get().ok_or_else(unreachable)?;
        app.lunor_native()
            .call(
                command,
                Slot {
                    service: &self.service,
                    account: &self.account,
                    value,
                },
            )
            .map_err(|_| unreachable())
    }

    pub fn get_password(&self) -> Result<String, Error> {
        self.call::<Value>("secretGet", None)?
            .value
            .ok_or(Error::NoEntry)
    }

    pub fn set_password(&self, password: &str) -> Result<(), Error> {
        self.call::<serde_json::Value>("secretSet", Some(password))
            .map(|_| ())
    }

    pub fn delete_credential(&self) -> Result<(), Error> {
        self.call::<serde_json::Value>("secretDelete", None)
            .map(|_| ())
    }
}
