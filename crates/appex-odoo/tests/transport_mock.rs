mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use appex_odoo::{
    AppexModuleTransport, BackupPhase, BackupRequest, BackupTransport, DbManagerTransport, OdooError, OdooRpc,
    OdooVersion, RpcProtocol,
};
use async_trait::async_trait;
use common::{ZipSpec, backup_zip, recorder, sha256_hex};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;
use url::Url;
use wiremock::matchers::{body_string_contains, header, method, path};
use wiremock::{Mock, MockServer, Request, ResponseTemplate};

fn version(major: u16) -> OdooVersion {
    OdooVersion { major, minor: 0, server_version: format!("{major}.0"), saas: false }
}

fn request(dir: &tempfile::TempDir) -> BackupRequest {
    BackupRequest {
        database: "cliente1".into(),
        include_filestore: true,
        dest_dir: dir.path().join("backups"),
        file_stem: "cliente1_2026-09-16_10-30-00".into(),
    }
}

fn dir_entries(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .map(|entries| entries.map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect())
        .unwrap_or_default();
    names.sort();
    names
}

// ---------------------------------------------------------------------------
// DbManagerTransport
// ---------------------------------------------------------------------------

#[tokio::test]
async fn db_manager_downloads_and_validates() {
    let server = MockServer::start().await;
    let zip = backup_zip(&ZipSpec::default());
    Mock::given(method("POST"))
        .and(path("/web/database/backup"))
        .and(body_string_contains("master_pwd=m%40ster"))
        .and(body_string_contains("name=cliente1"))
        .and(body_string_contains("backup_format=zip"))
        .respond_with(|req: &Request| {
            // Odoo < 19 must not receive the `filestore` field.
            assert!(!String::from_utf8_lossy(&req.body).contains("filestore"));
            ResponseTemplate::new(200)
                .set_body_raw(backup_zip(&ZipSpec::default()), "application/octet-stream; charset=binary")
                .set_delay(Duration::from_millis(1200))
        })
        .mount(&server)
        .await;

    let dir = tempfile::tempdir().unwrap();
    let transport = DbManagerTransport::new(
        common::client(),
        Url::parse(&server.uri()).unwrap(),
        "m@ster".into(),
        version(17),
        Duration::from_secs(30),
    );
    let (progress, events) = recorder();
    let backup = transport.run(&request(&dir), progress, CancellationToken::new()).await.unwrap();

    assert_eq!(backup.path, dir.path().join("backups/cliente1_2026-09-16_10-30-00.zip"));
    assert_eq!(backup.size, zip.len() as u64);
    assert_eq!(backup.sha256, sha256_hex(&zip));
    assert_eq!(backup.manifest.db_name, "cliente1");
    assert_eq!(dir_entries(&dir.path().join("backups")), vec!["cliente1_2026-09-16_10-30-00.zip"]);

    {
        let events = events.lock().unwrap();
        assert_eq!(events.first(), Some(&BackupPhase::Requesting));
        assert!(events.iter().any(|e| matches!(e, BackupPhase::ServerPreparing { .. })));
        assert!(
            events
                .iter()
                .any(|e| matches!(e, BackupPhase::Downloading { received, .. } if *received == zip.len() as u64))
        );
        assert_eq!(events.last(), Some(&BackupPhase::Validating));
    }

    // A second run never overwrites the first file.
    let (progress, _) = recorder();
    let second = transport.run(&request(&dir), progress, CancellationToken::new()).await.unwrap();
    assert_eq!(second.path, dir.path().join("backups/cliente1_2026-09-16_10-30-00-1.zip"));
}

#[tokio::test]
async fn db_manager_sends_filestore_flag_on_19() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/web/database/backup"))
        .and(body_string_contains("filestore=false"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            backup_zip(&ZipSpec { filestore_files: 0, ..ZipSpec::default() }),
            "application/octet-stream",
        ))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let transport = DbManagerTransport::new(
        common::client(),
        Url::parse(&server.uri()).unwrap(),
        "x".into(),
        version(19),
        Duration::from_secs(30),
    );
    let mut req = request(&dir);
    req.include_filestore = false;
    let (progress, _) = recorder();
    let backup = transport.run(&req, progress, CancellationToken::new()).await.unwrap();
    assert_eq!(backup.manifest.filestore_files, 0);
}

#[tokio::test]
async fn db_manager_maps_html_errors_and_cleans_up() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/web/database/backup"))
        .respond_with(ResponseTemplate::new(200).set_body_raw(
            r#"<html><body><div class="alert alert-danger" role="alert">Database backup error: Access Denied</div></body></html>"#,
            "text/html; charset=utf-8",
        ))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let transport = DbManagerTransport::new(
        common::client(),
        Url::parse(&server.uri()).unwrap(),
        "bad".into(),
        version(17),
        Duration::from_secs(30),
    );
    let (progress, _) = recorder();
    let err = transport.run(&request(&dir), progress, CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, OdooError::AccessDenied(_)), "{err:?}");
    assert!(dir_entries(&dir.path().join("backups")).is_empty());
}

#[tokio::test]
async fn db_manager_rejects_non_zip_payload() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/web/database/backup"))
        .respond_with(ResponseTemplate::new(200).set_body_raw("not a zip at all", "application/octet-stream"))
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let transport = DbManagerTransport::new(
        common::client(),
        Url::parse(&server.uri()).unwrap(),
        "x".into(),
        version(16),
        Duration::from_secs(30),
    );
    let (progress, _) = recorder();
    let err = transport.run(&request(&dir), progress, CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, OdooError::InvalidBackup(_)), "{err:?}");
    assert!(dir_entries(&dir.path().join("backups")).is_empty());
}

#[tokio::test]
async fn db_manager_cancel_and_prepare_timeout() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/web/database/backup"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_raw(backup_zip(&ZipSpec::default()), "application/octet-stream")
                .set_delay(Duration::from_secs(20)),
        )
        .mount(&server)
        .await;
    let dir = tempfile::tempdir().unwrap();
    let base = Url::parse(&server.uri()).unwrap();

    let transport =
        DbManagerTransport::new(common::client(), base.clone(), "x".into(), version(17), Duration::from_secs(60));
    let cancel = CancellationToken::new();
    let trigger = cancel.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        trigger.cancel();
    });
    let (progress, _) = recorder();
    let started = std::time::Instant::now();
    let err = transport.run(&request(&dir), progress, cancel).await.unwrap_err();
    assert!(matches!(err, OdooError::Cancelled), "{err:?}");
    assert!(started.elapsed() < Duration::from_secs(5));

    let transport =
        DbManagerTransport::new(common::client(), base, "x".into(), version(17), Duration::from_millis(400));
    let (progress, _) = recorder();
    let err = transport.run(&request(&dir), progress, CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, OdooError::PrepareTimeout), "{err:?}");
    assert!(dir_entries(&dir.path().join("backups")).is_empty());
}

// ---------------------------------------------------------------------------
// AppexModuleTransport
// ---------------------------------------------------------------------------

/// In-memory `appex.backup.api` implementation.
struct FakeModule {
    api_version: u64,
    polls_before_done: usize,
    polls: AtomicUsize,
    final_state: Value,
    calls: Mutex<Vec<String>>,
    missing: bool,
}

impl FakeModule {
    fn done(size: usize, sha256: &str) -> Self {
        Self {
            api_version: 1,
            polls_before_done: 2,
            polls: AtomicUsize::new(0),
            final_state: json!({"job_id": "job-1", "state": "done", "size": size, "sha256": sha256, "filename": "x.zip", "error": null}),
            calls: Mutex::new(Vec::new()),
            missing: false,
        }
    }
}

#[async_trait]
impl OdooRpc for FakeModule {
    fn protocol(&self) -> RpcProtocol {
        RpcProtocol::Json2
    }

    fn database(&self) -> &str {
        "cliente1"
    }

    async fn authenticate(&self) -> appex_odoo::Result<i64> {
        Ok(2)
    }

    async fn call(
        &self,
        model: &str,
        method: &str,
        _ids: &[i64],
        kwargs: Map<String, Value>,
    ) -> appex_odoo::Result<Value> {
        assert_eq!(model, "appex.backup.api");
        self.calls.lock().unwrap().push(method.to_owned());
        if self.missing {
            return Err(OdooError::Rpc {
                code: appex_odoo::MODEL_NOT_FOUND.into(),
                message: "Object appex.backup.api doesn't exist".into(),
            });
        }
        match method {
            "get_info" => Ok(
                json!({"api_version": self.api_version, "module_version": "17.0.1.0.0", "database": "cliente1", "filestore_supported": true}),
            ),
            "request_backup" => {
                assert_eq!(kwargs.get("include_filestore"), Some(&json!(true)));
                Ok(json!({"job_id": "job-1"}))
            }
            "get_job" => {
                assert_eq!(kwargs.get("job_id"), Some(&json!("job-1")));
                if self.polls.fetch_add(1, Ordering::SeqCst) < self.polls_before_done {
                    Ok(json!({"job_id": "job-1", "state": "running", "size": null, "sha256": null, "error": null}))
                } else {
                    Ok(self.final_state.clone())
                }
            }
            "discard_job" => Ok(json!(true)),
            other => panic!("unexpected method {other}"),
        }
    }
}

fn module_transport(rpc: Arc<FakeModule>, server: &MockServer) -> AppexModuleTransport {
    AppexModuleTransport::new(
        rpc,
        common::client(),
        Url::parse(&server.uri()).unwrap(),
        "api-key".into(),
        Duration::from_millis(50),
        Duration::from_secs(30),
    )
    .with_retry_base(Duration::from_millis(20))
}

#[tokio::test]
async fn module_happy_path() {
    let server = MockServer::start().await;
    let zip = backup_zip(&ZipSpec::default());
    let body = zip.clone();
    Mock::given(method("GET"))
        .and(path("/appex_backup/download/job-1"))
        .and(header("authorization", "Bearer api-key"))
        .and(header("x-odoo-database", "cliente1"))
        .respond_with(move |_: &Request| ResponseTemplate::new(200).set_body_raw(body.clone(), "application/zip"))
        .expect(1)
        .mount(&server)
        .await;

    let fake = Arc::new(FakeModule::done(zip.len(), &sha256_hex(&zip)));
    let dir = tempfile::tempdir().unwrap();
    let (progress, events) = recorder();
    let backup = module_transport(Arc::clone(&fake), &server)
        .run(&request(&dir), progress, CancellationToken::new())
        .await
        .unwrap();

    assert_eq!(backup.sha256, sha256_hex(&zip));
    assert_eq!(backup.size, zip.len() as u64);
    assert!(backup.path.ends_with("cliente1_2026-09-16_10-30-00.zip"));
    assert_eq!(*fake.calls.lock().unwrap().first().unwrap(), "get_info");
    assert_eq!(fake.calls.lock().unwrap().last().unwrap(), "discard_job");
    let events = events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e, BackupPhase::ServerPreparing { .. })));
    assert!(
        events.iter().any(|e| matches!(e, BackupPhase::Downloading { total: Some(t), .. } if *t == zip.len() as u64))
    );
}

#[tokio::test]
async fn module_resumes_with_range_after_truncated_download() {
    let server = MockServer::start().await;
    let zip = backup_zip(&ZipSpec::default());
    let half = zip.len() / 2;
    let body = zip.clone();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&ranges);
    Mock::given(method("GET"))
        .and(path("/appex_backup/download/job-1"))
        .respond_with(move |req: &Request| {
            let range = req.headers.get("range").map(|v| v.to_str().unwrap().to_owned());
            seen.lock().unwrap().push(range.clone());
            match range {
                // First attempt: the connection "drops" half way.
                None => ResponseTemplate::new(200).set_body_raw(body[..half].to_vec(), "application/zip"),
                Some(value) => {
                    let start: usize = value.trim_start_matches("bytes=").trim_end_matches('-').parse().unwrap();
                    ResponseTemplate::new(206)
                        .insert_header(
                            "content-range",
                            format!("bytes {start}-{}/{}", body.len() - 1, body.len()).as_str(),
                        )
                        .set_body_raw(body[start..].to_vec(), "application/zip")
                }
            }
        })
        .mount(&server)
        .await;

    let fake = Arc::new(FakeModule::done(zip.len(), &sha256_hex(&zip)));
    let dir = tempfile::tempdir().unwrap();
    let (progress, _) = recorder();
    let backup = module_transport(fake, &server).run(&request(&dir), progress, CancellationToken::new()).await.unwrap();
    assert_eq!(backup.sha256, sha256_hex(&zip));
    assert_eq!(*ranges.lock().unwrap(), vec![None, Some(format!("bytes={half}-"))]);
}

#[tokio::test]
async fn module_restarts_when_server_ignores_range() {
    let server = MockServer::start().await;
    let zip = backup_zip(&ZipSpec::default());
    let body = zip.clone();
    let attempts = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&attempts);
    Mock::given(method("GET"))
        .and(path("/appex_backup/download/job-1"))
        .respond_with(move |_: &Request| {
            if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                ResponseTemplate::new(200).set_body_raw(body[..100].to_vec(), "application/zip")
            } else {
                ResponseTemplate::new(200).set_body_raw(body.clone(), "application/zip")
            }
        })
        .mount(&server)
        .await;
    let fake = Arc::new(FakeModule::done(zip.len(), &sha256_hex(&zip)));
    let dir = tempfile::tempdir().unwrap();
    let (progress, _) = recorder();
    let backup = module_transport(fake, &server).run(&request(&dir), progress, CancellationToken::new()).await.unwrap();
    assert_eq!(backup.sha256, sha256_hex(&zip));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn module_detects_sha_mismatch() {
    let server = MockServer::start().await;
    let zip = backup_zip(&ZipSpec::default());
    let body = zip.clone();
    Mock::given(method("GET"))
        .and(path("/appex_backup/download/job-1"))
        .respond_with(move |_: &Request| ResponseTemplate::new(200).set_body_raw(body.clone(), "application/zip"))
        .mount(&server)
        .await;
    let fake = Arc::new(FakeModule::done(zip.len(), &"0".repeat(64)));
    let dir = tempfile::tempdir().unwrap();
    let (progress, _) = recorder();
    let err = module_transport(Arc::clone(&fake), &server)
        .run(&request(&dir), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(&err, OdooError::InvalidBackup(m) if m.contains("sha256")), "{err:?}");
    assert!(dir_entries(&dir.path().join("backups")).is_empty());
    assert_eq!(fake.calls.lock().unwrap().last().unwrap(), "discard_job");
}

#[tokio::test]
async fn module_failed_job_missing_module_and_bad_api_version() {
    let server = MockServer::start().await;
    let dir = tempfile::tempdir().unwrap();

    let mut failed = FakeModule::done(0, "");
    failed.final_state = json!({"job_id": "job-1", "state": "failed", "error": "pg_dump: error: connection failed"});
    let (progress, _) = recorder();
    let err = module_transport(Arc::new(failed), &server)
        .run(&request(&dir), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(&err, OdooError::ServerBackupError(m) if m.contains("pg_dump")), "{err:?}");

    let mut missing = FakeModule::done(0, "");
    missing.missing = true;
    let (progress, _) = recorder();
    let err = module_transport(Arc::new(missing), &server)
        .run(&request(&dir), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, OdooError::ModuleNotInstalled), "{err:?}");

    let mut future = FakeModule::done(0, "");
    future.api_version = 2;
    let (progress, _) = recorder();
    let err = module_transport(Arc::new(future), &server)
        .run(&request(&dir), progress, CancellationToken::new())
        .await
        .unwrap_err();
    assert!(matches!(err, OdooError::ModuleApiIncompatible(2)), "{err:?}");
}

#[tokio::test]
async fn module_gives_up_after_retries_on_server_errors() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/appex_backup/download/job-1"))
        .respond_with(ResponseTemplate::new(503))
        .expect(6)
        .mount(&server)
        .await;
    let fake = Arc::new(FakeModule::done(10, &"0".repeat(64)));
    let dir = tempfile::tempdir().unwrap();
    let (progress, _) = recorder();
    let err =
        module_transport(fake, &server).run(&request(&dir), progress, CancellationToken::new()).await.unwrap_err();
    assert!(matches!(err, OdooError::HttpStatus { status: 503, .. }), "{err:?}");
}
