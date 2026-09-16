//! Backup transports.
//!
//! - [`DbManagerTransport`]: `POST /web/database/backup` (needs `list_db = True` and the
//!   master password). The server builds the whole zip before the first byte arrives.
//! - [`AppexModuleTransport`]: `appex_backup` module API (see `docs/appex-backup-module-api.md`).
//!   Works with `list_db = False`, uses an API key, resumable download.
//!
//! Both write `<dest_dir>/<file_stem>.zip.part`, hash it (SHA-256) while streaming,
//! validate it with [`crate::validate_backup_zip`] and rename it to `.zip`. On error
//! or cancellation the `.part` file is removed.
//!
//! The `reqwest::Client` passed in must not have a global request timeout: the
//! database manager only answers once the whole zip is ready.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_RANGE, CONTENT_TYPE, RANGE};
use reqwest::{Client, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncSeekExt, AsyncWriteExt, BufWriter};
use tokio_util::sync::CancellationToken;
use url::Url;

use crate::json2::DATABASE_HEADER;
use crate::net::{cancellable, endpoint, map_reqwest, read_text_limited, to_hex};
use crate::probe::{MODULE_API_MODEL, MODULE_API_VERSION};
use crate::rpc::OdooRpc;
use crate::validate::validate_backup_zip_cancellable;
use crate::{BackupManifest, OdooError, OdooVersion, Result};

/// A download that stops sending bytes for this long is considered dead.
const STALL_TIMEOUT: Duration = Duration::from_secs(300);
/// Minimum interval between `Downloading` progress events.
const PROGRESS_INTERVAL: Duration = Duration::from_millis(200);
const WRITE_BUFFER: usize = 1024 * 1024;
const ZIP_MAGIC: &[u8; 4] = b"PK\x03\x04";
/// Download retries of the module transport (after the first attempt).
const MAX_DOWNLOAD_RETRIES: u32 = 5;
const RPC_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransportKind {
    DbManager,
    AppexModule,
}

#[derive(Debug, Clone)]
pub struct BackupRequest {
    pub database: String,
    pub include_filestore: bool,
    pub dest_dir: PathBuf,
    /// File name without extension, e.g. `cliente1_2026-09-16_10-30-00`.
    pub file_stem: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case")]
pub enum BackupPhase {
    Requesting,
    ServerPreparing { elapsed_secs: u64 },
    Downloading { received: u64, total: Option<u64> },
    Validating,
}

pub type ProgressFn = Arc<dyn Fn(BackupPhase) + Send + Sync>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadedBackup {
    pub path: PathBuf,
    pub size: u64,
    pub sha256: String,
    pub manifest: BackupManifest,
}

#[async_trait]
pub trait BackupTransport: Send + Sync {
    fn kind(&self) -> TransportKind;
    async fn run(
        &self,
        request: &BackupRequest,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> Result<DownloadedBackup>;
}

// ---------------------------------------------------------------------------
// Database manager
// ---------------------------------------------------------------------------

pub struct DbManagerTransport {
    http: Client,
    base_url: Url,
    master_password: SecretString,
    version: OdooVersion,
    prepare_timeout: Duration,
}

impl DbManagerTransport {
    /// `prepare_timeout`: maximum time to wait for the response headers while the
    /// server runs pg_dump and zips the filestore.
    pub fn new(
        http: Client,
        base_url: Url,
        master_password: SecretString,
        version: OdooVersion,
        prepare_timeout: Duration,
    ) -> Self {
        Self { http, base_url, master_password, version, prepare_timeout }
    }

    fn form(&self, request: &BackupRequest) -> Vec<(&'static str, String)> {
        let mut form = vec![
            ("master_pwd", self.master_password.expose_secret().to_owned()),
            ("name", request.database.clone()),
            ("backup_format", "zip".to_owned()),
        ];
        // Only Odoo ≥ 19 knows `filestore`; older versions log a warning for unknown args.
        if self.version.major >= 19 {
            form.push(("filestore", request.include_filestore.to_string()));
        }
        form
    }
}

#[async_trait]
impl BackupTransport for DbManagerTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::DbManager
    }

    async fn run(
        &self,
        request: &BackupRequest,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> Result<DownloadedBackup> {
        progress(BackupPhase::Requesting);
        let url = endpoint(&self.base_url, "web/database/backup")?;
        tokio::fs::create_dir_all(&request.dest_dir).await?;

        let send = self.http.post(url).form(&self.form(request)).send();
        let response = wait_for_headers(send, self.prepare_timeout, &progress, &cancel).await?;

        let status = response.status();
        let content_type =
            response.headers().get(CONTENT_TYPE).and_then(|v| v.to_str().ok()).unwrap_or_default().to_ascii_lowercase();
        if !status.is_success()
            || !(content_type.starts_with("application/octet-stream") || content_type.starts_with("application/zip"))
        {
            let body = read_text_limited(response, 256 * 1024).await;
            return Err(db_manager_error(status, &body));
        }

        let total = response.content_length();
        let mut part = PartFile::create(&request.dest_dir, &request.file_stem).await?;
        let mut hasher = Sha256::new();
        let mut head = Vec::with_capacity(4);
        let mut meter = ProgressMeter::new(total);
        let mut stream = response.bytes_stream();

        loop {
            let next = cancellable(&cancel, tokio::time::timeout(STALL_TIMEOUT, stream.next())).await?;
            let chunk = match next {
                Err(_) => return Err(OdooError::Timeout),
                Ok(None) => break,
                Ok(Some(chunk)) => chunk.map_err(map_reqwest)?,
            };
            if head.len() < ZIP_MAGIC.len() {
                let needed = ZIP_MAGIC.len() - head.len();
                head.extend_from_slice(&chunk[..chunk.len().min(needed)]);
                if head.len() == ZIP_MAGIC.len() && head != ZIP_MAGIC {
                    return Err(OdooError::InvalidBackup("the server response is not a zip file".into()));
                }
            }
            hasher.update(&chunk);
            part.writer.write_all(&chunk).await?;
            meter.add(chunk.len() as u64, &progress);
        }
        if head.len() < ZIP_MAGIC.len() {
            return Err(OdooError::InvalidBackup("the server returned an empty backup".into()));
        }
        meter.finish(&progress);
        part.flush().await?;

        finalize(part, request, meter.received, to_hex(&hasher.finalize()), &progress, &cancel).await
    }
}

/// Awaits the response headers, emitting `ServerPreparing` every second.
async fn wait_for_headers(
    send: impl Future<Output = reqwest::Result<Response>>,
    prepare_timeout: Duration,
    progress: &ProgressFn,
    cancel: &CancellationToken,
) -> Result<Response> {
    let started = Instant::now();
    let deadline = tokio::time::sleep(prepare_timeout);
    let mut ticker = tokio::time::interval(Duration::from_secs(1));
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    tokio::pin!(send, deadline);

    loop {
        tokio::select! {
            biased;
            () = cancel.cancelled() => return Err(OdooError::Cancelled),
            result = &mut send => return result.map_err(map_reqwest),
            () = &mut deadline => return Err(OdooError::PrepareTimeout),
            _ = ticker.tick() => progress(BackupPhase::ServerPreparing { elapsed_secs: started.elapsed().as_secs() }),
        }
    }
}

/// Turns the HTML page Odoo renders on backup errors into an error.
fn db_manager_error(status: StatusCode, body: &str) -> OdooError {
    let lower = body.to_ascii_lowercase();
    if lower.contains("database manager has been disabled") {
        return OdooError::DatabaseManagerDisabled;
    }
    match extract_alert_message(body) {
        Some(message) => {
            let detail = message.strip_prefix("Database backup error:").map_or(message.as_str(), str::trim);
            if detail.eq_ignore_ascii_case("access denied") || detail.contains("AccessDenied") {
                OdooError::AccessDenied("invalid master password or database management disabled".into())
            } else {
                OdooError::ServerBackupError(detail.to_owned())
            }
        }
        None if !status.is_success() => {
            OdooError::HttpStatus { status: status.as_u16(), message: html_to_text(body).chars().take(200).collect() }
        }
        None => OdooError::ServerBackupError("the server did not return a backup file".into()),
    }
}

/// Text of the first `alert-danger` element of Odoo's database manager page.
fn extract_alert_message(html: &str) -> Option<String> {
    let mut rest = html;
    while let Some(pos) = rest.find("alert-danger") {
        rest = &rest[pos..];
        let start = rest.find('>')? + 1;
        let end = rest[start..].find("</div>")? + start;
        let text = html_to_text(&rest[start..end]);
        if !text.is_empty() {
            return Some(text);
        }
        rest = &rest[end..];
    }
    None
}

fn html_to_text(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    decode_entities(&text).split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Decodes the entities Odoo's templates emit (`&amp;`, `&#34;`, `&#x27;`, …).
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let candidate = &rest[amp + 1..];
        let decoded = candidate.find(';').filter(|end| *end <= 10).and_then(|end| {
            let name = &candidate[..end];
            let ch = match name {
                "amp" => Some('&'),
                "lt" => Some('<'),
                "gt" => Some('>'),
                "quot" => Some('"'),
                "apos" => Some('\''),
                "nbsp" => Some(' '),
                _ => name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .map(|hex| u32::from_str_radix(hex, 16).ok())
                    .unwrap_or_else(|| name.strip_prefix('#').and_then(|dec| dec.parse().ok()))
                    .and_then(char::from_u32),
            };
            ch.map(|ch| (ch, end))
        });
        match decoded {
            Some((ch, end)) => {
                out.push(ch);
                rest = &candidate[end + 1..];
            }
            None => {
                out.push('&');
                rest = candidate;
            }
        }
    }
    out.push_str(rest);
    out
}

// ---------------------------------------------------------------------------
// appex_backup module
// ---------------------------------------------------------------------------

pub struct AppexModuleTransport {
    rpc: Arc<dyn OdooRpc>,
    http: Client,
    base_url: Url,
    api_key: SecretString,
    poll_interval: Duration,
    prepare_timeout: Duration,
    retry_base: Duration,
}

impl AppexModuleTransport {
    pub fn new(
        rpc: Arc<dyn OdooRpc>,
        http: Client,
        base_url: Url,
        api_key: SecretString,
        poll_interval: Duration,
        prepare_timeout: Duration,
    ) -> Self {
        Self { rpc, http, base_url, api_key, poll_interval, prepare_timeout, retry_base: Duration::from_secs(1) }
    }

    /// First download retry delay (doubles on each retry). Default: 1 s.
    pub fn with_retry_base(mut self, retry_base: Duration) -> Self {
        self.retry_base = retry_base;
        self
    }

    async fn rpc_call(&self, method: &str, kwargs: Map<String, Value>, cancel: &CancellationToken) -> Result<Value> {
        let call = tokio::time::timeout(RPC_TIMEOUT, self.rpc.call(MODULE_API_MODEL, method, &[], kwargs));
        match cancellable(cancel, call).await? {
            Err(_) => Err(OdooError::Timeout),
            Ok(Err(err)) if err.is_model_not_found() => Err(OdooError::ModuleNotInstalled),
            Ok(result) => result,
        }
    }

    async fn wait_for_job(&self, job_id: &str, progress: &ProgressFn, cancel: &CancellationToken) -> Result<ModuleJob> {
        let started = Instant::now();
        let mut ticker = tokio::time::interval(Duration::from_secs(1));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let mut next_poll = tokio::time::Instant::now();

        loop {
            if started.elapsed() >= self.prepare_timeout {
                return Err(OdooError::PrepareTimeout);
            }
            tokio::select! {
                biased;
                () = cancel.cancelled() => return Err(OdooError::Cancelled),
                () = tokio::time::sleep_until(next_poll) => {
                    let job = self.get_job(job_id, cancel).await?;
                    match job.state.as_str() {
                        "done" => return Ok(job),
                        "failed" => {
                            return Err(OdooError::ServerBackupError(job.error.unwrap_or_else(|| "backup job failed".into())));
                        }
                        "expired" => return Err(OdooError::ServerBackupError("backup job expired".into())),
                        "pending" | "running" => {}
                        other => return Err(OdooError::Protocol(format!("unknown backup job state {other:?}"))),
                    }
                    next_poll = tokio::time::Instant::now() + self.poll_interval;
                }
                _ = ticker.tick() => progress(BackupPhase::ServerPreparing { elapsed_secs: started.elapsed().as_secs() }),
            }
        }
    }

    async fn get_job(&self, job_id: &str, cancel: &CancellationToken) -> Result<ModuleJob> {
        let value = self.rpc_call("get_job", kwargs(json!({"job_id": job_id})), cancel).await?;
        serde_json::from_value(value).map_err(|err| OdooError::Protocol(format!("invalid get_job result: {err}")))
    }

    async fn discard(&self, job_id: &str) {
        let call = self.rpc.call(MODULE_API_MODEL, "discard_job", &[], kwargs(json!({"job_id": job_id})));
        match tokio::time::timeout(Duration::from_secs(15), call).await {
            Ok(Ok(_)) => {}
            Ok(Err(err)) => tracing::debug!(error = %err, "discard_job failed"),
            Err(_) => tracing::debug!("discard_job timed out"),
        }
    }

    async fn download(
        &self,
        job_id: &str,
        expected_size: u64,
        part: &mut PartFile,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> Result<(u64, String)> {
        let url = endpoint(&self.base_url, &format!("appex_backup/download/{job_id}"))?;
        let mut state =
            DownloadState { received: 0, hasher: Sha256::new(), meter: ProgressMeter::new(Some(expected_size)) };
        let mut retries = 0;

        loop {
            match self.download_attempt(&url, expected_size, part, &mut state, progress, cancel).await {
                Ok(()) => break,
                Err(Attempt::Fatal(err)) => return Err(err),
                Err(Attempt::Retry(err)) => {
                    retries += 1;
                    if retries > MAX_DOWNLOAD_RETRIES {
                        return Err(err);
                    }
                    let delay = self.retry_base * 2u32.saturating_pow(retries - 1);
                    tracing::warn!(error = %err, retries, ?delay, "backup download interrupted, retrying");
                    cancellable(cancel, tokio::time::sleep(delay)).await?;
                }
            }
        }
        state.meter.finish(progress);
        Ok((state.received, to_hex(&state.hasher.finalize())))
    }

    async fn download_attempt(
        &self,
        url: &Url,
        expected_size: u64,
        part: &mut PartFile,
        state: &mut DownloadState,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> std::result::Result<(), Attempt> {
        let mut request = self.http.get(url.clone()).bearer_auth(self.api_key.expose_secret());
        let database = self.rpc.database();
        if !database.is_empty() {
            request = request.header(DATABASE_HEADER, database);
        }
        if state.received > 0 {
            request = request.header(RANGE, format!("bytes={}-", state.received));
        }

        let response = match cancellable(cancel, request.send()).await.map_err(Attempt::Fatal)? {
            Ok(response) => response,
            Err(err) => return Err(Attempt::Retry(map_reqwest(err))),
        };

        match response.status() {
            StatusCode::OK => {
                if state.received > 0 {
                    // The server ignored the Range header: start over.
                    state.reset(part).await.map_err(Attempt::Fatal)?;
                }
            }
            StatusCode::PARTIAL_CONTENT => {
                let start =
                    response.headers().get(CONTENT_RANGE).and_then(|v| v.to_str().ok()).and_then(content_range_start);
                if start != Some(state.received) {
                    state.reset(part).await.map_err(Attempt::Fatal)?;
                    return Err(Attempt::Retry(OdooError::Protocol("unexpected Content-Range, restarting".into())));
                }
            }
            StatusCode::RANGE_NOT_SATISFIABLE => {
                state.reset(part).await.map_err(Attempt::Fatal)?;
                return Err(Attempt::Retry(OdooError::Protocol("range not satisfiable, restarting".into())));
            }
            StatusCode::UNAUTHORIZED => return Err(Attempt::Fatal(OdooError::AuthenticationFailed)),
            StatusCode::FORBIDDEN => {
                return Err(Attempt::Fatal(OdooError::AccessDenied("the API key user cannot download backups".into())));
            }
            StatusCode::NOT_FOUND => {
                return Err(Attempt::Fatal(OdooError::ServerBackupError("backup job not found".into())));
            }
            StatusCode::CONFLICT => return Err(Attempt::Retry(OdooError::Protocol("backup is not ready yet".into()))),
            StatusCode::GONE => return Err(Attempt::Fatal(OdooError::ServerBackupError("backup job expired".into()))),
            status if status.is_server_error() => {
                return Err(Attempt::Retry(OdooError::HttpStatus {
                    status: status.as_u16(),
                    message: "server error".into(),
                }));
            }
            status => {
                let body = read_text_limited(response, 1024).await;
                return Err(Attempt::Fatal(OdooError::HttpStatus { status: status.as_u16(), message: body }));
            }
        }

        let mut stream = response.bytes_stream();
        loop {
            let next = cancellable(cancel, tokio::time::timeout(STALL_TIMEOUT, stream.next()))
                .await
                .map_err(Attempt::Fatal)?;
            let chunk = match next {
                Err(_) => {
                    part.flush().await.map_err(Attempt::Fatal)?;
                    return Err(Attempt::Retry(OdooError::Timeout));
                }
                Ok(None) => break,
                Ok(Some(Ok(chunk))) => chunk,
                Ok(Some(Err(err))) => {
                    part.flush().await.map_err(Attempt::Fatal)?;
                    return Err(Attempt::Retry(map_reqwest(err)));
                }
            };
            if state.received + chunk.len() as u64 > expected_size {
                return Err(Attempt::Fatal(OdooError::InvalidBackup("download is larger than announced".into())));
            }
            state.hasher.update(&chunk);
            part.writer.write_all(&chunk).await.map_err(|err| Attempt::Fatal(err.into()))?;
            state.received += chunk.len() as u64;
            state.meter.add(chunk.len() as u64, progress);
        }
        part.flush().await.map_err(Attempt::Fatal)?;

        if state.received < expected_size {
            return Err(Attempt::Retry(OdooError::Protocol(format!(
                "download ended at {} of {expected_size} bytes",
                state.received
            ))));
        }
        Ok(())
    }
}

#[async_trait]
impl BackupTransport for AppexModuleTransport {
    fn kind(&self) -> TransportKind {
        TransportKind::AppexModule
    }

    async fn run(
        &self,
        request: &BackupRequest,
        progress: ProgressFn,
        cancel: CancellationToken,
    ) -> Result<DownloadedBackup> {
        progress(BackupPhase::Requesting);

        let info = self.rpc_call("get_info", Map::new(), &cancel).await?;
        let api_version = info.get("api_version").and_then(Value::as_u64).unwrap_or(0);
        if api_version != u64::from(MODULE_API_VERSION) {
            return Err(OdooError::ModuleApiIncompatible(u32::try_from(api_version).unwrap_or(u32::MAX)));
        }
        tokio::fs::create_dir_all(&request.dest_dir).await?;

        let requested = self
            .rpc_call("request_backup", kwargs(json!({"include_filestore": request.include_filestore})), &cancel)
            .await?;
        let job_id = requested
            .get("job_id")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| OdooError::Protocol(format!("invalid request_backup result: {requested}")))?
            .to_owned();

        let result = self.run_job(request, &job_id, &progress, &cancel).await;
        // Best effort, also after errors and cancellation: free the server-side file.
        self.discard(&job_id).await;
        result
    }
}

impl AppexModuleTransport {
    async fn run_job(
        &self,
        request: &BackupRequest,
        job_id: &str,
        progress: &ProgressFn,
        cancel: &CancellationToken,
    ) -> Result<DownloadedBackup> {
        let job = self.wait_for_job(job_id, progress, cancel).await?;
        let expected_size = job.size.ok_or_else(|| OdooError::Protocol("finished job without size".into()))?;
        let expected_sha =
            job.sha256.clone().ok_or_else(|| OdooError::Protocol("finished job without sha256".into()))?;

        let mut part = PartFile::create(&request.dest_dir, &request.file_stem).await?;
        let (size, sha256) = self.download(job_id, expected_size, &mut part, progress, cancel).await?;
        if size != expected_size {
            return Err(OdooError::InvalidBackup(format!("size mismatch: got {size} bytes, expected {expected_size}")));
        }
        if !sha256.eq_ignore_ascii_case(expected_sha.trim()) {
            return Err(OdooError::InvalidBackup("sha256 mismatch".into()));
        }
        finalize(part, request, size, sha256, progress, cancel).await
    }
}

#[derive(Debug, Deserialize)]
struct ModuleJob {
    state: String,
    size: Option<u64>,
    sha256: Option<String>,
    error: Option<String>,
}

enum Attempt {
    Retry(OdooError),
    Fatal(OdooError),
}

struct DownloadState {
    received: u64,
    hasher: Sha256,
    meter: ProgressMeter,
}

impl DownloadState {
    async fn reset(&mut self, part: &mut PartFile) -> Result<()> {
        part.truncate().await?;
        self.received = 0;
        self.hasher = Sha256::new();
        self.meter.received = 0;
        Ok(())
    }
}

/// `bytes 100-199/200` → `Some(100)`.
fn content_range_start(value: &str) -> Option<u64> {
    let range = value.trim().strip_prefix("bytes")?.trim();
    range.split('-').next()?.trim().parse().ok()
}

fn kwargs(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
}

// ---------------------------------------------------------------------------
// Shared file handling
// ---------------------------------------------------------------------------

/// `.zip.part` file removed on drop unless committed.
struct PartFile {
    path: PathBuf,
    writer: BufWriter<tokio::fs::File>,
    committed: bool,
}

impl PartFile {
    async fn create(dest_dir: &Path, file_stem: &str) -> Result<Self> {
        let path = dest_dir.join(format!("{file_stem}.zip.part"));
        let file = tokio::fs::File::create(&path).await?;
        Ok(Self { path, writer: BufWriter::with_capacity(WRITE_BUFFER, file), committed: false })
    }

    async fn flush(&mut self) -> Result<()> {
        self.writer.flush().await?;
        Ok(())
    }

    async fn truncate(&mut self) -> Result<()> {
        self.writer.flush().await?;
        let file = self.writer.get_mut();
        file.set_len(0).await?;
        file.seek(std::io::SeekFrom::Start(0)).await?;
        Ok(())
    }
}

impl Drop for PartFile {
    fn drop(&mut self) {
        if !self.committed {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}

/// Throttled `Downloading` events.
struct ProgressMeter {
    received: u64,
    total: Option<u64>,
    last_emit: Option<Instant>,
}

impl ProgressMeter {
    fn new(total: Option<u64>) -> Self {
        Self { received: 0, total, last_emit: None }
    }

    fn add(&mut self, bytes: u64, progress: &ProgressFn) {
        self.received += bytes;
        if self.last_emit.is_none_or(|at| at.elapsed() >= PROGRESS_INTERVAL) {
            self.last_emit = Some(Instant::now());
            progress(BackupPhase::Downloading { received: self.received, total: self.total });
        }
    }

    fn finish(&mut self, progress: &ProgressFn) {
        progress(BackupPhase::Downloading { received: self.received, total: self.total.or(Some(self.received)) });
    }
}

/// Syncs, validates and renames the part file to its final unique name.
async fn finalize(
    mut part: PartFile,
    request: &BackupRequest,
    size: u64,
    sha256: String,
    progress: &ProgressFn,
    cancel: &CancellationToken,
) -> Result<DownloadedBackup> {
    part.flush().await?;
    part.writer.get_mut().sync_all().await?;
    progress(BackupPhase::Validating);

    let abort = Arc::new(AtomicBool::new(false));
    let path = part.path.clone();
    let database = request.database.clone();
    let task_abort = Arc::clone(&abort);
    let mut validation =
        tokio::task::spawn_blocking(move || validate_backup_zip_cancellable(&path, Some(&database), &task_abort));

    let manifest = tokio::select! {
        biased;
        () = cancel.cancelled() => {
            abort.store(true, Ordering::Relaxed);
            // Wait for the blocking reader to close the file before it is removed.
            let _ = validation.await;
            return Err(OdooError::Cancelled);
        }
        joined = &mut validation => {
            joined.map_err(|err| OdooError::Protocol(format!("validation task failed: {err}")))??
        }
    };

    let final_path = unique_final_path(&request.dest_dir, &request.file_stem);
    tokio::fs::rename(&part.path, &final_path).await?;
    part.committed = true;

    Ok(DownloadedBackup { path: final_path, size, sha256, manifest })
}

fn unique_final_path(dir: &Path, stem: &str) -> PathBuf {
    let first = dir.join(format!("{stem}.zip"));
    if !first.exists() {
        return first;
    }
    (1u32..)
        .map(|n| dir.join(format!("{stem}-{n}.zip")))
        .find(|candidate| !candidate.exists())
        .expect("an unused file name exists")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_backup_error_messages() {
        let html = r#"<html><body><div class="container">
            <div class="alert alert-danger" role="alert">Database backup error: Access Denied</div>
        </div></body></html>"#;
        assert!(matches!(db_manager_error(StatusCode::OK, html), OdooError::AccessDenied(_)));

        let html =
            r#"<div class="alert alert-danger">Database backup error: Database &#39;nope&#39; is not known</div>"#;
        assert!(matches!(
            db_manager_error(StatusCode::OK, html),
            OdooError::ServerBackupError(m) if m == "Database 'nope' is not known"
        ));

        // Odoo 15 wraps long messages over several lines and uses numeric entities.
        let html = "<div class=\"alert alert-danger\">Database backup error: connection to server at &#34;db15&#34;\n  failed: FATAL:  database &#34;nope&#34; does not exist\n</div>";
        assert!(matches!(
            db_manager_error(StatusCode::OK, html),
            OdooError::ServerBackupError(m) if m == "connection to server at \"db15\" failed: FATAL: database \"nope\" does not exist"
        ));

        let html = r#"<div class="alert alert-danger text-center">The database manager has been disabled by the administrator</div>"#;
        assert!(matches!(db_manager_error(StatusCode::OK, html), OdooError::DatabaseManagerDisabled));

        assert!(matches!(
            db_manager_error(StatusCode::BAD_GATEWAY, "<h1>502</h1>"),
            OdooError::HttpStatus { status: 502, .. }
        ));
    }

    #[test]
    fn parses_content_range() {
        assert_eq!(content_range_start("bytes 100-199/200"), Some(100));
        assert_eq!(content_range_start("bytes */200"), None);
    }

    #[test]
    fn unique_names_do_not_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(unique_final_path(dir.path(), "db"), dir.path().join("db.zip"));
        std::fs::write(dir.path().join("db.zip"), b"x").unwrap();
        std::fs::write(dir.path().join("db-1.zip"), b"x").unwrap();
        assert_eq!(unique_final_path(dir.path(), "db"), dir.path().join("db-2.zip"));
    }
}
