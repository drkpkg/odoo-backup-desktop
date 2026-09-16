//! Google Drive account helpers shared by commands and the backup runner.

use appex_storage::gdrive::{DriveOptions, GoogleDriveAdapter, OAuthClient};
use secrecy::SecretString;

use crate::error::{CommandError, CommandResult};
use crate::models::{DriveConfig, DriveStatus};
use crate::settings::Settings;

/// OAuth client embedded at build time (`APPEX_GDRIVE_CLIENT_ID` / `APPEX_GDRIVE_CLIENT_SECRET`).
/// A client configured in the app takes precedence.
const BUILTIN_CLIENT_ID: Option<&str> = option_env!("APPEX_GDRIVE_CLIENT_ID");
const BUILTIN_CLIENT_SECRET: Option<&str> = option_env!("APPEX_GDRIVE_CLIENT_SECRET");

pub fn oauth_client(config: &DriveConfig) -> Option<OAuthClient> {
    match config.client_id.as_deref().filter(|id| !id.is_empty()) {
        Some(client_id) => Some(OAuthClient {
            client_id: client_id.to_owned(),
            client_secret: config.client_secret.as_ref().filter(|s| !s.is_empty()).map(|s| s.to_secret_string()),
        }),
        None => BUILTIN_CLIENT_ID.filter(|id| !id.is_empty()).map(|client_id| OAuthClient {
            client_id: client_id.to_owned(),
            client_secret: BUILTIN_CLIENT_SECRET.filter(|s| !s.is_empty()).map(SecretString::from),
        }),
    }
}

pub fn status(config: &DriveConfig) -> DriveStatus {
    DriveStatus {
        configured: oauth_client(config).is_some(),
        client_id: config.client_id.clone(),
        has_client_secret: config.client_secret.as_ref().is_some_and(|s| !s.is_empty()),
        connected: config.refresh_token.as_ref().is_some_and(|t| !t.is_empty()),
        email: config.email.clone(),
        display_name: config.display_name.clone(),
    }
}

pub fn drive_options(settings: &Settings) -> DriveOptions {
    DriveOptions {
        root_folder_name: settings.drive.root_folder_name.clone(),
        shared_drive_id: settings.drive.shared_drive_id.clone(),
        permanent_delete: settings.drive.permanent_delete,
        ..DriveOptions::default()
    }
}

/// Builds the adapter for a connected account.
pub fn adapter(http: &reqwest::Client, config: &DriveConfig, settings: &Settings) -> CommandResult<GoogleDriveAdapter> {
    let client = oauth_client(config)
        .ok_or_else(|| CommandError::new("drive_not_configured", "Google Drive OAuth client is not configured"))?;
    let refresh_token = config
        .refresh_token
        .as_ref()
        .filter(|t| !t.is_empty())
        .ok_or_else(|| CommandError::new("drive_not_connected", "Google Drive account is not connected"))?;
    Ok(GoogleDriveAdapter::new(http.clone(), client, refresh_token.to_secret_string(), drive_options(settings)))
}

#[cfg(test)]
mod tests {
    use appex_vault::SecretField;

    use super::*;

    #[test]
    fn status_never_exposes_secrets() {
        let config = DriveConfig {
            client_id: Some("id.apps.googleusercontent.com".into()),
            client_secret: Some(SecretField::new("GOCSPX-secret")),
            refresh_token: Some(SecretField::new("1//refresh")),
            email: Some("ops@appex.lat".into()),
            display_name: None,
        };
        let json = serde_json::to_string(&status(&config)).unwrap();
        assert!(!json.contains("GOCSPX-secret"));
        assert!(!json.contains("1//refresh"));
        assert!(json.contains("\"connected\":true"));
        assert!(json.contains("\"hasClientSecret\":true"));
    }

    #[test]
    fn app_client_takes_precedence() {
        let config = DriveConfig { client_id: Some("mine".into()), ..DriveConfig::default() };
        assert_eq!(oauth_client(&config).unwrap().client_id, "mine");
    }
}
