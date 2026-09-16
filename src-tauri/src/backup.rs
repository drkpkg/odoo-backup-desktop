//! Backup job runner: resolve transport → download + validate → local retention →
//! optional Google Drive upload + remote retention → history + notification.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use appex_odoo::{
    AppexModuleTransport, BackupPhase, BackupRequest, BackupTransport, Credentials, DbManagerTransport,
    DownloadedBackup, OdooRpc, OdooVersion, SecretKind, TransportKind,
};
use appex_storage::local::LocalFolderAdapter;
use appex_storage::{RetentionPolicy, StorageAdapter, StorageError, TargetSpec, UploadMeta, apply_retention};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, Runtime};
use tauri_plugin_notification::NotificationExt;
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::error::{CommandError, CommandResult};
use crate::history::{BackupStatus, Completion, DriveResult, DriveUploadStatus, HistoryEntry};
use crate::jobs::BackupStage;
use crate::models::{DriveConfig, InstanceRecord, TransportPreference};
use crate::settings::Settings;
use crate::state::AppState;

const MODULE_POLL_INTERVAL: Duration = Duration::from_secs(2);
const PROGRESS_THROTTLE: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum BackupEvent {
    Started {
        job_id: String,
        instance_id: String,
        transport: TransportKind,
    },
    Progress {
        job_id: String,
        stage: BackupStage,
        #[serde(skip_serializing_if = "Option::is_none")]
        elapsed_secs: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        received: Option<u64>,
        #[serde(skip_serializing_if = "Option::is_none")]
        total: Option<Option<u64>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        sent: Option<u64>,
    },
    Completed {
        job_id: String,
        entry: Box<HistoryEntry>,
    },
    Failed {
        job_id: String,
        code: String,
        message: String,
    },
    Cancelled {
        job_id: String,
    },
}

/// Normalizes an instance URL to its base (`https://host[:port]/[path/]`).
pub fn parse_base_url(raw: &str) -> CommandResult<Url> {
    let mut url = Url::parse(raw.trim()).map_err(|err| CommandError::new("invalid_url", err.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(CommandError::new("invalid_url", "the URL must start with http:// or https://"));
    }
    url.set_query(None);
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    let path = url.path().trim_end_matches('/');
    let path = path.strip_suffix("/web").unwrap_or(path).to_owned();
    url.set_path(&format!("{path}/"));
    Ok(url)
}

/// `<db>_<YYYY-MM-DD_HH-MM-SS>` in UTC, like Odoo's database manager.
pub fn file_stem(database: &str, now: chrono::DateTime<Utc>) -> String {
    let safe: String = database
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect();
    format!("{safe}_{}", now.format("%Y-%m-%d_%H-%M-%S"))
}

pub fn target_spec(instance: &InstanceRecord) -> TargetSpec {
    TargetSpec { instance_id: instance.id.clone(), instance_name: instance.name.clone() }
}

/// Registers and spawns a backup job. Returns the job id.
pub async fn start<R: Runtime>(
    app: AppHandle<R>,
    state: &AppState,
    instance_id: &str,
    channel: Channel<BackupEvent>,
) -> CommandResult<String> {
    let (instance, drive) = state.vault.read(|data| (data.instance(instance_id).cloned(), data.drive.clone()))?;
    let instance = instance.ok_or_else(|| CommandError::not_found("instance not found"))?;
    if state.jobs.is_instance_running(&instance.id) {
        return Err(CommandError::new("backup_in_progress", "a backup of this instance is already running"));
    }

    let job_id = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    state.history.insert_running(&job_id, &instance.id, Utc::now()).await?;
    state.jobs.register(&job_id, &instance.id, cancel.clone());

    let job = Job { app: app.clone(), job_id: job_id.clone(), instance, drive, channel, cancel };
    tauri::async_runtime::spawn(job.run());
    Ok(job_id)
}

struct Job<R: Runtime> {
    app: AppHandle<R>,
    job_id: String,
    instance: InstanceRecord,
    drive: DriveConfig,
    channel: Channel<BackupEvent>,
    cancel: CancellationToken,
}

struct LocalOutcome {
    backup: DownloadedBackup,
    transport: TransportKind,
    version: OdooVersion,
}

impl<R: Runtime> Job<R> {
    fn state(&self) -> tauri::State<'_, AppState> {
        self.app.state::<AppState>()
    }

    fn emit(&self, event: BackupEvent) {
        if let Err(err) = self.channel.send(event) {
            tracing::debug!(job = %self.job_id, error = %err, "backup event not delivered");
        }
    }

    fn progress(&self, stage: BackupStage) {
        self.state().jobs.update(&self.job_id, |job| job.stage = stage);
        self.emit(BackupEvent::Progress {
            job_id: self.job_id.clone(),
            stage,
            elapsed_secs: None,
            received: None,
            total: None,
            sent: None,
        });
    }

    async fn run(self) {
        let settings = self.state().settings();
        let limiter = self.state().limiter.clone();
        let max = settings.max_concurrent_backups as usize;
        let app = self.app.clone();
        let permit = limiter
            .acquire(
                move || {
                    app.state::<AppState>().settings.read().map(|s| s.max_concurrent_backups as usize).unwrap_or(max)
                },
                &self.cancel,
            )
            .await;

        let local = match permit {
            None => Err(CommandError::new("cancelled", "backup cancelled")),
            Some(_) => self.run_local(&settings).await,
        };

        match local {
            Ok(outcome) => self.finish_success(outcome, &settings).await,
            Err(err) => self.finish_error(err).await,
        }
        self.state().jobs.remove(&self.job_id);
    }

    async fn run_local(&self, settings: &Settings) -> CommandResult<LocalOutcome> {
        let state = self.state();
        let http = state.http.clone();
        let instance = &self.instance;
        self.progress(BackupStage::Requesting);

        let base_url = parse_base_url(&instance.url)?;
        let version = cancellable(&self.cancel, appex_odoo::detect_version(&http, &base_url)).await??;
        if !version.is_supported() {
            return Err(appex_odoo::OdooError::UnsupportedVersion(version.label()).into());
        }

        let credentials = credentials(instance);
        let rpc = match &credentials {
            Some(credentials) => {
                let protocol = appex_odoo::select_protocol(&version, instance.protocol, credentials.kind)
                    .or_else(|_| appex_odoo::select_protocol(&version, Default::default(), credentials.kind))?;
                Some(appex_odoo::connect(
                    http.clone(),
                    base_url.clone(),
                    instance.database.clone(),
                    credentials.clone(),
                    protocol,
                ))
            }
            None => None,
        };

        let prepare_timeout = Duration::from_secs(u64::from(settings.server_prepare_timeout_minutes) * 60);
        let kind = self.resolve_transport(rpc.as_deref(), credentials.as_ref()).await?;
        let transport: Box<dyn BackupTransport> = match kind {
            TransportKind::DbManager => {
                let master = instance.master_password.as_ref().filter(|p| !p.is_empty()).ok_or_else(|| {
                    CommandError::new("missing_master_password", "the database manager needs the master password")
                })?;
                Box::new(DbManagerTransport::new(
                    http.clone(),
                    base_url.clone(),
                    master.to_secret_string(),
                    version.clone(),
                    prepare_timeout,
                ))
            }
            TransportKind::AppexModule => {
                let (Some(rpc), Some(credentials)) = (rpc.clone(), credentials.as_ref()) else {
                    return Err(CommandError::new("api_key_required", "the appex_backup module needs an API key"));
                };
                if credentials.kind != SecretKind::ApiKey {
                    return Err(CommandError::new("api_key_required", "the appex_backup module needs an API key"));
                }
                Box::new(AppexModuleTransport::new(
                    rpc,
                    http.clone(),
                    base_url.clone(),
                    credentials.secret.clone(),
                    MODULE_POLL_INTERVAL,
                    prepare_timeout,
                ))
            }
        };

        self.emit(BackupEvent::Started {
            job_id: self.job_id.clone(),
            instance_id: instance.id.clone(),
            transport: kind,
        });

        let local = LocalFolderAdapter::new(&settings.download_dir);
        let dest_dir = local.instance_dir(&target_spec(instance));
        tokio::fs::create_dir_all(&dest_dir)
            .await
            .map_err(|err| CommandError::new("download_dir_invalid", err.to_string()))?;
        let request = BackupRequest {
            database: instance.database.clone(),
            include_filestore: instance.include_filestore,
            dest_dir,
            file_stem: file_stem(&instance.database, Utc::now()),
        };

        let backup = transport.run(&request, self.phase_reporter(), self.cancel.clone()).await?;
        Ok(LocalOutcome { backup, transport: kind, version })
    }

    async fn resolve_transport(
        &self,
        rpc: Option<&dyn OdooRpc>,
        credentials: Option<&Credentials>,
    ) -> CommandResult<TransportKind> {
        if let Some(kind) = self.instance.transport.forced() {
            return Ok(kind);
        }
        debug_assert_eq!(self.instance.transport, TransportPreference::Auto);
        let has_api_key = credentials.is_some_and(|c| c.kind == SecretKind::ApiKey);
        if has_api_key
            && let Some(rpc) = rpc
            && module_available(rpc, &self.cancel).await
        {
            return Ok(TransportKind::AppexModule);
        }
        if self.instance.master_password.as_ref().is_some_and(|p| !p.is_empty()) {
            return Ok(TransportKind::DbManager);
        }
        Err(CommandError::new(
            "no_transport_available",
            "neither the appex_backup module (API key) nor the database manager (master password) is available",
        ))
    }

    fn phase_reporter(&self) -> appex_odoo::ProgressFn {
        let app = self.app.clone();
        let channel = self.channel.clone();
        let job_id = self.job_id.clone();
        let last_download = Arc::new(Mutex::new(Instant::now() - PROGRESS_THROTTLE));
        Arc::new(move |phase: BackupPhase| {
            let (stage, elapsed_secs, received, total) = match phase {
                BackupPhase::Requesting => (BackupStage::Requesting, None, None, None),
                BackupPhase::ServerPreparing { elapsed_secs } => {
                    (BackupStage::ServerPreparing, Some(elapsed_secs), None, None)
                }
                BackupPhase::Downloading { received, total } => {
                    let complete = total.is_some_and(|t| received >= t);
                    if let Ok(mut last) = last_download.lock() {
                        if !complete && last.elapsed() < PROGRESS_THROTTLE {
                            return;
                        }
                        *last = Instant::now();
                    }
                    (BackupStage::Downloading, None, Some(received), Some(total))
                }
                BackupPhase::Validating => (BackupStage::Validating, None, None, None),
            };
            app.state::<AppState>().jobs.update(&job_id, |job| {
                job.stage = stage;
                job.elapsed_secs = elapsed_secs;
                job.received = received;
                job.total = total;
            });
            let _ = channel.send(BackupEvent::Progress {
                job_id: job_id.clone(),
                stage,
                elapsed_secs,
                received,
                total,
                sent: None,
            });
        })
    }

    async fn finish_success(&self, outcome: LocalOutcome, settings: &Settings) {
        let state = self.state();
        let LocalOutcome { backup, transport, version } = outcome;
        let completion = Completion {
            transport: Some(transport),
            file_path: Some(backup.path.clone()),
            size_bytes: Some(backup.size),
            sha256: Some(backup.sha256.clone()),
            odoo_version: Some(version.label()),
            ..Completion::default()
        };
        if let Err(err) = state.history.finish(&self.job_id, BackupStatus::Success, completion).await {
            tracing::error!(job = %self.job_id, error = %err, "could not record backup in history");
        }

        self.apply_local_retention(settings).await;

        let mut cancelled = false;
        if self.instance.upload_to_drive {
            let drive = self.upload_to_drive(&backup, settings).await;
            cancelled = drive.error_message.as_deref() == Some("cancelled");
            if let Err(err) = state.history.set_drive(&self.job_id, drive.clone()).await {
                tracing::error!(job = %self.job_id, error = %err, "could not record Drive result");
            }
            if drive.status == DriveUploadStatus::Failed {
                self.notify("Backup guardado, pero falló la subida a Google Drive", &self.instance.name);
            }
        }

        if cancelled {
            self.emit(BackupEvent::Cancelled { job_id: self.job_id.clone() });
            return;
        }
        match state.history.get(&self.job_id).await {
            Ok(Some(mut entry)) => {
                entry.instance_name = self.instance.name.clone();
                self.emit(BackupEvent::Completed { job_id: self.job_id.clone(), entry: Box::new(entry) });
            }
            Ok(None) | Err(_) => self.emit(BackupEvent::Failed {
                job_id: self.job_id.clone(),
                code: "history_db".into(),
                message: "backup finished but the history entry could not be read".into(),
            }),
        }
        self.notify("Backup completado", &format!("{} · {}", self.instance.name, human_size(backup.size)));
    }

    async fn apply_local_retention(&self, settings: &Settings) {
        let Some(keep_last) = settings.keep_last_local else { return };
        self.progress(BackupStage::Retention);
        let adapter = LocalFolderAdapter::new(&settings.download_dir);
        let policy = RetentionPolicy { keep_last: Some(keep_last), max_age_days: None };
        let result = async {
            let target = adapter.ensure_target(&target_spec(&self.instance)).await?;
            apply_retention(&adapter, &target, &policy).await
        }
        .await;
        if let Err(err) = result {
            tracing::warn!(job = %self.job_id, error = %err, "local retention failed");
        }
    }

    async fn upload_to_drive(&self, backup: &DownloadedBackup, settings: &Settings) -> DriveResult {
        let failed = |message: String| DriveResult {
            status: DriveUploadStatus::Failed,
            file_id: None,
            error_message: Some(message),
        };
        let state = self.state();
        let adapter = match crate::drive::adapter(&state.http, &self.drive, settings) {
            Ok(adapter) => adapter,
            Err(err) => return failed(err.code),
        };

        self.progress(BackupStage::Uploading);
        let channel = self.channel.clone();
        let job_id = self.job_id.clone();
        let app = self.app.clone();
        let last = Arc::new(Mutex::new(Instant::now() - PROGRESS_THROTTLE));
        let progress: appex_storage::UploadProgressFn = Arc::new(move |sent, total| {
            if let Ok(mut last) = last.lock() {
                if sent < total && last.elapsed() < PROGRESS_THROTTLE {
                    return;
                }
                *last = Instant::now();
            }
            app.state::<AppState>().jobs.update(&job_id, |job| {
                job.stage = BackupStage::Uploading;
                job.sent = Some(sent);
                job.total = Some(Some(total));
            });
            let _ = channel.send(BackupEvent::Progress {
                job_id: job_id.clone(),
                stage: BackupStage::Uploading,
                elapsed_secs: None,
                received: None,
                total: Some(Some(total)),
                sent: Some(sent),
            });
        });

        let meta = UploadMeta {
            instance_id: self.instance.id.clone(),
            database: self.instance.database.clone(),
            sha256: backup.sha256.clone(),
            created_at: Utc::now(),
        };
        let upload = async {
            let target = adapter.ensure_target(&target_spec(&self.instance)).await?;
            let object = adapter.upload(&target, &backup.path, &meta, progress, self.cancel.clone()).await?;
            Ok::<_, StorageError>((target, object))
        }
        .await;

        match upload {
            Ok((target, object)) => {
                if let Some(keep_last) = settings.drive.keep_last {
                    self.progress(BackupStage::Retention);
                    let policy = RetentionPolicy { keep_last: Some(keep_last), max_age_days: None };
                    if let Err(err) = apply_retention(&adapter, &target, &policy).await {
                        tracing::warn!(job = %self.job_id, error = %err, "Drive retention failed");
                    }
                }
                DriveResult { status: DriveUploadStatus::Success, file_id: Some(object.id), error_message: None }
            }
            Err(StorageError::Cancelled) => failed("cancelled".into()),
            Err(err) => {
                tracing::warn!(job = %self.job_id, code = err.code(), error = %err, "Drive upload failed");
                failed(format!("{}: {err}", err.code()))
            }
        }
    }

    async fn finish_error(&self, err: CommandError) {
        let state = self.state();
        let cancelled = err.code == "cancelled";
        let status = if cancelled { BackupStatus::Cancelled } else { BackupStatus::Failed };
        let completion = Completion {
            error_code: (!cancelled).then(|| err.code.clone()),
            error_message: (!cancelled).then(|| err.message.clone()),
            ..Completion::default()
        };
        if let Err(history_err) = state.history.finish(&self.job_id, status, completion).await {
            tracing::error!(job = %self.job_id, error = %history_err, "could not record backup failure");
        }
        if cancelled {
            self.emit(BackupEvent::Cancelled { job_id: self.job_id.clone() });
        } else {
            tracing::warn!(job = %self.job_id, code = %err.code, error = %err.message, "backup failed");
            self.emit(BackupEvent::Failed { job_id: self.job_id.clone(), code: err.code, message: err.message });
            self.notify("Backup fallido", &self.instance.name);
        }
    }

    fn notify(&self, title: &str, body: &str) {
        if !self.state().notifications {
            return;
        }
        if let Err(err) = self.app.notification().builder().title(title).body(body).show() {
            tracing::debug!(error = %err, "desktop notification failed");
        }
    }
}

pub fn credentials(instance: &InstanceRecord) -> Option<Credentials> {
    instance.secret.as_ref().filter(|s| !s.is_empty()).map(|secret| Credentials {
        login: instance.login.clone(),
        secret: secret.to_secret_string(),
        kind: instance.secret_kind,
    })
}

async fn module_available(rpc: &dyn OdooRpc, cancel: &CancellationToken) -> bool {
    let call = rpc.call("appex.backup.api", "get_info", &[], Map::new());
    match cancellable(cancel, call).await {
        Ok(Ok(info)) => info.get("api_version").and_then(Value::as_u64) == Some(1),
        Ok(Err(err)) => {
            tracing::debug!(code = err.code(), "appex_backup module not available");
            false
        }
        Err(_) => false,
    }
}

async fn cancellable<F: std::future::Future>(cancel: &CancellationToken, future: F) -> CommandResult<F::Output> {
    tokio::select! {
        output = future => Ok(output),
        () = cancel.cancelled() => Err(CommandError::new("cancelled", "backup cancelled")),
    }
}

pub fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 { format!("{bytes} B") } else { format!("{value:.1} {}", UNITS[unit]) }
}

pub fn backup_exists(path: &Path) -> bool {
    path.is_file()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    #[test]
    fn base_url_is_normalized() {
        assert_eq!(parse_base_url("https://cliente1.nube.com").unwrap().as_str(), "https://cliente1.nube.com/");
        assert_eq!(parse_base_url(" https://c.nube.com/web?debug=1#x ").unwrap().as_str(), "https://c.nube.com/");
        assert_eq!(parse_base_url("http://10.0.0.5:8069/odoo/").unwrap().as_str(), "http://10.0.0.5:8069/odoo/");
        assert_eq!(parse_base_url("https://u:p@c.nube.com").unwrap().as_str(), "https://c.nube.com/");
        assert_eq!(parse_base_url("ftp://c.nube.com").unwrap_err().code, "invalid_url");
        assert_eq!(parse_base_url("cliente1.nube.com").unwrap_err().code, "invalid_url");
    }

    #[test]
    fn file_stem_matches_odoo_format() {
        let now = Utc.with_ymd_and_hms(2026, 9, 16, 10, 30, 5).unwrap();
        assert_eq!(file_stem("cliente1", now), "cliente1_2026-09-16_10-30-05");
        assert_eq!(file_stem("a b/c", now), "a_b_c_2026-09-16_10-30-05");
    }

    #[test]
    fn events_serialize_to_contract() {
        let event = BackupEvent::Progress {
            job_id: "j".into(),
            stage: BackupStage::ServerPreparing,
            elapsed_secs: Some(3),
            received: None,
            total: None,
            sent: None,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"type": "progress", "jobId": "j", "stage": "server_preparing", "elapsedSecs": 3})
        );
        let event = BackupEvent::Progress {
            job_id: "j".into(),
            stage: BackupStage::Downloading,
            elapsed_secs: None,
            received: Some(10),
            total: Some(None),
            sent: None,
        };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"type": "progress", "jobId": "j", "stage": "downloading", "received": 10, "total": null})
        );
        let event =
            BackupEvent::Started { job_id: "j".into(), instance_id: "i".into(), transport: TransportKind::AppexModule };
        assert_eq!(
            serde_json::to_value(&event).unwrap(),
            serde_json::json!({"type": "started", "jobId": "j", "instanceId": "i", "transport": "appex_module"})
        );
    }

    #[test]
    fn sizes_are_human_readable() {
        assert_eq!(human_size(512), "512 B");
        assert_eq!(human_size(1536), "1.5 KB");
        assert_eq!(human_size(5 * 1024 * 1024 * 1024), "5.0 GB");
    }
}
