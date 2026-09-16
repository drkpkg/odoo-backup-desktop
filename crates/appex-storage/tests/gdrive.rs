use std::sync::{Arc, Mutex};
use std::time::Duration;

use appex_storage::gdrive::{DriveOptions, GoogleDriveAdapter, GoogleEndpoints, OAuthClient, RetryPolicy};
use appex_storage::{RemoteObject, StorageAdapter, StorageError, TargetId, TargetSpec, UploadMeta, UploadProgressFn};
use chrono::{TimeZone, Utc};
use md5::{Digest, Md5};
use secrecy::SecretString;
use serde_json::json;
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::matchers::{
    body_json, body_partial_json, body_string_contains, header, method, path, query_param, query_param_contains,
    query_param_is_missing,
};
use wiremock::{Mock, MockBuilder, MockServer, ResponseTemplate};

const KIB_256: usize = 256 * 1024;

fn http() -> reqwest::Client {
    reqwest::Client::builder().no_proxy().build().unwrap()
}

fn client() -> OAuthClient {
    OAuthClient { client_id: "cid".into(), client_secret: Some(SecretString::from("csecret")) }
}

fn fast_retry(max_attempts: u32) -> RetryPolicy {
    RetryPolicy { max_attempts, base_delay: Duration::from_millis(1), max_delay: Duration::from_millis(5) }
}

fn adapter_with(server: &MockServer, options: DriveOptions, retry: RetryPolicy) -> GoogleDriveAdapter {
    let base = Url::parse(&format!("{}/", server.uri())).unwrap();
    GoogleDriveAdapter::with_endpoints(
        http(),
        client(),
        SecretString::from("refresh-1"),
        options,
        GoogleEndpoints::with_base(&base),
    )
    .with_retry_policy(retry)
}

fn adapter(server: &MockServer) -> GoogleDriveAdapter {
    adapter_with(server, DriveOptions { chunk_size: KIB_256, ..DriveOptions::default() }, fast_retry(4))
}

async fn mount_token(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=refresh_token"))
        .and(body_string_contains("refresh_token=refresh-1"))
        .and(body_string_contains("client_secret=csecret"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "access_token": "at-1", "expires_in": 3600, "token_type": "Bearer"
        })))
        .mount(server)
        .await;
}

fn about_ok() -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({"user": {"emailAddress": "ops@appex.lat", "displayName": "Ops"}}))
}

fn meta() -> UploadMeta {
    UploadMeta {
        instance_id: "inst-1".into(),
        database: "cliente1".into(),
        sha256: "f".repeat(64),
        created_at: Utc::now(),
    }
}

fn data(len: usize) -> Vec<u8> {
    (0..len).map(|i| (i % 251) as u8).collect()
}

fn md5_hex(bytes: &[u8]) -> String {
    Md5::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}

fn write_temp(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("cliente1_2026-09-16_10-00-00.zip");
    std::fs::write(&file, bytes).unwrap();
    (dir, file)
}

fn recorder() -> (UploadProgressFn, Arc<Mutex<Vec<u64>>>) {
    let calls = Arc::new(Mutex::new(Vec::new()));
    let sink = calls.clone();
    (Arc::new(move |sent, _total| sink.lock().unwrap().push(sent)), calls)
}

async fn mount_session(server: &MockServer, session: &str, total: usize) {
    Mock::given(method("POST"))
        .and(path("/upload/drive/v3/files"))
        .and(query_param("uploadType", "resumable"))
        .and(query_param("supportsAllDrives", "true"))
        .and(header("x-upload-content-length", total.to_string().as_str()))
        .and(header("x-upload-content-type", "application/zip"))
        .and(body_partial_json(json!({
            "name": "cliente1_2026-09-16_10-00-00.zip",
            "parents": ["folder-1"],
            "appProperties": {"appexBackup": "file", "instanceId": "inst-1", "database": "cliente1"}
        })))
        .respond_with(
            ResponseTemplate::new(200).insert_header("Location", format!("{}/upload/{session}", server.uri())),
        )
        .up_to_n_times(1)
        .mount(server)
        .await;
}

fn chunk(server_session: &str, range: &str) -> MockBuilder {
    Mock::given(method("PUT")).and(path(format!("/upload/{server_session}"))).and(header("content-range", range))
}

fn incomplete(last_byte: u64) -> ResponseTemplate {
    ResponseTemplate::new(308).insert_header("Range", format!("bytes=0-{last_byte}"))
}

fn completed(bytes: &[u8]) -> ResponseTemplate {
    completed_with_md5(bytes.len(), &md5_hex(bytes))
}

fn completed_with_md5(size: usize, md5: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_json(json!({
        "id": "file-1",
        "name": "cliente1_2026-09-16_10-00-00.zip",
        "size": size.to_string(),
        "createdTime": "2026-09-16T10:00:00.000Z",
        "webViewLink": "https://drive.google.com/file/d/file-1/view",
        "md5Checksum": md5,
        "appProperties": {"appexBackup": "file", "instanceId": "inst-1"}
    }))
}

#[tokio::test]
async fn access_token_is_cached() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at-1", "expires_in": 3600})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .and(header("authorization", "Bearer at-1"))
        .and(query_param("fields", "user(emailAddress,displayName)"))
        .respond_with(about_ok())
        .expect(2)
        .mount(&server)
        .await;

    let drive = adapter(&server);
    let info = drive.account_info().await.unwrap();
    assert_eq!(info.email.as_deref(), Some("ops@appex.lat"));
    assert_eq!(info.display_name.as_deref(), Some("Ops"));
    drive.account_info().await.unwrap();
}

#[tokio::test]
async fn unauthorized_response_refreshes_the_token_once() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"access_token": "at-1", "expires_in": 3600})))
        .expect(2)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/drive/v3/about")).respond_with(about_ok()).mount(&server).await;

    adapter(&server).account_info().await.unwrap();
}

#[tokio::test]
async fn invalid_grant_means_auth_expired() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "invalid_grant", "error_description": "Token has been expired or revoked."
        })))
        .mount(&server)
        .await;

    let err = adapter(&server).account_info().await.unwrap_err();
    assert!(matches!(err, StorageError::AuthExpired), "{err:?}");
}

#[tokio::test]
async fn invalid_client_is_a_configuration_error() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(401).set_body_json(json!({"error": "invalid_client"})))
        .mount(&server)
        .await;
    let err = adapter(&server).account_info().await.unwrap_err();
    assert!(matches!(err, StorageError::NotConfigured(_)), "{err:?}");
}

#[tokio::test]
async fn rate_limit_and_quota_errors_are_mapped() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "7"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": 403, "message": "Rate limit", "errors": [{"reason": "userRateLimitExceeded"}]}
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "error": {"code": 403, "message": "The user's Drive storage quota has been exceeded.",
                      "errors": [{"reason": "storageQuotaExceeded"}]}
        })))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/drive/v3/about")).respond_with(about_ok()).mount(&server).await;

    let no_retry = adapter_with(&server, DriveOptions::default(), fast_retry(1));
    let err = no_retry.account_info().await.unwrap_err();
    assert!(
        matches!(err, StorageError::RateLimited { retry_after: Some(d) } if d == Duration::from_secs(7)),
        "{err:?}"
    );
    let err = no_retry.account_info().await.unwrap_err();
    assert!(matches!(err, StorageError::RateLimited { retry_after: None }), "{err:?}");
    let err = no_retry.account_info().await.unwrap_err();
    assert!(matches!(err, StorageError::QuotaExceeded(_)), "{err:?}");
    no_retry.account_info().await.unwrap();
}

#[tokio::test]
async fn retryable_errors_are_retried_with_backoff() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(429).insert_header("Retry-After", "30"))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/about"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET")).and(path("/drive/v3/about")).respond_with(about_ok()).expect(1).mount(&server).await;

    // Retry-After (30 s) is capped by the policy's max_delay (5 ms).
    let started = std::time::Instant::now();
    adapter(&server).account_info().await.unwrap();
    assert!(started.elapsed() < Duration::from_secs(5));
}

#[tokio::test]
async fn ensure_target_reuses_root_and_creates_instance_folder() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param_contains("q", "'root' in parents"))
        .and(query_param_contains("q", "name = 'Appex Backup'"))
        .and(query_param_contains("q", "appProperties has { key='appexBackup' and value='root' }"))
        .and(query_param("supportsAllDrives", "true"))
        .respond_with(
            ResponseTemplate::new(200).set_body_json(json!({"files": [{"id": "root-1", "name": "Appex Backup"}]})),
        )
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param_contains("q", "'root-1' in parents"))
        .and(query_param_contains("q", "appProperties has { key='instanceId' and value='inst-9' }"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": []})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files"))
        .and(query_param("supportsAllDrives", "true"))
        .and(body_partial_json(json!({
            "name": "Cliente 'Nueve'",
            "mimeType": "application/vnd.google-apps.folder",
            "parents": ["root-1"],
            "appProperties": {"appexBackup": "instance", "instanceId": "inst-9"}
        })))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "folder-9", "name": "Cliente 'Nueve'"})))
        .expect(1)
        .mount(&server)
        .await;

    let drive = adapter(&server);
    let spec = TargetSpec { instance_id: "inst-9".into(), instance_name: "Cliente 'Nueve'".into() };
    assert_eq!(drive.ensure_target(&spec).await.unwrap(), TargetId("folder-9".into()));
    // Cached: no further requests.
    assert_eq!(drive.ensure_target(&spec).await.unwrap(), TargetId("folder-9".into()));
}

#[tokio::test]
async fn ensure_target_on_shared_drive_creates_missing_root() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("corpora", "drive"))
        .and(query_param("driveId", "sd-1"))
        .and(query_param("includeItemsFromAllDrives", "true"))
        .and(query_param_contains("q", "'sd-1' in parents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": []})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/drive/v3/files"))
        .and(body_partial_json(
            json!({"name": "Backups", "parents": ["sd-1"], "appProperties": {"appexBackup": "root"}}),
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "root-sd"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("driveId", "sd-1"))
        .and(query_param_contains("q", "'root-sd' in parents"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"files": [{"id": "inst-folder"}]})))
        .expect(1)
        .mount(&server)
        .await;

    let options = DriveOptions {
        root_folder_name: "Backups".into(),
        shared_drive_id: Some("sd-1".into()),
        ..DriveOptions::default()
    };
    let drive = adapter_with(&server, options, fast_retry(2));
    let spec = TargetSpec { instance_id: "inst-1".into(), instance_name: "Uno".into() };
    assert_eq!(drive.ensure_target(&spec).await.unwrap(), TargetId("inst-folder".into()));
}

#[tokio::test]
async fn multi_chunk_resumable_upload() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(2 * KIB_256 + 1000);
    let total = bytes.len();
    mount_session(&server, "session-1", total).await;
    chunk("session-1", "bytes 0-262143/525288").respond_with(incomplete(262_143)).expect(1).mount(&server).await;
    chunk("session-1", "bytes 262144-524287/525288").respond_with(incomplete(524_287)).expect(1).mount(&server).await;
    chunk("session-1", "bytes 524288-525287/525288").respond_with(completed(&bytes)).expect(1).mount(&server).await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, calls) = recorder();
    let object = adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(
        object,
        RemoteObject {
            id: "file-1".into(),
            name: "cliente1_2026-09-16_10-00-00.zip".into(),
            size: Some(total as u64),
            created_at: Some(Utc.with_ymd_and_hms(2026, 9, 16, 10, 0, 0).unwrap()),
            instance_id: Some("inst-1".into()),
            web_link: Some("https://drive.google.com/file/d/file-1/view".into()),
        }
    );
    assert_eq!(*calls.lock().unwrap(), vec![262_144, 524_288, 525_288]);
}

#[tokio::test]
async fn upload_follows_the_server_confirmed_offset() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(2 * KIB_256);
    mount_session(&server, "session-1", bytes.len()).await;
    // The server kept only the first 100 000 bytes of the first chunk.
    chunk("session-1", "bytes 0-262143/524288").respond_with(incomplete(99_999)).expect(1).mount(&server).await;
    chunk("session-1", "bytes 100000-362143/524288").respond_with(incomplete(362_143)).expect(1).mount(&server).await;
    // MD5 must cover each byte once even though bytes were re-sent.
    chunk("session-1", "bytes 362144-524287/524288").respond_with(completed(&bytes)).expect(1).mount(&server).await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, _) = recorder();
    adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap();
}

#[tokio::test]
async fn upload_resumes_after_a_server_error_using_status_query() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(2 * KIB_256);
    mount_session(&server, "session-1", bytes.len()).await;
    chunk("session-1", "bytes 0-262143/524288")
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .expect(1)
        .mount(&server)
        .await;
    chunk("session-1", "bytes */524288").respond_with(incomplete(262_143)).expect(1).mount(&server).await;
    chunk("session-1", "bytes 262144-524287/524288").respond_with(completed(&bytes)).expect(1).mount(&server).await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, calls) = recorder();
    adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(*calls.lock().unwrap(), vec![262_144, 524_288]);
}

#[tokio::test]
async fn persistent_chunk_failures_end_the_upload() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(1000);
    mount_session(&server, "session-1", bytes.len()).await;
    chunk("session-1", "bytes 0-999/1000").respond_with(ResponseTemplate::new(502)).expect(4).mount(&server).await;
    // Nothing persisted: 308 without Range.
    chunk("session-1", "bytes */1000").respond_with(ResponseTemplate::new(308)).expect(3).mount(&server).await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, _) = recorder();
    let err = adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::Transient(_)), "{err:?}");
}

#[tokio::test]
async fn expired_session_is_restarted_once() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(1000);
    mount_session(&server, "session-a", bytes.len()).await;
    mount_session(&server, "session-b", bytes.len()).await;
    chunk("session-a", "bytes 0-999/1000").respond_with(ResponseTemplate::new(404)).expect(1).mount(&server).await;
    chunk("session-b", "bytes 0-999/1000").respond_with(completed(&bytes)).expect(1).mount(&server).await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, _) = recorder();
    let object = adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(object.id, "file-1");
}

#[tokio::test]
async fn md5_mismatch_fails_and_removes_the_upload() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    let bytes = data(1000);
    mount_session(&server, "session-1", bytes.len()).await;
    chunk("session-1", "bytes 0-999/1000")
        .respond_with(completed_with_md5(bytes.len(), "00000000000000000000000000000000"))
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/drive/v3/files/file-1"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;

    let (_dir, file) = write_temp(&bytes);
    let (progress, _) = recorder();
    let err = adapter(&server)
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(&err, StorageError::Fatal(m) if m.contains("checksum mismatch")), "{err:?}");
}

#[tokio::test]
async fn upload_rejects_misaligned_chunks_and_honors_cancellation() {
    let server = MockServer::start().await;
    let (_dir, file) = write_temp(&data(1000));

    let misaligned = adapter_with(&server, DriveOptions { chunk_size: 1000, ..DriveOptions::default() }, fast_retry(1));
    let (progress, _) = recorder();
    let err = misaligned
        .upload(&TargetId("folder-1".into()), &file, &meta(), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::Fatal(_)));

    let cancel = CancellationToken::new();
    cancel.cancel();
    let (progress, _) = recorder();
    let err =
        adapter(&server).upload(&TargetId("folder-1".into()), &file, &meta(), progress, cancel).await.unwrap_err();
    assert!(matches!(err, StorageError::Cancelled));
    assert!(server.received_requests().await.unwrap().is_empty());
}

#[tokio::test]
async fn list_backups_paginates() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param_is_missing("pageToken"))
        .and(query_param_contains("q", "'folder-1' in parents"))
        .and(query_param_contains("q", "appProperties has { key='appexBackup' and value='file' }"))
        .and(query_param("orderBy", "createdTime desc"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [
                {"id": "c", "name": "c.zip", "size": "30", "createdTime": "2026-09-16T10:00:00Z",
                 "appProperties": {"instanceId": "inst-1"}},
                {"id": "b", "name": "b.zip", "size": "20", "createdTime": "2026-09-15T10:00:00Z"}
            ],
            "nextPageToken": "p2"
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/drive/v3/files"))
        .and(query_param("pageToken", "p2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "files": [{"id": "a", "name": "a.zip", "size": "10", "createdTime": "2026-09-14T10:00:00Z"}]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let objects = adapter(&server).list_backups(&TargetId("folder-1".into())).await.unwrap();
    assert_eq!(objects.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), vec!["c", "b", "a"]);
    assert_eq!(objects[0].size, Some(30));
    assert_eq!(objects[0].instance_id.as_deref(), Some("inst-1"));
    assert_eq!(objects[2].created_at, Some(Utc.with_ymd_and_hms(2026, 9, 14, 10, 0, 0).unwrap()));
}

#[tokio::test]
async fn delete_trashes_or_removes_permanently() {
    let server = MockServer::start().await;
    mount_token(&server).await;
    Mock::given(method("PATCH"))
        .and(path("/drive/v3/files/abc"))
        .and(query_param("supportsAllDrives", "true"))
        .and(body_json(json!({"trashed": true})))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({"id": "abc"})))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/drive/v3/files/abc"))
        .and(query_param("supportsAllDrives", "true"))
        .respond_with(ResponseTemplate::new(204))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("DELETE"))
        .and(path("/drive/v3/files/gone"))
        .respond_with(
            ResponseTemplate::new(404)
                .set_body_json(json!({"error": {"code": 404, "message": "File not found: gone."}})),
        )
        .mount(&server)
        .await;

    let object = |id: &str| RemoteObject {
        id: id.into(),
        name: "x.zip".into(),
        size: None,
        created_at: None,
        instance_id: None,
        web_link: None,
    };
    adapter(&server).delete(&object("abc")).await.unwrap();

    let permanent =
        adapter_with(&server, DriveOptions { permanent_delete: true, ..DriveOptions::default() }, fast_retry(1));
    permanent.delete(&object("abc")).await.unwrap();
    assert!(matches!(permanent.delete(&object("gone")).await, Err(StorageError::NotFound(_))));
}

#[tokio::test]
async fn revoke_accepts_already_invalid_tokens() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .and(body_string_contains("token=refresh-1"))
        .respond_with(ResponseTemplate::new(200))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/revoke"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({"error": "invalid_token"})))
        .mount(&server)
        .await;

    let drive = adapter(&server);
    drive.revoke().await.unwrap();
    drive.revoke().await.unwrap();
}
