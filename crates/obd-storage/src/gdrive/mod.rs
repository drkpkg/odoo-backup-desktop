//! Google Drive destination.
//!
//! - OAuth 2.0 for desktop apps: system browser + loopback redirect
//!   (`http://127.0.0.1:<random port>`) + PKCE (S256) + `state`.
//! - Scope `drive.file` only (non-sensitive; the app sees only files it created).
//! - Resumable uploads in chunks that are multiples of 256 KiB (default 8 MiB).
//! - Files/folders are tagged with `appProperties` (`obdBackup`, `instanceId`).

mod api;
mod oauth;
mod upload;

pub use crate::util::RetryPolicy;
pub use oauth::{AuthorizedAccount, OAuthClient, PendingAuthorization};

use std::collections::HashMap;
use std::path::Path;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, RequestBuilder, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;
use url::Url;

use self::api::{DriveFile, FOLDER_MIME, FileList, endpoint, error_from_response, fetch_about, refresh_access_token};
use crate::util::{escape_query, map_reqwest_error};
use crate::{
    Capabilities, RemoteObject, Result, StorageAdapter, StorageError, TargetId, TargetSpec, UploadMeta,
    UploadProgressFn,
};

pub const DRIVE_FILE_SCOPE: &str = "https://www.googleapis.com/auth/drive.file";
pub const DEFAULT_CHUNK_SIZE: usize = 8 * 1024 * 1024;
/// Resumable upload chunks must be multiples of this size (except the last one).
pub const CHUNK_ALIGNMENT: usize = 256 * 1024;

/// Refresh the access token this long before Google says it expires.
const TOKEN_EXPIRY_MARGIN: Duration = Duration::from_secs(60);
const LIST_PAGE_SIZE: &str = "100";

/// Google endpoints, overridable for tests.
#[derive(Debug, Clone)]
pub struct GoogleEndpoints {
    pub auth_url: Url,
    pub token_url: Url,
    pub revoke_url: Url,
    pub drive_api: Url,
    pub upload_api: Url,
}

impl Default for GoogleEndpoints {
    fn default() -> Self {
        Self {
            auth_url: Url::parse("https://accounts.google.com/o/oauth2/v2/auth").expect("static url"),
            token_url: Url::parse("https://oauth2.googleapis.com/token").expect("static url"),
            revoke_url: Url::parse("https://oauth2.googleapis.com/revoke").expect("static url"),
            drive_api: Url::parse("https://www.googleapis.com/drive/v3/").expect("static url"),
            upload_api: Url::parse("https://www.googleapis.com/upload/drive/v3/").expect("static url"),
        }
    }
}

impl GoogleEndpoints {
    /// All endpoints under one base URL (`<base>/token`, `<base>/drive/v3/`, …), for mocks.
    pub fn with_base(base: &Url) -> Self {
        let join = |path: &str| base.join(path).expect("valid mock endpoint");
        Self {
            auth_url: join("auth"),
            token_url: join("token"),
            revoke_url: join("revoke"),
            drive_api: join("drive/v3/"),
            upload_api: join("upload/drive/v3/"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DriveOptions {
    /// Top-level folder created in "My Drive" (or the shared drive root).
    pub root_folder_name: String,
    /// Upload into this shared drive instead of "My Drive".
    pub shared_drive_id: Option<String>,
    /// `true`: `files.delete`; `false`: move to trash (`trashed = true`).
    pub permanent_delete: bool,
    /// Must be a multiple of 256 KiB.
    pub chunk_size: usize,
}

impl Default for DriveOptions {
    fn default() -> Self {
        Self {
            root_folder_name: "Odoo Backup Desktop".into(),
            shared_drive_id: None,
            permanent_delete: false,
            chunk_size: DEFAULT_CHUNK_SIZE,
        }
    }
}

/// Google account behind the adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub email: Option<String>,
    pub display_name: Option<String>,
}

struct CachedToken {
    value: SecretString,
    expires_at: Instant,
}

pub struct GoogleDriveAdapter {
    http: Client,
    client: OAuthClient,
    refresh_token: SecretString,
    options: DriveOptions,
    endpoints: GoogleEndpoints,
    retry: RetryPolicy,
    token: Mutex<Option<CachedToken>>,
    /// Folder ids already resolved: `"root"` and `instance:<id>` keys.
    folders: Mutex<HashMap<String, String>>,
}

impl GoogleDriveAdapter {
    pub fn new(http: Client, client: OAuthClient, refresh_token: SecretString, options: DriveOptions) -> Self {
        Self::with_endpoints(http, client, refresh_token, options, GoogleEndpoints::default())
    }

    pub fn with_endpoints(
        http: Client,
        client: OAuthClient,
        refresh_token: SecretString,
        options: DriveOptions,
        endpoints: GoogleEndpoints,
    ) -> Self {
        Self {
            http,
            client,
            refresh_token,
            options,
            endpoints,
            retry: RetryPolicy::default(),
            token: Mutex::new(None),
            folders: Mutex::new(HashMap::new()),
        }
    }

    /// Replaces the retry policy used for retryable errors.
    pub fn with_retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn options(&self) -> &DriveOptions {
        &self.options
    }

    /// Email and display name of the connected account (`about.get`).
    pub async fn account_info(&self) -> Result<AccountInfo> {
        let mut unauthorized_retry = true;
        let mut failures = 0;
        loop {
            let token = self.access_token().await?;
            match fetch_about(&self.http, &self.endpoints.drive_api, token.expose_secret()).await {
                Ok((email, display_name)) => return Ok(AccountInfo { email, display_name }),
                Err(StorageError::AuthExpired) if unauthorized_retry => {
                    unauthorized_retry = false;
                    self.invalidate_token().await;
                }
                Err(err) if err.is_retryable() && failures + 1 < self.retry.attempts() => {
                    failures += 1;
                    tokio::time::sleep(self.retry.delay(failures, retry_after_of(&err))).await;
                }
                Err(err) => return Err(err),
            }
        }
    }

    /// Revokes the refresh token at Google (best effort on disconnect).
    pub async fn revoke(&self) -> Result<()> {
        let response = self
            .http
            .post(self.endpoints.revoke_url.clone())
            .form(&[("token", self.refresh_token.expose_secret())])
            .send()
            .await
            .map_err(map_reqwest_error)?;
        let status = response.status();
        if status.is_success() {
            return Ok(());
        }
        let body = response.text().await.unwrap_or_default();
        // Already revoked or expired tokens are fine for a disconnect.
        if status == StatusCode::BAD_REQUEST && body.contains("invalid_token") {
            return Ok(());
        }
        if status.is_server_error() {
            return Err(StorageError::Transient(format!("revoke returned {status}")));
        }
        Err(StorageError::Fatal(format!("revoke returned {status}")))
    }

    // ---- auth -------------------------------------------------------------------------

    async fn access_token(&self) -> Result<SecretString> {
        let mut guard = self.token.lock().await;
        if let Some(cached) = guard.as_ref()
            && cached.expires_at > Instant::now() + TOKEN_EXPIRY_MARGIN
        {
            return Ok(cached.value.clone());
        }
        let response =
            refresh_access_token(&self.http, &self.endpoints.token_url, &self.client, &self.refresh_token).await?;
        let expires_in = Duration::from_secs(response.expires_in.unwrap_or(3600));
        let value = SecretString::from(response.access_token);
        *guard = Some(CachedToken { value: value.clone(), expires_at: Instant::now() + expires_in });
        Ok(value)
    }

    async fn invalidate_token(&self) {
        *self.token.lock().await = None;
    }

    /// Sends an authenticated request with one token refresh on 401 and bounded
    /// retries for retryable errors. `build` is called for every attempt.
    async fn send<F>(&self, build: F) -> Result<Response>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        let mut unauthorized_retry = true;
        let mut failures = 0;
        loop {
            let token = self.access_token().await?;
            let outcome = match build(&self.http).bearer_auth(token.expose_secret()).send().await {
                Ok(response) if response.status().is_success() => return Ok(response),
                Ok(response) if response.status() == StatusCode::UNAUTHORIZED && unauthorized_retry => {
                    unauthorized_retry = false;
                    self.invalidate_token().await;
                    continue;
                }
                Ok(response) => error_from_response(response).await,
                Err(err) => map_reqwest_error(err),
            };
            if outcome.is_retryable() && failures + 1 < self.retry.attempts() {
                failures += 1;
                let delay = self.retry.delay(failures, retry_after_of(&outcome));
                tracing::debug!(error = %outcome, ?delay, "retrying Google Drive request");
                tokio::time::sleep(delay).await;
                continue;
            }
            return Err(outcome);
        }
    }

    async fn send_json<T: serde::de::DeserializeOwned, F>(&self, build: F) -> Result<T>
    where
        F: Fn(&Client) -> RequestBuilder,
    {
        let response = self.send(build).await?;
        response.json().await.map_err(|e| StorageError::Fatal(format!("invalid Google Drive response: {e}")))
    }

    // ---- folders ----------------------------------------------------------------------

    fn files_url(&self) -> Url {
        endpoint(&self.endpoints.drive_api, &["files"])
    }

    fn file_url(&self, id: &str) -> Url {
        endpoint(&self.endpoints.drive_api, &["files", id])
    }

    /// Query parameters that scope `files.list` to the configured drive.
    fn list_scope(&self) -> Vec<(&'static str, String)> {
        let mut params =
            vec![("supportsAllDrives", "true".to_owned()), ("includeItemsFromAllDrives", "true".to_owned())];
        if let Some(drive_id) = &self.options.shared_drive_id {
            params.push(("corpora", "drive".to_owned()));
            params.push(("driveId", drive_id.clone()));
        }
        params
    }

    async fn list_files(&self, query: &str, order_by: Option<&str>) -> Result<Vec<DriveFile>> {
        let mut files = Vec::new();
        let mut page_token: Option<String> = None;
        loop {
            let mut params = self.list_scope();
            params.push(("q", query.to_owned()));
            params
                .push(("fields", "nextPageToken,files(id,name,size,createdTime,webViewLink,appProperties)".to_owned()));
            params.push(("pageSize", LIST_PAGE_SIZE.to_owned()));
            if let Some(order) = order_by {
                params.push(("orderBy", order.to_owned()));
            }
            if let Some(token) = &page_token {
                params.push(("pageToken", token.clone()));
            }
            let url = self.files_url();
            let page: FileList = self.send_json(|http| http.get(url.clone()).query(&params)).await?;
            files.extend(page.files);
            match page.next_page_token.filter(|t| !t.is_empty()) {
                Some(next) => page_token = Some(next),
                None => return Ok(files),
            }
        }
    }

    async fn find_or_create_folder(
        &self,
        name: &str,
        parent: &str,
        match_name: bool,
        properties: &[(&str, &str)],
    ) -> Result<String> {
        let mut query =
            format!("mimeType = '{FOLDER_MIME}' and trashed = false and '{}' in parents", escape_query(parent));
        if match_name {
            query.push_str(&format!(" and name = '{}'", escape_query(name)));
        }
        for (key, value) in properties {
            query.push_str(&format!(
                " and appProperties has {{ key='{}' and value='{}' }}",
                escape_query(key),
                escape_query(value)
            ));
        }

        if let Some(existing) = self.list_files(&query, Some("createdTime")).await?.into_iter().next() {
            return Ok(existing.id);
        }

        let app_properties: serde_json::Map<String, serde_json::Value> =
            properties.iter().map(|(k, v)| ((*k).to_owned(), json!(v))).collect();
        let body = json!({
            "name": name,
            "mimeType": FOLDER_MIME,
            "parents": [parent],
            "appProperties": app_properties,
        });
        let url = self.files_url();
        let created: DriveFile = self
            .send_json(|http| {
                http.post(url.clone()).query(&[("supportsAllDrives", "true"), ("fields", "id,name")]).json(&body)
            })
            .await?;
        tracing::info!(folder = %name, "created Google Drive folder");
        Ok(created.id)
    }

    async fn root_folder(&self) -> Result<String> {
        if let Some(id) = self.folders.lock().await.get("root") {
            return Ok(id.clone());
        }
        let parent = self.options.shared_drive_id.clone().unwrap_or_else(|| "root".to_owned());
        let id =
            self.find_or_create_folder(&self.options.root_folder_name, &parent, true, &[("obdBackup", "root")]).await?;
        self.folders.lock().await.insert("root".into(), id.clone());
        Ok(id)
    }
}

fn retry_after_of(err: &StorageError) -> Option<Duration> {
    match err {
        StorageError::RateLimited { retry_after } => *retry_after,
        _ => None,
    }
}

#[async_trait]
impl StorageAdapter for GoogleDriveAdapter {
    fn provider_id(&self) -> &'static str {
        "google_drive"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities { resumable: true, server_side_checksum: true, max_file_size: Some(5 * 1024 * 1024 * 1024 * 1024) }
    }

    async fn ensure_target(&self, spec: &TargetSpec) -> Result<TargetId> {
        let cache_key = format!("instance:{}", spec.instance_id);
        if let Some(id) = self.folders.lock().await.get(&cache_key) {
            return Ok(TargetId(id.clone()));
        }
        let root = self.root_folder().await?;
        let id = self
            .find_or_create_folder(
                &spec.instance_name,
                &root,
                false,
                &[("obdBackup", "instance"), ("instanceId", spec.instance_id.as_str())],
            )
            .await?;
        self.folders.lock().await.insert(cache_key, id.clone());
        Ok(TargetId(id))
    }

    async fn upload(
        &self,
        target: &TargetId,
        file: &Path,
        meta: &UploadMeta,
        progress: UploadProgressFn,
        cancel: CancellationToken,
    ) -> Result<RemoteObject> {
        upload::resumable_upload(self, target, file, meta, progress, cancel).await
    }

    async fn list_backups(&self, target: &TargetId) -> Result<Vec<RemoteObject>> {
        let query = format!(
            "'{}' in parents and trashed = false and appProperties has {{ key='obdBackup' and value='file' }}",
            escape_query(&target.0)
        );
        let files = self.list_files(&query, Some("createdTime desc")).await?;
        Ok(files.into_iter().map(DriveFile::into_remote).collect())
    }

    async fn delete(&self, object: &RemoteObject) -> Result<()> {
        let url = self.file_url(&object.id);
        if self.options.permanent_delete {
            self.send(|http| http.delete(url.clone()).query(&[("supportsAllDrives", "true")])).await?;
        } else {
            let body = json!({ "trashed": true });
            self.send(|http| {
                http.patch(url.clone()).query(&[("supportsAllDrives", "true"), ("fields", "id")]).json(&body)
            })
            .await?;
        }
        Ok(())
    }
}
