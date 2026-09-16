//! Data stored in the vault and DTOs exchanged with the UI.
//!
//! Records (`*Record`, `VaultData`) may hold secrets and never leave Rust.
//! Views (`*View`, `AppStatus`, `DriveStatus`) are what the webview receives.

use chrono::{DateTime, Utc};
use obd_odoo::{ProbeReport, ProtocolPreference, SecretKind, TransportKind};
use obd_vault::SecretField;
use serde::{Deserialize, Deserializer, Serialize};

use crate::history::HistoryEntry;

pub const VAULT_DATA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultData {
    pub version: u32,
    #[serde(default)]
    pub instances: Vec<InstanceRecord>,
    #[serde(default)]
    pub drive: DriveConfig,
}

impl Default for VaultData {
    fn default() -> Self {
        Self { version: VAULT_DATA_VERSION, instances: Vec::new(), drive: DriveConfig::default() }
    }
}

impl VaultData {
    pub fn instance(&self, id: &str) -> Option<&InstanceRecord> {
        self.instances.iter().find(|instance| instance.id == id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportPreference {
    #[default]
    Auto,
    DbManager,
    ObdModule,
}

impl TransportPreference {
    pub fn forced(self) -> Option<TransportKind> {
        match self {
            Self::Auto => None,
            Self::DbManager => Some(TransportKind::DbManager),
            Self::ObdModule => Some(TransportKind::ObdModule),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceRecord {
    pub id: String,
    pub name: String,
    pub url: String,
    pub database: String,
    pub login: String,
    pub secret_kind: SecretKind,
    pub secret: Option<SecretField>,
    pub master_password: Option<SecretField>,
    pub transport: TransportPreference,
    pub protocol: ProtocolPreference,
    pub include_filestore: bool,
    pub upload_to_drive: bool,
    pub last_probe: Option<ProbeSnapshot>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl InstanceRecord {
    pub fn view(&self, last_backup: Option<HistoryEntry>) -> InstanceView {
        InstanceView {
            id: self.id.clone(),
            name: self.name.clone(),
            url: self.url.clone(),
            database: self.database.clone(),
            login: self.login.clone(),
            secret_kind: self.secret_kind,
            has_secret: self.secret.as_ref().is_some_and(|secret| !secret.is_empty()),
            has_master_password: self.master_password.as_ref().is_some_and(|secret| !secret.is_empty()),
            transport: self.transport,
            protocol: self.protocol,
            include_filestore: self.include_filestore,
            upload_to_drive: self.upload_to_drive,
            last_probe: self.last_probe.clone(),
            last_backup,
            created_at: self.created_at,
            updated_at: self.updated_at,
        }
    }
}

/// A probe report plus the time it was taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeSnapshot {
    #[serde(flatten)]
    pub report: ProbeReport,
    pub checked_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveConfig {
    pub client_id: Option<String>,
    pub client_secret: Option<SecretField>,
    pub refresh_token: Option<SecretField>,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceView {
    pub id: String,
    pub name: String,
    pub url: String,
    pub database: String,
    pub login: String,
    pub secret_kind: SecretKind,
    pub has_secret: bool,
    pub has_master_password: bool,
    pub transport: TransportPreference,
    pub protocol: ProtocolPreference,
    pub include_filestore: bool,
    pub upload_to_drive: bool,
    pub last_probe: Option<ProbeSnapshot>,
    pub last_backup: Option<HistoryEntry>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstanceInput {
    pub id: Option<String>,
    pub name: String,
    pub url: String,
    pub database: String,
    #[serde(default)]
    pub login: String,
    pub secret_kind: SecretKind,
    /// Absent or empty: keep the stored secret.
    #[serde(default)]
    pub secret: Option<String>,
    /// Absent: keep; `null`: remove; string: replace.
    #[serde(default, deserialize_with = "deserialize_patch")]
    pub master_password: Option<Option<String>>,
    pub transport: TransportPreference,
    pub protocol: ProtocolPreference,
    pub include_filestore: bool,
    pub upload_to_drive: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeRequest {
    pub instance_id: Option<String>,
    pub url: String,
    #[serde(default)]
    pub database: Option<String>,
    #[serde(default)]
    pub login: Option<String>,
    pub secret_kind: SecretKind,
    #[serde(default)]
    pub secret: Option<String>,
    #[serde(default)]
    pub master_password: Option<String>,
    pub protocol: ProtocolPreference,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultStatus {
    pub exists: bool,
    pub unlocked: bool,
    pub keychain_available: bool,
    pub keychain_enabled: bool,
    pub password_enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppStatus {
    pub app_version: String,
    pub vault: VaultStatus,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveStatus {
    pub configured: bool,
    pub client_id: Option<String>,
    pub has_client_secret: bool,
    pub connected: bool,
    pub email: Option<String>,
    pub display_name: Option<String>,
}

/// Distinguishes an absent field (`None`) from an explicit `null` (`Some(None)`).
pub fn deserialize_patch<'de, D, T>(deserializer: D) -> Result<Option<Option<T>>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::<T>::deserialize(deserializer).map(Some)
}

pub fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}
