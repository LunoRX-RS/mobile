//! iOS Keychain with an explicit protection class. The `keyring` crate stores iOS items with no accessibility attribute,
//! which the system treats as "when unlocked", and those items travel with encrypted device backups to a new phone.
//! The YouTube session cookie must stay on this device and stay readable while the phone is locked (background playback
//! resolves the next track), so items use `AfterFirstUnlockThisDeviceOnly`. The module mirrors the slice of the `keyring`
//! API the stores use, so they compile unchanged.
use security_framework::{
    access_control::{ProtectionMode, SecAccessControl},
    passwords::{
        PasswordOptions, delete_generic_password, generic_password, set_generic_password_options,
    },
};
use std::fmt;

const ERR_ITEM_NOT_FOUND: i32 = -25300;
const ERR_INTERACTION_NOT_ALLOWED: i32 = -25308;

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
        f.write_str("ios keychain")
    }
}
impl std::error::Error for Error {}

fn map(err: security_framework::base::Error) -> Error {
    match err.code() {
        ERR_ITEM_NOT_FOUND => Error::NoEntry,
        // Device locked before first unlock: reachable but refused.
        ERR_INTERACTION_NOT_ALLOWED => Error::NoStorageAccess(Box::new(err)),
        _ => Error::PlatformFailure(Box::new(err)),
    }
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

    pub fn get_password(&self) -> Result<String, Error> {
        let bytes = generic_password(PasswordOptions::new_generic_password(
            &self.service,
            &self.account,
        ))
        .map_err(map)?;
        String::from_utf8(bytes).map_err(|e| Error::Invalid("password".into(), e.to_string()))
    }

    pub fn set_password(&self, password: &str) -> Result<(), Error> {
        // Replace rather than update: an update keeps the protection class of an item an older build stored.
        match self.delete_credential() {
            Ok(()) | Err(Error::NoEntry) => {}
            Err(e) => return Err(e),
        }
        let mut options = PasswordOptions::new_generic_password(&self.service, &self.account);
        let control = SecAccessControl::create_with_protection(
            Some(ProtectionMode::AccessibleAfterFirstUnlockThisDeviceOnly),
            0,
        )
        .map_err(map)?;
        options.set_access_control(control);
        set_generic_password_options(password.as_bytes(), options).map_err(map)
    }

    pub fn delete_credential(&self) -> Result<(), Error> {
        delete_generic_password(&self.service, &self.account).map_err(map)
    }
}
