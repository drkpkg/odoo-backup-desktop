use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

use crate::Result;

/// Which backups a target holds: one folder/prefix per Odoo instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TargetSpec {
    pub instance_id: String,
    pub instance_name: String,
}

/// Opaque, adapter-specific target identifier (folder id, directory path, …).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TargetId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadMeta {
    pub instance_id: String,
    pub database: String,
    pub sha256: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteObject {
    pub id: String,
    pub name: String,
    pub size: Option<u64>,
    pub created_at: Option<DateTime<Utc>>,
    pub instance_id: Option<String>,
    pub web_link: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    pub resumable: bool,
    pub server_side_checksum: bool,
    pub max_file_size: Option<u64>,
}

/// `(bytes_sent, total_bytes)`.
pub type UploadProgressFn = Arc<dyn Fn(u64, u64) + Send + Sync>;

#[async_trait]
pub trait StorageAdapter: Send + Sync {
    /// Stable id, e.g. "local" or "google_drive".
    fn provider_id(&self) -> &'static str;
    fn capabilities(&self) -> Capabilities;

    /// Finds or creates the per-instance folder and returns its id.
    async fn ensure_target(&self, spec: &TargetSpec) -> Result<TargetId>;

    /// Streams `file` to the target. Must not load the whole file in memory.
    async fn upload(
        &self,
        target: &TargetId,
        file: &Path,
        meta: &UploadMeta,
        progress: UploadProgressFn,
        cancel: CancellationToken,
    ) -> Result<RemoteObject>;

    /// Backups created by Odoo Backup Desktop in `target`, newest first.
    async fn list_backups(&self, target: &TargetId) -> Result<Vec<RemoteObject>>;

    async fn delete(&self, object: &RemoteObject) -> Result<()>;
}
