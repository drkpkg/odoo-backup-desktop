//! Encrypted local vault for Odoo Backup Desktop.
//!
//! The vault is a single file encrypted with XChaCha20-Poly1305 under a random
//! 32-byte data-encryption key (DEK). The DEK is never written in clear: it is
//! kept in the OS keychain and/or wrapped with a key derived from a master
//! password (Argon2id). See `docs/architecture.md` for the file format.
//!
//! All functions in this crate are blocking. Callers running inside an async
//! runtime must use `tokio::task::spawn_blocking` (keychain backends may block
//! on D-Bus and Argon2id takes hundreds of milliseconds by design).

mod crypto;
mod error;
mod format;
mod fsio;
mod keystore;
mod secret;

pub use error::{Result, VaultError};
pub use keystore::{KeyStore, MemoryKeyStore, OsKeyStore, init_os_keystore};
pub use secret::SecretField;

use std::path::{Path, PathBuf};

use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use zeroize::Zeroizing;

use crate::crypto::Key;
use crate::format::{FLAG_KEYCHAIN, FLAG_PASSWORD, HEADER_LEN, Header, KEY_LEN, NONCE_LEN, SALT_LEN, WRAPPED_DEK_LEN};

/// Argon2id cost parameters used to derive the password key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub parallelism: u32,
}

impl Default for KdfParams {
    /// RFC 9106 second recommended option: 64 MiB, 3 iterations.
    fn default() -> Self {
        Self { m_cost_kib: 64 * 1024, t_cost: 3, parallelism: 1 }
    }
}

impl KdfParams {
    /// Very cheap parameters, only for unit tests.
    #[doc(hidden)]
    pub fn insecure_for_tests() -> Self {
        Self { m_cost_kib: 64, t_cost: 1, parallelism: 1 }
    }
}

/// Public, non-secret description of a vault file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VaultInfo {
    /// The DEK is stored in the OS keychain.
    pub keychain_enabled: bool,
    /// The DEK is also wrapped with a master password.
    pub password_enabled: bool,
}

/// Options used when a vault is created. At least one unlock method is required.
#[derive(Debug)]
pub struct CreateOptions {
    pub use_keychain: bool,
    pub master_password: Option<SecretString>,
    pub kdf: KdfParams,
}

/// How to obtain the DEK when unlocking.
#[derive(Debug)]
pub enum UnlockMethod {
    Keychain,
    Password(SecretString),
}

/// Handle to the vault file on disk (locked state).
#[derive(Debug, Clone)]
pub struct VaultFile {
    path: PathBuf,
}

impl VaultFile {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn exists(&self) -> bool {
        self.path.is_file()
    }

    /// Reads the header only (no secrets needed).
    ///
    /// The header is authenticated only when the vault is unlocked, so this is a
    /// hint for the UI (which unlock methods to offer), not a security decision.
    pub fn info(&self) -> Result<VaultInfo> {
        let bytes = fsio::read_file(&self.path)?;
        let (header, _) = Header::decode(&bytes)?;
        Ok(header.info())
    }

    /// Creates a new vault holding `data`. Fails with `AlreadyExists` if the file exists.
    pub fn create<T: Serialize>(
        &self,
        keystore: &dyn KeyStore,
        options: CreateOptions,
        data: &T,
    ) -> Result<UnlockedVault> {
        if self.path.exists() {
            return Err(VaultError::AlreadyExists);
        }
        if !options.use_keychain && options.master_password.is_none() {
            return Err(VaultError::InvalidOptions("at least one unlock method is required".into()));
        }

        let dek = crypto::random_key()?;
        let mut header = Header {
            flags: 0,
            kdf: KdfParams { m_cost_kib: 0, t_cost: 0, parallelism: 0 },
            salt: [0; SALT_LEN],
            dek_nonce: [0; NONCE_LEN],
            wrapped_dek: [0; WRAPPED_DEK_LEN],
            payload_nonce: [0; NONCE_LEN],
        };
        if let Some(password) = &options.master_password {
            set_password_material(&mut header, &dek, password, options.kdf)?;
        }
        let plaintext = serialize(data)?;

        if options.use_keychain {
            keystore.store(dek.as_ref())?;
            header.flags |= FLAG_KEYCHAIN;
        }

        let mut vault = UnlockedVault { path: self.path.clone(), dek, header };
        if let Err(err) = vault.write(&plaintext) {
            if options.use_keychain
                && let Err(cleanup) = keystore.delete()
            {
                tracing::warn!(error = %cleanup, "could not remove vault key from keychain after a failed create");
            }
            return Err(err);
        }
        Ok(vault)
    }

    /// Unlocks the vault with the keychain or the master password.
    pub fn unlock(&self, keystore: &dyn KeyStore, method: UnlockMethod) -> Result<UnlockedVault> {
        let bytes = fsio::read_file(&self.path)?;
        let (header, ciphertext) = Header::decode(&bytes)?;
        let aad = &bytes[..HEADER_LEN];

        let dek = match method {
            UnlockMethod::Keychain => {
                if !header.keychain_enabled() {
                    return Err(VaultError::KeychainNotEnabled);
                }
                let stored = keystore.load()?.ok_or(VaultError::KeychainKeyMissing)?;
                if stored.len() != KEY_LEN {
                    return Err(VaultError::KeychainKeyMismatch);
                }
                let mut dek = Zeroizing::new([0u8; KEY_LEN]);
                dek.copy_from_slice(&stored);
                if decrypt_payload(&dek, &header, aad, ciphertext).is_none() {
                    return Err(VaultError::KeychainKeyMismatch);
                }
                dek
            }
            UnlockMethod::Password(password) => {
                if !header.password_enabled() {
                    return Err(VaultError::PasswordNotEnabled);
                }
                let password_key = crypto::derive_password_key(&password, &header.salt, &header.kdf)
                    .map_err(|_| VaultError::Corrupted("invalid key derivation parameters".into()))?;
                let dek = crypto::unwrap_dek(&password_key, &header.dek_nonce, &header.wrapped_dek)
                    .ok_or(VaultError::WrongPassword)?;
                if decrypt_payload(&dek, &header, aad, ciphertext).is_none() {
                    return Err(VaultError::Corrupted("authentication failed".into()));
                }
                dek
            }
        };

        Ok(UnlockedVault { path: self.path.clone(), dek, header })
    }
}

/// An unlocked vault: holds the DEK in memory (zeroized on drop).
pub struct UnlockedVault {
    path: PathBuf,
    dek: Key,
    header: Header,
}

impl std::fmt::Debug for UnlockedVault {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnlockedVault").finish_non_exhaustive()
    }
}

impl UnlockedVault {
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn info(&self) -> VaultInfo {
        self.header.info()
    }

    /// Decrypts and deserializes the payload currently on disk.
    pub fn load<T: DeserializeOwned>(&self) -> Result<T> {
        let plaintext = self.read_plaintext()?;
        deserialize(&plaintext)
    }

    /// Serializes, encrypts and atomically replaces the vault file.
    pub fn save<T: Serialize>(&mut self, data: &T) -> Result<()> {
        let plaintext = serialize(data)?;
        self.write(&plaintext)
    }

    /// Sets, replaces (`Some`) or removes (`None`) the master password.
    /// Removing is only allowed while the keychain is enabled.
    pub fn set_master_password(&mut self, new_password: Option<SecretString>, kdf: KdfParams) -> Result<()> {
        let plaintext = self.read_plaintext()?;
        let mut header = self.header;
        match new_password {
            Some(password) => set_password_material(&mut header, &self.dek, &password, kdf)?,
            None => {
                if !header.keychain_enabled() {
                    return Err(VaultError::InvalidOptions(
                        "cannot remove the master password while the keychain is disabled".into(),
                    ));
                }
                header.clear_password();
            }
        }
        self.write_with_header(header, &plaintext)
    }

    /// Enables (stores the DEK) or disables (deletes the DEK) the keychain.
    /// Disabling is only allowed while a master password is set.
    pub fn set_keychain_enabled(&mut self, keystore: &dyn KeyStore, enabled: bool) -> Result<()> {
        let mut header = self.header;
        if enabled {
            // Storing again also repairs a keychain entry deleted outside the app.
            keystore.store(self.dek.as_ref())?;
            if header.keychain_enabled() {
                return Ok(());
            }
            header.flags |= FLAG_KEYCHAIN;
        } else {
            if !header.password_enabled() {
                return Err(VaultError::InvalidOptions(
                    "cannot disable the keychain while no master password is set".into(),
                ));
            }
            if !header.keychain_enabled() {
                return Ok(());
            }
            // Delete first: if it fails nothing changes; if the rewrite then fails the
            // vault still opens with the master password.
            keystore.delete()?;
            header.flags &= !FLAG_KEYCHAIN;
        }
        let plaintext = self.read_plaintext()?;
        self.write_with_header(header, &plaintext)
    }

    fn read_plaintext(&self) -> Result<Zeroizing<Vec<u8>>> {
        let bytes = fsio::read_file(&self.path)?;
        let (header, ciphertext) = Header::decode(&bytes)?;
        decrypt_payload(&self.dek, &header, &bytes[..HEADER_LEN], ciphertext)
            .ok_or_else(|| VaultError::Corrupted("authentication failed".into()))
    }

    fn write(&mut self, plaintext: &[u8]) -> Result<()> {
        self.write_with_header(self.header, plaintext)
    }

    fn write_with_header(&mut self, mut header: Header, plaintext: &[u8]) -> Result<()> {
        header.payload_nonce = crypto::random_array()?;
        let header_bytes = header.encode();

        let mut buffer = Zeroizing::new(Vec::with_capacity(plaintext.len() + format::TAG_LEN));
        buffer.extend_from_slice(plaintext);
        crypto::seal(&self.dek, &header.payload_nonce, &header_bytes, &mut buffer)?;

        let mut file = Vec::with_capacity(HEADER_LEN + buffer.len());
        file.extend_from_slice(&header_bytes);
        file.extend_from_slice(&buffer);
        fsio::write_atomic(&self.path, &file)?;
        self.header = header;
        Ok(())
    }
}

fn set_password_material(header: &mut Header, dek: &Key, password: &SecretString, kdf: KdfParams) -> Result<()> {
    if password.expose_secret().is_empty() {
        return Err(VaultError::InvalidOptions("the master password must not be empty".into()));
    }
    crypto::argon2_params(&kdf)?;
    let salt = crypto::random_array::<SALT_LEN>()?;
    let dek_nonce = crypto::random_array::<NONCE_LEN>()?;
    let password_key = crypto::derive_password_key(password, &salt, &kdf)?;
    header.wrapped_dek = crypto::wrap_dek(&password_key, &dek_nonce, dek)?;
    header.kdf = kdf;
    header.salt = salt;
    header.dek_nonce = dek_nonce;
    header.flags |= FLAG_PASSWORD;
    Ok(())
}

fn decrypt_payload(dek: &Key, header: &Header, aad: &[u8], ciphertext: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
    let mut buffer = Zeroizing::new(ciphertext.to_vec());
    crypto::open(dek, &header.payload_nonce, aad, &mut buffer).then_some(buffer)
}

fn serialize<T: Serialize>(data: &T) -> Result<Zeroizing<Vec<u8>>> {
    serde_json::to_vec(data).map(Zeroizing::new).map_err(|e| VaultError::Serde(describe_serde_error(&e)))
}

fn deserialize<T: DeserializeOwned>(plaintext: &[u8]) -> Result<T> {
    serde_json::from_slice(plaintext).map_err(|e| VaultError::Serde(describe_serde_error(&e)))
}

/// serde_json messages can quote payload values (i.e. secrets); keep only the position.
fn describe_serde_error(err: &serde_json::Error) -> String {
    format!("{:?} error at line {} column {}", err.classify(), err.line(), err.column())
}

#[cfg(test)]
mod tests;
