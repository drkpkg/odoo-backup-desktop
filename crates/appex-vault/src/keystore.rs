use std::sync::{Arc, Mutex};

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use keyring_core::{Entry, Error as KeyringError};
use zeroize::Zeroizing;

use crate::{Result, VaultError};

/// Storage for the vault DEK outside the vault file. Methods are blocking.
///
/// Never call an [`OsKeyStore`] from an async task directly: the Linux backend blocks
/// on D-Bus (async-io executor). Use `tokio::task::spawn_blocking`.
pub trait KeyStore: Send + Sync {
    /// Whether the backend can be used right now (e.g. Secret Service is running).
    fn is_available(&self) -> bool;
    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>>;
    fn store(&self, secret: &[u8]) -> Result<()>;
    fn delete(&self) -> Result<()>;
}

/// Serializes store initialization so concurrent callers do not connect twice.
static INIT_LOCK: Mutex<()> = Mutex::new(());

/// Registers the platform keyring-core default store (Secret Service on Linux,
/// Credential Manager on Windows, Keychain on macOS). Call once at startup.
///
/// Returns `KeychainUnavailable` when the platform store cannot be reached (for
/// example no `org.freedesktop.secrets` provider on the session bus). Calling it
/// again after a failure retries the connection.
pub fn init_os_keystore() -> Result<()> {
    let _guard = INIT_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if keyring_core::get_default_store().is_some() {
        return Ok(());
    }
    let store = platform_store()?;
    keyring_core::set_default_store(store);
    Ok(())
}

#[cfg(target_os = "linux")]
fn platform_store() -> Result<Arc<keyring_core::CredentialStore>> {
    let store = zbus_secret_service_keyring_store::Store::new().map_err(unavailable)?;
    Ok(store)
}

#[cfg(target_os = "windows")]
fn platform_store() -> Result<Arc<keyring_core::CredentialStore>> {
    let store = windows_native_keyring_store::Store::new().map_err(unavailable)?;
    Ok(store)
}

#[cfg(target_os = "macos")]
fn platform_store() -> Result<Arc<keyring_core::CredentialStore>> {
    let store = apple_native_keyring_store::keychain::Store::new().map_err(unavailable)?;
    Ok(store)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn platform_store() -> Result<Arc<keyring_core::CredentialStore>> {
    Err(VaultError::KeychainUnavailable)
}

fn unavailable(err: KeyringError) -> VaultError {
    tracing::debug!(error = %err, "OS keychain unavailable");
    VaultError::KeychainUnavailable
}

fn map_keyring_error(err: KeyringError) -> VaultError {
    match err {
        KeyringError::NoStorageAccess(_) | KeyringError::NoDefaultStore => unavailable(err),
        other => VaultError::Keyring(other.to_string()),
    }
}

/// OS keychain entry identified by service + account.
///
/// The secret is stored base64-encoded (KDE Wallet only accepts UTF-8). On Windows
/// the credential uses `Local` persistence so the key does not roam with the profile.
#[derive(Debug, Clone)]
pub struct OsKeyStore {
    pub service: String,
    pub account: String,
}

impl OsKeyStore {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self { service: service.into(), account: account.into() }
    }

    fn entry(&self) -> Result<Entry> {
        init_os_keystore()?;
        #[cfg(target_os = "windows")]
        let entry = {
            let modifiers = std::collections::HashMap::from([("persistence", "Local")]);
            Entry::new_with_modifiers(&self.service, &self.account, &modifiers)
        };
        #[cfg(not(target_os = "windows"))]
        let entry = Entry::new(&self.service, &self.account);
        entry.map_err(map_keyring_error)
    }
}

impl KeyStore for OsKeyStore {
    /// Checks that a platform store is registered (connecting if needed) and that an
    /// entry can be built. It does not read the secret, so it never triggers an
    /// unlock prompt; a locked collection surfaces later as `KeychainUnavailable`.
    fn is_available(&self) -> bool {
        self.entry().is_ok()
    }

    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
        let encoded = match self.entry()?.get_password() {
            Ok(value) => Zeroizing::new(value),
            Err(KeyringError::NoEntry) => return Ok(None),
            Err(KeyringError::BadEncoding(bytes)) => {
                drop(Zeroizing::new(bytes));
                return Err(VaultError::KeychainKeyMismatch);
            }
            Err(err) => return Err(map_keyring_error(err)),
        };
        let decoded = BASE64.decode(encoded.trim().as_bytes()).map_err(|_| VaultError::KeychainKeyMismatch)?;
        Ok(Some(Zeroizing::new(decoded)))
    }

    fn store(&self, secret: &[u8]) -> Result<()> {
        let encoded = Zeroizing::new(BASE64.encode(secret));
        self.entry()?.set_password(&encoded).map_err(map_keyring_error)
    }

    fn delete(&self) -> Result<()> {
        match self.entry()?.delete_credential() {
            Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
            Err(err) => Err(map_keyring_error(err)),
        }
    }
}

/// In-memory keystore for tests and for environments without a keychain.
#[derive(Default)]
pub struct MemoryKeyStore {
    available: bool,
    value: Mutex<Option<Zeroizing<Vec<u8>>>>,
}

impl MemoryKeyStore {
    pub fn new(available: bool) -> Self {
        Self { available, value: Mutex::new(None) }
    }
}

impl KeyStore for MemoryKeyStore {
    fn is_available(&self) -> bool {
        self.available
    }

    fn load(&self) -> Result<Option<Zeroizing<Vec<u8>>>> {
        if !self.available {
            return Err(VaultError::KeychainUnavailable);
        }
        Ok(self.value.lock().expect("keystore poisoned").clone())
    }

    fn store(&self, secret: &[u8]) -> Result<()> {
        if !self.available {
            return Err(VaultError::KeychainUnavailable);
        }
        *self.value.lock().expect("keystore poisoned") = Some(Zeroizing::new(secret.to_vec()));
        Ok(())
    }

    fn delete(&self) -> Result<()> {
        if !self.available {
            return Err(VaultError::KeychainUnavailable);
        }
        *self.value.lock().expect("keystore poisoned") = None;
        Ok(())
    }
}
