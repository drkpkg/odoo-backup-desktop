//! Non-secret settings persisted as `settings.json` in the app data directory.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct DriveSettings {
    pub root_folder_name: String,
    pub keep_last: Option<u32>,
    pub permanent_delete: bool,
    pub shared_drive_id: Option<String>,
}

impl Default for DriveSettings {
    fn default() -> Self {
        Self {
            root_folder_name: "Appex Backup".into(),
            keep_last: Some(10),
            permanent_delete: false,
            shared_drive_id: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    pub download_dir: String,
    pub keep_last_local: Option<u32>,
    pub max_concurrent_backups: u32,
    pub server_prepare_timeout_minutes: u32,
    pub auto_lock_minutes: Option<u32>,
    pub drive: DriveSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            download_dir: String::new(),
            keep_last_local: Some(10),
            max_concurrent_backups: 2,
            server_prepare_timeout_minutes: 60,
            auto_lock_minutes: Some(15),
            drive: DriveSettings::default(),
        }
    }
}

impl Settings {
    pub fn validated(mut self) -> CommandResult<Self> {
        self.download_dir = self.download_dir.trim().to_owned();
        if self.download_dir.is_empty() {
            return Err(CommandError::invalid_input("downloadDir is required"));
        }
        if !Path::new(&self.download_dir).is_absolute() {
            return Err(CommandError::invalid_input("downloadDir must be an absolute path"));
        }
        self.max_concurrent_backups = self.max_concurrent_backups.clamp(1, 4);
        self.server_prepare_timeout_minutes = self.server_prepare_timeout_minutes.clamp(5, 720);
        self.keep_last_local = self.keep_last_local.filter(|n| *n > 0);
        self.auto_lock_minutes = self.auto_lock_minutes.filter(|n| *n > 0).map(|n| n.min(24 * 60));
        self.drive.root_folder_name = self.drive.root_folder_name.trim().to_owned();
        if self.drive.root_folder_name.is_empty() {
            self.drive.root_folder_name = DriveSettings::default().root_folder_name;
        }
        self.drive.keep_last = self.drive.keep_last.filter(|n| *n > 0);
        self.drive.shared_drive_id =
            self.drive.shared_drive_id.map(|id| id.trim().to_owned()).filter(|id| !id.is_empty());
        Ok(self)
    }
}

pub fn load(path: &Path, default_download_dir: &Path) -> Settings {
    let mut settings = std::fs::read(path)
        .ok()
        .and_then(|bytes| match serde_json::from_slice::<Settings>(&bytes) {
            Ok(settings) => Some(settings),
            Err(err) => {
                tracing::warn!(error = %err, "settings.json is invalid, using defaults");
                None
            }
        })
        .unwrap_or_default();
    if settings.download_dir.trim().is_empty() {
        settings.download_dir = default_download_dir.to_string_lossy().into_owned();
    }
    settings.validated().unwrap_or_else(|_| Settings {
        download_dir: default_download_dir.to_string_lossy().into_owned(),
        ..Settings::default()
    })
}

pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(settings).map_err(std::io::Error::other)?;
    let tmp: PathBuf = path.with_extension("json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validation_clamps_values() {
        let settings = Settings {
            download_dir: std::env::temp_dir().to_string_lossy().into_owned(),
            max_concurrent_backups: 9,
            server_prepare_timeout_minutes: 1,
            keep_last_local: Some(0),
            auto_lock_minutes: Some(0),
            drive: DriveSettings {
                root_folder_name: "  ".into(),
                shared_drive_id: Some(" ".into()),
                ..Default::default()
            },
        }
        .validated()
        .unwrap();
        assert_eq!(settings.max_concurrent_backups, 4);
        assert_eq!(settings.server_prepare_timeout_minutes, 5);
        assert_eq!(settings.keep_last_local, None);
        assert_eq!(settings.auto_lock_minutes, None);
        assert_eq!(settings.drive.root_folder_name, "Appex Backup");
        assert_eq!(settings.drive.shared_drive_id, None);
    }

    #[test]
    fn relative_download_dir_is_rejected() {
        let settings = Settings { download_dir: "backups".into(), ..Settings::default() };
        assert_eq!(settings.validated().unwrap_err().code, "invalid_input");
    }

    #[test]
    fn load_falls_back_to_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, b"{not json").unwrap();
        let settings = load(&path, dir.path());
        assert_eq!(settings.download_dir, dir.path().to_string_lossy());
        assert_eq!(settings.max_concurrent_backups, 2);
    }
}
