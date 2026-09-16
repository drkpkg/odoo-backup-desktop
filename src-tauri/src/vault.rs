//! Vault lifecycle for the app: create, unlock, lock, read and mutate `VaultData`.
//!
//! Decrypted data stays in Rust memory while unlocked. Every blocking call (keychain,
//! Argon2id, file I/O) runs on `spawn_blocking`.

use std::sync::{Arc, Mutex};
use std::time::Instant;

use appex_vault::{CreateOptions, KdfParams, KeyStore, UnlockMethod, UnlockedVault, VaultFile, VaultInfo};
use secrecy::SecretString;

use crate::error::{CommandError, CommandResult};
use crate::models::{VaultData, VaultStatus};

struct Unlocked {
    vault: UnlockedVault,
    data: VaultData,
}

#[derive(Clone)]
pub struct VaultManager {
    file: VaultFile,
    keystore: Arc<dyn KeyStore>,
    state: Arc<Mutex<Option<Unlocked>>>,
    last_activity: Arc<Mutex<Instant>>,
    kdf: KdfParams,
}

impl VaultManager {
    pub fn new(file: VaultFile, keystore: Arc<dyn KeyStore>) -> Self {
        Self::with_kdf(file, keystore, KdfParams::default())
    }

    pub fn with_kdf(file: VaultFile, keystore: Arc<dyn KeyStore>, kdf: KdfParams) -> Self {
        Self {
            file,
            keystore,
            state: Arc::new(Mutex::new(None)),
            last_activity: Arc::new(Mutex::new(Instant::now())),
            kdf,
        }
    }

    async fn blocking<T, F>(&self, f: F) -> CommandResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&VaultManager) -> CommandResult<T> + Send + 'static,
    {
        let this = self.clone();
        tokio::task::spawn_blocking(move || f(&this)).await?
    }

    fn lock_state(&self) -> CommandResult<std::sync::MutexGuard<'_, Option<Unlocked>>> {
        self.state.lock().map_err(|_| CommandError::internal("vault state lock poisoned"))
    }

    pub fn touch(&self) {
        if let Ok(mut last) = self.last_activity.lock() {
            *last = Instant::now();
        }
    }

    pub fn idle_for(&self) -> std::time::Duration {
        self.last_activity.lock().map(|last| last.elapsed()).unwrap_or_default()
    }

    pub fn is_unlocked(&self) -> bool {
        self.state.lock().map(|s| s.is_some()).unwrap_or(false)
    }

    pub async fn status(&self) -> CommandResult<VaultStatus> {
        self.blocking(|this| {
            let keychain_available = this.keystore.is_available();
            let unlocked_info = this.lock_state()?.as_ref().map(|u| u.vault.info());
            let info = match unlocked_info {
                Some(info) => Some(info),
                None if this.file.exists() => Some(this.file.info()?),
                None => None,
            };
            Ok(status_from(this.file.exists(), unlocked_info.is_some(), keychain_available, info))
        })
        .await
    }

    pub async fn create(&self, use_keychain: bool, master_password: Option<SecretString>) -> CommandResult<()> {
        self.blocking(move |this| {
            if use_keychain && !this.keystore.is_available() {
                return Err(appex_vault::VaultError::KeychainUnavailable.into());
            }
            let data = VaultData::default();
            let options = CreateOptions { use_keychain, master_password, kdf: this.kdf };
            let vault = this.file.create(this.keystore.as_ref(), options, &data)?;
            *this.lock_state()? = Some(Unlocked { vault, data });
            this.touch();
            Ok(())
        })
        .await
    }

    /// `None` → keychain; `Some(password)` → master password.
    pub async fn unlock(&self, master_password: Option<SecretString>) -> CommandResult<()> {
        self.blocking(move |this| {
            if this.lock_state()?.is_some() {
                return Ok(());
            }
            let method = match master_password {
                Some(password) => UnlockMethod::Password(password),
                None => UnlockMethod::Keychain,
            };
            let vault = this.file.unlock(this.keystore.as_ref(), method)?;
            let mut data: VaultData = vault.load()?;
            migrate(&mut data);
            *this.lock_state()? = Some(Unlocked { vault, data });
            this.touch();
            Ok(())
        })
        .await
    }

    pub fn lock(&self) -> bool {
        match self.state.lock() {
            Ok(mut state) => state.take().is_some(),
            Err(_) => false,
        }
    }

    /// Runs `f` on a clone-free borrow of the decrypted data.
    pub fn read<T>(&self, f: impl FnOnce(&VaultData) -> T) -> CommandResult<T> {
        let state = self.lock_state()?;
        let unlocked = state.as_ref().ok_or_else(CommandError::vault_locked)?;
        Ok(f(&unlocked.data))
    }

    /// Applies `f` to a copy of the data, persists it and swaps it in on success.
    pub async fn update<T, F>(&self, f: F) -> CommandResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut VaultData) -> CommandResult<T> + Send + 'static,
    {
        self.blocking(move |this| {
            let mut state = this.lock_state()?;
            let unlocked = state.as_mut().ok_or_else(CommandError::vault_locked)?;
            let mut data = unlocked.data.clone();
            let result = f(&mut data)?;
            unlocked.vault.save(&data)?;
            unlocked.data = data;
            this.touch();
            Ok(result)
        })
        .await
    }

    pub async fn set_master_password(&self, new_password: Option<SecretString>) -> CommandResult<()> {
        self.blocking(move |this| {
            let mut state = this.lock_state()?;
            let unlocked = state.as_mut().ok_or_else(CommandError::vault_locked)?;
            unlocked.vault.set_master_password(new_password, this.kdf)?;
            this.touch();
            Ok(())
        })
        .await
    }

    pub async fn set_keychain_enabled(&self, enabled: bool) -> CommandResult<()> {
        self.blocking(move |this| {
            let mut state = this.lock_state()?;
            let unlocked = state.as_mut().ok_or_else(CommandError::vault_locked)?;
            unlocked.vault.set_keychain_enabled(this.keystore.as_ref(), enabled)?;
            this.touch();
            Ok(())
        })
        .await
    }
}

fn migrate(data: &mut VaultData) {
    // Version 1 is the first format; future migrations go here.
    data.version = crate::models::VAULT_DATA_VERSION;
}

fn status_from(exists: bool, unlocked: bool, keychain_available: bool, info: Option<VaultInfo>) -> VaultStatus {
    VaultStatus {
        exists,
        unlocked,
        keychain_available,
        keychain_enabled: info.is_some_and(|i| i.keychain_enabled),
        password_enabled: info.is_some_and(|i| i.password_enabled),
    }
}

#[cfg(test)]
mod tests {
    use appex_vault::MemoryKeyStore;

    use super::*;
    use crate::models::DriveConfig;

    fn manager(dir: &tempfile::TempDir, keychain: bool) -> VaultManager {
        VaultManager::with_kdf(
            VaultFile::new(dir.path().join("vault.bin")),
            Arc::new(MemoryKeyStore::new(keychain)),
            KdfParams::insecure_for_tests(),
        )
    }

    #[tokio::test]
    async fn create_update_lock_and_unlock_with_keychain() {
        let dir = tempfile::tempdir().unwrap();
        let vault = manager(&dir, true);
        assert!(!vault.status().await.unwrap().exists);

        vault.create(true, None).await.unwrap();
        vault
            .update(|data| {
                data.drive = DriveConfig { client_id: Some("client".into()), ..DriveConfig::default() };
                Ok(())
            })
            .await
            .unwrap();
        assert!(vault.lock());
        assert!(matches!(vault.read(|_| ()), Err(e) if e.code == "vault_locked"));

        vault.unlock(None).await.unwrap();
        let client = vault.read(|data| data.drive.client_id.clone()).unwrap();
        assert_eq!(client.as_deref(), Some("client"));
        let status = vault.status().await.unwrap();
        assert!(status.unlocked && status.keychain_enabled && !status.password_enabled);
    }

    #[tokio::test]
    async fn keychain_is_required_when_requested() {
        let dir = tempfile::tempdir().unwrap();
        let vault = manager(&dir, false);
        let err = vault.create(true, None).await.unwrap_err();
        assert_eq!(err.code, "keychain_unavailable");
    }

    #[tokio::test]
    async fn failed_update_keeps_previous_data() {
        let dir = tempfile::tempdir().unwrap();
        let vault = manager(&dir, false);
        vault.create(false, Some(SecretString::from("correct horse"))).await.unwrap();
        let err = vault
            .update(|data| {
                data.drive.client_id = Some("changed".into());
                Err::<(), _>(CommandError::invalid_input("nope"))
            })
            .await
            .unwrap_err();
        assert_eq!(err.code, "invalid_input");
        assert_eq!(vault.read(|d| d.drive.client_id.clone()).unwrap(), None);

        vault.lock();
        let err = vault.unlock(Some(SecretString::from("wrong"))).await.unwrap_err();
        assert_eq!(err.code, "vault_wrong_password");
        vault.unlock(Some(SecretString::from("correct horse"))).await.unwrap();
    }
}
