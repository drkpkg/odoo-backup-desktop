//! Backup history stored in SQLite. Holds no secrets: instance names are
//! resolved from the vault when listing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use obd_odoo::TransportKind;
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use crate::error::{CommandError, CommandResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackupStatus {
    Running,
    Success,
    Failed,
    Cancelled,
}

impl BackupStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Success => "success",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "running" => Self::Running,
            "success" => Self::Success,
            "cancelled" => Self::Cancelled,
            _ => Self::Failed,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DriveUploadStatus {
    Skipped,
    Success,
    Failed,
}

impl DriveUploadStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Skipped => "skipped",
            Self::Success => "success",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> Self {
        match value {
            "success" => Self::Success,
            "failed" => Self::Failed,
            _ => Self::Skipped,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveResult {
    pub status: DriveUploadStatus,
    pub file_id: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: String,
    pub instance_id: String,
    pub instance_name: String,
    pub status: BackupStatus,
    pub transport: Option<TransportKind>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    pub file_path: Option<String>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub odoo_version: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub drive: DriveResult,
}

/// Final values written when a backup ends.
#[derive(Debug, Clone, Default)]
pub struct Completion {
    pub transport: Option<TransportKind>,
    pub file_path: Option<PathBuf>,
    pub size_bytes: Option<u64>,
    pub sha256: Option<String>,
    pub odoo_version: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Clone)]
pub struct History {
    conn: Arc<Mutex<Connection>>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS backups (
    id            TEXT PRIMARY KEY,
    instance_id   TEXT NOT NULL,
    status        TEXT NOT NULL,
    transport     TEXT,
    started_at    TEXT NOT NULL,
    finished_at   TEXT,
    file_path     TEXT,
    size_bytes    INTEGER,
    sha256        TEXT,
    odoo_version  TEXT,
    error_code    TEXT,
    error_message TEXT,
    drive_status  TEXT NOT NULL DEFAULT 'skipped',
    drive_file_id TEXT,
    drive_error   TEXT
);
CREATE INDEX IF NOT EXISTS backups_instance_started ON backups (instance_id, started_at DESC);
CREATE INDEX IF NOT EXISTS backups_started ON backups (started_at DESC);
";

impl History {
    pub fn open(path: &Path) -> CommandResult<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> CommandResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> CommandResult<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.execute_batch(SCHEMA)?;
        // Backups interrupted by a crash or forced exit.
        conn.execute(
            "UPDATE backups SET status = 'failed', error_code = 'interrupted',
             error_message = 'the application was closed during the backup', finished_at = ?1
             WHERE status = 'running'",
            params![Utc::now().to_rfc3339()],
        )?;
        Ok(Self { conn: Arc::new(Mutex::new(conn)) })
    }

    async fn with_conn<T, F>(&self, f: F) -> CommandResult<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> rusqlite::Result<T> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().map_err(|_| CommandError::internal("history lock poisoned"))?;
            f(&guard).map_err(CommandError::from)
        })
        .await?
    }

    pub async fn insert_running(&self, id: &str, instance_id: &str, started_at: DateTime<Utc>) -> CommandResult<()> {
        let (id, instance_id) = (id.to_owned(), instance_id.to_owned());
        self.with_conn(move |conn| {
            conn.execute(
                "INSERT INTO backups (id, instance_id, status, started_at) VALUES (?1, ?2, 'running', ?3)",
                params![id, instance_id, started_at.to_rfc3339()],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn finish(&self, id: &str, status: BackupStatus, completion: Completion) -> CommandResult<()> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE backups SET status = ?2, transport = ?3, finished_at = ?4, file_path = ?5, size_bytes = ?6,
                 sha256 = ?7, odoo_version = ?8, error_code = ?9, error_message = ?10 WHERE id = ?1",
                params![
                    id,
                    status.as_str(),
                    completion.transport.map(transport_str),
                    Utc::now().to_rfc3339(),
                    completion.file_path.map(|p| p.to_string_lossy().into_owned()),
                    completion.size_bytes.map(|s| s as i64),
                    completion.sha256,
                    completion.odoo_version,
                    completion.error_code,
                    completion.error_message,
                ],
            )
            .map(|_| ())
        })
        .await
    }

    pub async fn set_drive(&self, id: &str, drive: DriveResult) -> CommandResult<()> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            conn.execute(
                "UPDATE backups SET drive_status = ?2, drive_file_id = ?3, drive_error = ?4 WHERE id = ?1",
                params![id, drive.status.as_str(), drive.file_id, drive.error_message],
            )
            .map(|_| ())
        })
        .await
    }

    /// Entries newest first. `instance_name` is left empty; callers fill it from the vault.
    pub async fn list(&self, instance_id: Option<String>, limit: u32) -> CommandResult<Vec<HistoryEntry>> {
        let limit = i64::from(limit.clamp(1, 1000));
        self.with_conn(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM backups WHERE (?1 IS NULL OR instance_id = ?1) ORDER BY started_at DESC LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![instance_id, limit], row_to_entry)?;
            rows.collect()
        })
        .await
    }

    pub async fn get(&self, id: &str) -> CommandResult<Option<HistoryEntry>> {
        let id = id.to_owned();
        self.with_conn(move |conn| {
            conn.query_row("SELECT * FROM backups WHERE id = ?1", params![id], row_to_entry).optional()
        })
        .await
    }

    /// Latest entry per instance.
    pub async fn latest_per_instance(&self) -> CommandResult<Vec<HistoryEntry>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT b.* FROM backups b
                 JOIN (SELECT instance_id, MAX(started_at) AS started_at FROM backups GROUP BY instance_id) latest
                 ON latest.instance_id = b.instance_id AND latest.started_at = b.started_at",
            )?;
            let rows = stmt.query_map([], row_to_entry)?;
            rows.collect()
        })
        .await
    }

    pub async fn delete_for_instance(&self, instance_id: &str) -> CommandResult<()> {
        let instance_id = instance_id.to_owned();
        self.with_conn(move |conn| {
            conn.execute("DELETE FROM backups WHERE instance_id = ?1", params![instance_id]).map(|_| ())
        })
        .await
    }
}

fn transport_str(kind: TransportKind) -> &'static str {
    match kind {
        TransportKind::DbManager => "db_manager",
        TransportKind::ObdModule => "obd_module",
    }
}

fn parse_time(value: Option<String>) -> Option<DateTime<Utc>> {
    value.and_then(|v| DateTime::parse_from_rfc3339(&v).ok()).map(|t| t.with_timezone(&Utc))
}

fn row_to_entry(row: &Row<'_>) -> rusqlite::Result<HistoryEntry> {
    let transport: Option<String> = row.get("transport")?;
    let started_at: String = row.get("started_at")?;
    let size: Option<i64> = row.get("size_bytes")?;
    let drive_status: String = row.get("drive_status")?;
    Ok(HistoryEntry {
        id: row.get("id")?,
        instance_id: row.get("instance_id")?,
        instance_name: String::new(),
        status: BackupStatus::parse(&row.get::<_, String>("status")?),
        transport: transport.as_deref().and_then(|t| match t {
            "db_manager" => Some(TransportKind::DbManager),
            "obd_module" => Some(TransportKind::ObdModule),
            _ => None,
        }),
        started_at: parse_time(Some(started_at)).unwrap_or_else(Utc::now),
        finished_at: parse_time(row.get("finished_at")?),
        file_path: row.get("file_path")?,
        size_bytes: size.and_then(|s| u64::try_from(s).ok()),
        sha256: row.get("sha256")?,
        odoo_version: row.get("odoo_version")?,
        error_code: row.get("error_code")?,
        error_message: row.get("error_message")?,
        drive: DriveResult {
            status: DriveUploadStatus::parse(&drive_status),
            file_id: row.get("drive_file_id")?,
            error_message: row.get("drive_error")?,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn records_lifecycle_and_latest_per_instance() {
        let history = History::open_in_memory().unwrap();
        let t0 = Utc::now() - chrono::Duration::minutes(10);
        history.insert_running("a", "inst-1", t0).await.unwrap();
        history.insert_running("b", "inst-1", t0 + chrono::Duration::minutes(5)).await.unwrap();
        history.insert_running("c", "inst-2", t0).await.unwrap();

        history
            .finish(
                "b",
                BackupStatus::Success,
                Completion {
                    transport: Some(TransportKind::DbManager),
                    file_path: Some(PathBuf::from("/tmp/x.zip")),
                    size_bytes: Some(42),
                    sha256: Some("abc".into()),
                    odoo_version: Some("17.0".into()),
                    ..Completion::default()
                },
            )
            .await
            .unwrap();
        history
            .set_drive(
                "b",
                DriveResult { status: DriveUploadStatus::Success, file_id: Some("f1".into()), error_message: None },
            )
            .await
            .unwrap();

        let entry = history.get("b").await.unwrap().unwrap();
        assert_eq!(entry.status, BackupStatus::Success);
        assert_eq!(entry.size_bytes, Some(42));
        assert_eq!(entry.transport, Some(TransportKind::DbManager));
        assert_eq!(entry.drive.status, DriveUploadStatus::Success);

        let list = history.list(Some("inst-1".into()), 10).await.unwrap();
        assert_eq!(list.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["b", "a"]);

        let mut latest = history.latest_per_instance().await.unwrap();
        latest.sort_by(|x, y| x.instance_id.cmp(&y.instance_id));
        assert_eq!(latest.iter().map(|e| e.id.as_str()).collect::<Vec<_>>(), ["b", "c"]);

        history.delete_for_instance("inst-1").await.unwrap();
        assert!(history.list(Some("inst-1".into()), 10).await.unwrap().is_empty());
    }

    #[test]
    fn running_entries_are_marked_interrupted_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("history.sqlite3");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(SCHEMA).unwrap();
            conn.execute(
                "INSERT INTO backups (id, instance_id, status, started_at) VALUES ('x', 'i', 'running', ?1)",
                params![Utc::now().to_rfc3339()],
            )
            .unwrap();
        }
        let history = History::open(&path).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        let entry = runtime.block_on(history.get("x")).unwrap().unwrap();
        assert_eq!(entry.status, BackupStatus::Failed);
        assert_eq!(entry.error_code.as_deref(), Some("interrupted"));
    }
}
