//! Shared Google API plumbing: token endpoint, error mapping, Drive file DTOs.

use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::header::RETRY_AFTER;
use reqwest::{Client, Response, StatusCode};
use secrecy::{ExposeSecret, SecretString};
use serde::Deserialize;
use url::Url;

use super::OAuthClient;
use crate::util::map_reqwest_error;
use crate::{RemoteObject, Result, StorageError};

pub(crate) const FOLDER_MIME: &str = "application/vnd.google-apps.folder";
pub(crate) const FILE_FIELDS: &str = "id,name,size,createdTime,webViewLink,md5Checksum,appProperties";

#[derive(Debug, Deserialize)]
pub(crate) struct TokenResponse {
    pub access_token: String,
    pub expires_in: Option<u64>,
    pub refresh_token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct OAuthErrorBody {
    error: String,
    error_description: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DriveFile {
    pub id: String,
    #[serde(default)]
    pub name: String,
    pub size: Option<String>,
    pub created_time: Option<String>,
    pub web_view_link: Option<String>,
    pub md5_checksum: Option<String>,
    #[serde(default)]
    pub app_properties: std::collections::HashMap<String, String>,
}

impl DriveFile {
    pub fn into_remote(self) -> RemoteObject {
        RemoteObject {
            size: self.size.as_deref().and_then(|s| s.parse().ok()),
            created_at: self
                .created_time
                .as_deref()
                .and_then(|t| DateTime::parse_from_rfc3339(t).ok())
                .map(|t| t.with_timezone(&Utc)),
            instance_id: self.app_properties.get("instanceId").cloned(),
            web_link: self.web_view_link,
            id: self.id,
            name: self.name,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FileList {
    #[serde(default)]
    pub files: Vec<DriveFile>,
    pub next_page_token: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutResponse {
    user: Option<AboutUser>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AboutUser {
    email_address: Option<String>,
    display_name: Option<String>,
}

/// Appends path segments to a base URL such as `https://www.googleapis.com/drive/v3/`.
pub(crate) fn endpoint(base: &Url, segments: &[&str]) -> Url {
    let mut url = base.clone();
    if let Ok(mut path) = url.path_segments_mut() {
        path.pop_if_empty().extend(segments);
    }
    url
}

/// Posts a form to the token endpoint and maps OAuth errors.
pub(crate) async fn token_request(http: &Client, token_url: &Url, form: &[(&str, &str)]) -> Result<TokenResponse> {
    let response = http.post(token_url.clone()).form(form).send().await.map_err(map_reqwest_error)?;
    let status = response.status();
    if status.is_success() {
        return response.json().await.map_err(|e| StorageError::Fatal(format!("invalid token response: {e}")));
    }
    if status.is_server_error() {
        return Err(StorageError::Transient(format!("token endpoint returned {status}")));
    }
    let body = response.text().await.unwrap_or_default();
    let parsed: Option<OAuthErrorBody> = serde_json::from_str(&body).ok();
    let (error, description) = match parsed {
        Some(b) => (b.error, b.error_description.unwrap_or_default()),
        None => (status.to_string(), String::new()),
    };
    Err(match error.as_str() {
        "invalid_grant" => StorageError::AuthExpired,
        "invalid_client" | "unauthorized_client" => {
            StorageError::NotConfigured(format!("OAuth client rejected ({error}): {description}"))
        }
        _ => StorageError::AuthorizationDenied(format!("{error}: {description}")),
    })
}

/// Exchanges a refresh token for an access token.
pub(crate) async fn refresh_access_token(
    http: &Client,
    token_url: &Url,
    client: &OAuthClient,
    refresh_token: &SecretString,
) -> Result<TokenResponse> {
    let mut form = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token.expose_secret()),
        ("client_id", client.client_id.as_str()),
    ];
    if let Some(secret) = &client.client_secret {
        form.push(("client_secret", secret.expose_secret()));
    }
    token_request(http, token_url, &form).await
}

/// Reads the account email and display name (`about.get`).
pub(crate) async fn fetch_about(
    http: &Client,
    drive_api: &Url,
    access_token: &str,
) -> Result<(Option<String>, Option<String>)> {
    let mut url = endpoint(drive_api, &["about"]);
    url.query_pairs_mut().append_pair("fields", "user(emailAddress,displayName)");
    let response = http.get(url).bearer_auth(access_token).send().await.map_err(map_reqwest_error)?;
    if !response.status().is_success() {
        return Err(error_from_response(response).await);
    }
    let about: AboutResponse =
        response.json().await.map_err(|e| StorageError::Fatal(format!("invalid about response: {e}")))?;
    let user = about.user.unwrap_or(AboutUser { email_address: None, display_name: None });
    Ok((user.email_address, user.display_name))
}

fn retry_after(response: &Response) -> Option<Duration> {
    response
        .headers()
        .get(RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse::<u64>().ok())
        .map(Duration::from_secs)
}

/// Maps a non-success Google API response to a [`StorageError`].
pub(crate) async fn error_from_response(response: Response) -> StorageError {
    let status = response.status();
    let retry_after = retry_after(&response);
    let body = response.text().await.unwrap_or_default();
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap_or(serde_json::Value::Null);
    let message = parsed["error"]["message"].as_str().map(str::to_owned).unwrap_or_else(|| status.to_string());
    let reasons: Vec<&str> = parsed["error"]["errors"]
        .as_array()
        .map(|errors| errors.iter().filter_map(|e| e["reason"].as_str()).collect())
        .unwrap_or_default();
    let has_reason = |wanted: &[&str]| reasons.iter().any(|r| wanted.contains(r));

    match status {
        StatusCode::UNAUTHORIZED => StorageError::AuthExpired,
        StatusCode::TOO_MANY_REQUESTS => StorageError::RateLimited { retry_after },
        StatusCode::NOT_FOUND => StorageError::NotFound(message),
        StatusCode::FORBIDDEN
            if has_reason(&["storageQuotaExceeded", "quotaExceeded", "teamDriveFileLimitExceeded"]) =>
        {
            StorageError::QuotaExceeded(message)
        }
        StatusCode::FORBIDDEN
            if has_reason(&["userRateLimitExceeded", "rateLimitExceeded", "sharingRateLimitExceeded"]) =>
        {
            StorageError::RateLimited { retry_after }
        }
        s if s.is_server_error() => StorageError::Transient(format!("{status}: {message}")),
        _ => StorageError::Fatal(format!("{status}: {message}")),
    }
}
