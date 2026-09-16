use std::path::PathBuf;
use std::sync::{Mutex, RwLock};
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::{CommandError, CommandResult};
use crate::history::History;
use crate::jobs::{JobRegistry, Limiter};
use crate::settings::{self, Settings};
use crate::vault::VaultManager;

pub const KEYCHAIN_SERVICE: &str = "lat.appex.backup";
pub const KEYCHAIN_ACCOUNT: &str = "vault-dek";

#[derive(Debug, Clone)]
pub struct AppPaths {
    pub data_dir: PathBuf,
    pub vault_file: PathBuf,
    pub settings_file: PathBuf,
    pub history_file: PathBuf,
}

impl AppPaths {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            vault_file: data_dir.join("vault.bin"),
            settings_file: data_dir.join("settings.json"),
            history_file: data_dir.join("history.sqlite3"),
            data_dir,
        }
    }
}

pub struct AppState {
    pub app_version: String,
    /// Desktop notifications when a backup ends.
    pub notifications: bool,
    pub paths: AppPaths,
    pub vault: VaultManager,
    pub settings: RwLock<Settings>,
    pub history: History,
    pub jobs: JobRegistry,
    pub limiter: Limiter,
    pub http: reqwest::Client,
    /// Pending Google authorization: (attempt id, cancel token).
    pub drive_connect: Mutex<Option<(String, CancellationToken)>>,
}

impl AppState {
    pub fn settings(&self) -> Settings {
        self.settings.read().map(|s| s.clone()).unwrap_or_default()
    }

    pub fn replace_settings(&self, new_settings: Settings) -> CommandResult<Settings> {
        let validated = new_settings.validated()?;
        std::fs::create_dir_all(&validated.download_dir)
            .map_err(|err| CommandError::new("download_dir_invalid", err.to_string()))?;
        settings::save(&self.paths.settings_file, &validated)?;
        *self.settings.write().map_err(|_| CommandError::internal("settings lock poisoned"))? = validated.clone();
        Ok(validated)
    }
}

pub fn build_http_client(app_version: &str) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(format!("AppexBackup/{app_version}"))
        .connect_timeout(Duration::from_secs(20))
        .tcp_keepalive(Duration::from_secs(60))
        .build()
        .expect("HTTP client configuration is valid")
}
