//! Resumable upload protocol (`uploadType=resumable`).
//!
//! The server-confirmed offset (`Range` header of a `308`) is the source of truth.
//! After a transient failure the session status is queried with
//! `Content-Range: bytes */<total>`. An expired session (404/410) is restarted once.

use std::io::SeekFrom;
use std::path::Path;

use md5::{Digest, Md5};
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, CONTENT_TYPE, LOCATION, RANGE};
use reqwest::{Response, StatusCode};
use secrecy::ExposeSecret;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncSeekExt};
use tokio_util::sync::CancellationToken;
use url::Url;

use super::api::{DriveFile, FILE_FIELDS, endpoint, error_from_response};
use super::{CHUNK_ALIGNMENT, GoogleDriveAdapter};
use crate::util::{hex, map_reqwest_error};
use crate::{RemoteObject, Result, StorageError, TargetId, UploadMeta, UploadProgressFn};

/// How many times an expired upload session may be recreated.
const MAX_SESSION_RESTARTS: u32 = 1;

enum ChunkOutcome {
    /// Upload incomplete; next byte the server expects.
    Incomplete(u64),
    Complete(Box<DriveFile>),
    SessionExpired,
    Unauthorized,
}

pub(super) async fn resumable_upload(
    adapter: &GoogleDriveAdapter,
    target: &TargetId,
    file: &Path,
    meta: &UploadMeta,
    progress: UploadProgressFn,
    cancel: CancellationToken,
) -> Result<RemoteObject> {
    let chunk_size = adapter.options.chunk_size;
    if chunk_size == 0 || !chunk_size.is_multiple_of(CHUNK_ALIGNMENT) {
        return Err(StorageError::Fatal(format!("chunk_size {chunk_size} must be a non-zero multiple of 256 KiB")));
    }
    let file_name = file
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| StorageError::Fatal(format!("invalid file name: {}", file.display())))?
        .to_owned();
    let total = tokio::fs::metadata(file).await?.len();
    if total == 0 {
        return Err(StorageError::Fatal("refusing to upload an empty file".into()));
    }

    let mut reader = tokio::fs::File::open(file).await?;
    let mut buf = vec![0u8; chunk_size];
    let mut md5 = Md5::new();
    let mut hashed_until = 0u64;
    let mut restarts = 0;

    'session: loop {
        if cancel.is_cancelled() {
            return Err(StorageError::Cancelled);
        }
        let session = create_session(adapter, target, &file_name, total, meta).await?;
        let mut offset = 0u64;
        let mut failures = 0u32;
        let mut unauthorized = 0u32;
        let mut query_status = false;

        loop {
            if cancel.is_cancelled() {
                return Err(StorageError::Cancelled);
            }

            let attempt = if query_status {
                with_cancel(&cancel, send_status_query(adapter, &session, total)).await
            } else {
                let len = (total - offset).min(chunk_size as u64) as usize;
                reader.seek(SeekFrom::Start(offset)).await?;
                reader.read_exact(&mut buf[..len]).await?;
                let chunk_end = offset + len as u64;
                if chunk_end > hashed_until {
                    // Only bytes not hashed yet: re-sent ranges must not be counted twice.
                    md5.update(&buf[(hashed_until - offset) as usize..len]);
                    hashed_until = chunk_end;
                }
                with_cancel(&cancel, send_chunk(adapter, &session, &buf[..len], offset, total)).await
            };

            match attempt {
                Ok(ChunkOutcome::Incomplete(next)) => {
                    if next > total {
                        return Err(StorageError::Fatal(format!("server confirmed {next} bytes of {total}")));
                    }
                    // Only real progress resets the failure budget, so a chunk that keeps
                    // failing while status queries succeed still ends the upload.
                    if next > offset {
                        failures = 0;
                    }
                    unauthorized = 0;
                    offset = next;
                    query_status = false;
                    progress(offset, total);
                }
                Ok(ChunkOutcome::Complete(created)) => {
                    progress(total, total);
                    let local_md5 = hex(&md5.finalize_reset());
                    return finish_upload(adapter, *created, &local_md5).await;
                }
                Ok(ChunkOutcome::SessionExpired) => {
                    if restarts >= MAX_SESSION_RESTARTS {
                        return Err(StorageError::Fatal("upload session expired repeatedly".into()));
                    }
                    restarts += 1;
                    tracing::warn!("Google Drive upload session expired, starting a new one");
                    continue 'session;
                }
                Ok(ChunkOutcome::Unauthorized) => {
                    unauthorized += 1;
                    if unauthorized > 1 {
                        return Err(StorageError::AuthExpired);
                    }
                    adapter.invalidate_token().await;
                    query_status = true;
                }
                Err(err) if err.is_retryable() => {
                    failures += 1;
                    if failures >= adapter.retry.attempts() {
                        return Err(err);
                    }
                    let retry_after = match &err {
                        StorageError::RateLimited { retry_after } => *retry_after,
                        _ => None,
                    };
                    let delay = adapter.retry.delay(failures, retry_after);
                    tracing::debug!(error = %err, ?delay, offset, "retrying Google Drive upload chunk");
                    tokio::select! {
                        () = tokio::time::sleep(delay) => {}
                        () = cancel.cancelled() => return Err(StorageError::Cancelled),
                    }
                    query_status = true;
                }
                Err(err) => return Err(err),
            }
        }
    }
}

async fn with_cancel<T>(cancel: &CancellationToken, fut: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {
        result = fut => result,
        () = cancel.cancelled() => Err(StorageError::Cancelled),
    }
}

async fn create_session(
    adapter: &GoogleDriveAdapter,
    target: &TargetId,
    file_name: &str,
    total: u64,
    meta: &UploadMeta,
) -> Result<Url> {
    let mut url = endpoint(&adapter.endpoints.upload_api, &["files"]);
    url.query_pairs_mut()
        .append_pair("uploadType", "resumable")
        .append_pair("supportsAllDrives", "true")
        .append_pair("fields", FILE_FIELDS);
    let body = json!({
        "name": file_name,
        "parents": [target.0],
        "mimeType": "application/zip",
        "appProperties": {
            "obdBackup": "file",
            "instanceId": meta.instance_id,
            "database": meta.database,
            "sha256": meta.sha256,
        },
    });

    let response = adapter
        .send(|http| {
            http.post(url.clone())
                .header("X-Upload-Content-Type", "application/zip")
                .header("X-Upload-Content-Length", total.to_string())
                .json(&body)
        })
        .await?;
    let location = response
        .headers()
        .get(LOCATION)
        .and_then(|v| v.to_str().ok())
        .ok_or_else(|| StorageError::Fatal("resumable session response without Location header".into()))?;
    Url::parse(location).map_err(|e| StorageError::Fatal(format!("invalid upload session URI: {e}")))
}

async fn send_chunk(
    adapter: &GoogleDriveAdapter,
    session: &Url,
    chunk: &[u8],
    offset: u64,
    total: u64,
) -> Result<ChunkOutcome> {
    let token = adapter.access_token().await?;
    let end = offset + chunk.len() as u64 - 1;
    let response = adapter
        .http
        .put(session.clone())
        .bearer_auth(token.expose_secret())
        .header(CONTENT_RANGE, format!("bytes {offset}-{end}/{total}"))
        .header(CONTENT_TYPE, "application/zip")
        .body(chunk.to_vec())
        .send()
        .await
        .map_err(map_reqwest_error)?;
    interpret(response).await
}

async fn send_status_query(adapter: &GoogleDriveAdapter, session: &Url, total: u64) -> Result<ChunkOutcome> {
    let token = adapter.access_token().await?;
    let response = adapter
        .http
        .put(session.clone())
        .bearer_auth(token.expose_secret())
        .header(CONTENT_RANGE, format!("bytes */{total}"))
        .header(CONTENT_LENGTH, "0")
        .body(Vec::new())
        .send()
        .await
        .map_err(map_reqwest_error)?;
    interpret(response).await
}

async fn interpret(response: Response) -> Result<ChunkOutcome> {
    match response.status() {
        StatusCode::PERMANENT_REDIRECT => Ok(ChunkOutcome::Incomplete(confirmed_offset(&response)?)),
        StatusCode::OK | StatusCode::CREATED => {
            let file: DriveFile = response
                .json()
                .await
                .map_err(|e| StorageError::Fatal(format!("invalid upload completion response: {e}")))?;
            Ok(ChunkOutcome::Complete(Box::new(file)))
        }
        StatusCode::NOT_FOUND | StatusCode::GONE => Ok(ChunkOutcome::SessionExpired),
        StatusCode::UNAUTHORIZED => Ok(ChunkOutcome::Unauthorized),
        _ => Err(error_from_response(response).await),
    }
}

/// `Range: bytes=0-N` → `N + 1`; no header → nothing persisted yet.
fn confirmed_offset(response: &Response) -> Result<u64> {
    let Some(range) = response.headers().get(RANGE) else {
        return Ok(0);
    };
    let text = range.to_str().map_err(|_| StorageError::Fatal("invalid Range header".into()))?;
    let last = text
        .trim()
        .strip_prefix("bytes=")
        .and_then(|r| r.split_once('-'))
        .and_then(|(_, end)| end.trim().parse::<u64>().ok())
        .ok_or_else(|| StorageError::Fatal(format!("invalid Range header: {text}")))?;
    Ok(last + 1)
}

async fn finish_upload(adapter: &GoogleDriveAdapter, created: DriveFile, local_md5: &str) -> Result<RemoteObject> {
    match created.md5_checksum.clone() {
        Some(remote) if !remote.eq_ignore_ascii_case(local_md5) => {
            let object = created.into_remote();
            tracing::error!(name = %object.name, "Google Drive checksum mismatch; removing the corrupted upload");
            let url = endpoint(&adapter.endpoints.drive_api, &["files", &object.id]);
            if let Err(err) =
                adapter.send(|http| http.delete(url.clone()).query(&[("supportsAllDrives", "true")])).await
            {
                tracing::warn!(error = %err, "could not delete corrupted upload");
            }
            Err(StorageError::Fatal(format!(
                "checksum mismatch for {}: local md5 {local_md5}, Drive md5 {remote}",
                object.name
            )))
        }
        Some(_) => Ok(created.into_remote()),
        None => {
            tracing::warn!("Google Drive did not return md5Checksum; skipping checksum verification");
            Ok(created.into_remote())
        }
    }
}
