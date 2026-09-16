//! Integration tests against real Odoo servers. Run with `dev/odoo/run-integration.sh`
//! (all tests are `#[ignore]` and need the environment variables set by that script).

use std::net::SocketAddr;
use std::time::Duration;

use appex_odoo::{
    BackupRequest, BackupTransport, CheckStatus, Credentials, DbManagerTransport, OdooError, OdooRpc, ProbeInput,
    ProbeWarning, ProtocolPreference, RpcProtocol, SecretKind, TransportKind, connect, detect_version, probe,
};
use serde_json::{Map, json};
use tokio_util::sync::CancellationToken;
use url::Url;

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run dev/odoo/run-integration.sh"))
}

fn url(name: &str) -> Url {
    Url::parse(&env(name)).unwrap()
}

/// `itNN.localhost` → 127.0.0.1 (the URL port is kept); Host still selects the database.
fn client() -> reqwest::Client {
    let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
    reqwest::Client::builder()
        .resolve("it15.localhost", loopback)
        .resolve("it17.localhost", loopback)
        .resolve("it19.localhost", loopback)
        .connect_timeout(Duration::from_secs(10))
        .build()
        .unwrap()
}

fn password() -> Credentials {
    Credentials {
        login: env("APPEX_IT_ADMIN_LOGIN"),
        secret: env("APPEX_IT_ADMIN_PASSWORD").into(),
        kind: SecretKind::Password,
    }
}

fn api_key() -> Credentials {
    Credentials {
        login: env("APPEX_IT_ADMIN_LOGIN"),
        secret: env("APPEX_IT_ODOO19_API_KEY").into(),
        kind: SecretKind::ApiKey,
    }
}

fn progress() -> appex_odoo::ProgressFn {
    std::sync::Arc::new(|_| {})
}

/// Writes an avatar on the admin user so the filestore holds attachments.
///
/// Only keyword arguments are portable (JSON-2 has no positional args), and ORM
/// parameter names differ between versions (`res.users.write(values)` on 15,
/// `write(vals)` later), so both names are tried.
async fn add_filestore_content(rpc: &dyn OdooRpc) {
    const PNG_1X1: &str =
        "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAMAASsJTYQAAAAASUVORK5CYII=";
    let mut last_error = None;
    for name in ["vals", "values"] {
        let mut kwargs = Map::new();
        kwargs.insert(name.into(), json!({"image_1920": PNG_1X1}));
        match rpc.call("res.users", "write", &[2], kwargs).await {
            Ok(_) => return,
            Err(err) => last_error = Some(err),
        }
    }
    panic!("write avatar: {last_error:?}");
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_detects_versions() {
    let http = client();
    for (var, major) in [("APPEX_IT_ODOO15_URL", 15), ("APPEX_IT_ODOO17_URL", 17), ("APPEX_IT_ODOO19_URL", 19)] {
        let version = detect_version(&http, &url(var)).await.unwrap();
        eprintln!("{var}: {version:?}");
        assert_eq!(version.major, major);
        assert!(version.is_supported());
    }
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_xmlrpc_with_password_on_15_and_17() {
    let http = client();
    for (var, db) in [("APPEX_IT_ODOO15_URL", "it15"), ("APPEX_IT_ODOO17_URL", "it17")] {
        let rpc = connect(http.clone(), url(var), db.into(), password(), RpcProtocol::XmlRpc);
        let uid = rpc.authenticate().await.unwrap();
        assert_eq!(uid, 2, "{var}");

        let mut kwargs = Map::new();
        kwargs.insert("fields".into(), json!(["login"]));
        let users = rpc.call("res.users", "read", &[uid], kwargs).await.unwrap();
        assert_eq!(users[0]["login"], "admin");

        let missing = rpc.call("appex.backup.api", "get_info", &[], Map::new()).await.unwrap_err();
        eprintln!("{var} missing model: {missing:?}");
        assert!(missing.is_model_not_found(), "{var}: {missing:?}");

        let bad = Credentials { secret: "wrong-password".into(), ..password() };
        let err =
            connect(http.clone(), url(var), db.into(), bad, RpcProtocol::XmlRpc).authenticate().await.unwrap_err();
        assert!(matches!(err, OdooError::AuthenticationFailed), "{var}: {err:?}");

        let err = connect(http.clone(), url(var), "nope".into(), password(), RpcProtocol::XmlRpc)
            .authenticate()
            .await
            .unwrap_err();
        eprintln!("{var} unknown database: {err:?}");
    }
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_json2_and_xmlrpc_with_api_key_on_19() {
    let http = client();
    let base = url("APPEX_IT_ODOO19_URL");

    let json2 = connect(http.clone(), base.clone(), "it19".into(), api_key(), RpcProtocol::Json2);
    assert_eq!(json2.authenticate().await.unwrap(), 2);
    let mut kwargs = Map::new();
    kwargs.insert("fields".into(), json!(["login"]));
    let users = json2.call("res.users", "read", &[2], kwargs).await.unwrap();
    assert_eq!(users[0]["login"], "admin");
    let missing = json2.call("appex.backup.api", "get_info", &[], Map::new()).await.unwrap_err();
    eprintln!("json2 missing model: {missing:?}");
    assert!(missing.is_model_not_found(), "{missing:?}");

    // Database resolved by dbfilter when the header is omitted.
    let no_db = connect(http.clone(), base.clone(), String::new(), api_key(), RpcProtocol::Json2);
    assert_eq!(no_db.authenticate().await.unwrap(), 2);

    let bad = Credentials { secret: "0123456789abcdef".into(), ..api_key() };
    let err =
        connect(http.clone(), base.clone(), "it19".into(), bad, RpcProtocol::Json2).authenticate().await.unwrap_err();
    assert!(matches!(err, OdooError::AuthenticationFailed), "{err:?}");

    let xmlrpc = connect(http.clone(), base, "it19".into(), api_key(), RpcProtocol::XmlRpc);
    assert_eq!(xmlrpc.authenticate().await.unwrap(), 2);
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_probe_reports_module_missing_and_db_manager_ok() {
    let http = client();
    let cases = [
        ("APPEX_IT_ODOO15_URL", "it15", password(), RpcProtocol::XmlRpc),
        ("APPEX_IT_ODOO17_URL", "it17", password(), RpcProtocol::XmlRpc),
        ("APPEX_IT_ODOO19_URL", "it19", api_key(), RpcProtocol::Json2),
    ];
    for (var, db, credentials, expected_protocol) in cases {
        let report = probe(
            &http,
            ProbeInput {
                base_url: url(var),
                database: None,
                credentials: Some(credentials),
                master_password: Some(env("APPEX_IT_MASTER_PASSWORD").into()),
                protocol_preference: ProtocolPreference::Auto,
            },
        )
        .await
        .unwrap();
        eprintln!("{var}: {}", serde_json::to_string(&report).unwrap());
        assert_eq!(report.database.as_deref(), Some(db));
        assert!(report.warnings.contains(&ProbeWarning::DatabaseDerivedFromHost));
        assert!(report.warnings.contains(&ProbeWarning::InsecureHttp));
        assert_eq!(report.protocol, Some(expected_protocol));
        assert_eq!(report.auth, CheckStatus::Ok, "{var}");
        assert!(matches!(&report.module, CheckStatus::Failed { code, .. } if code == "module_not_installed"), "{var}");
        assert_eq!(report.db_manager, CheckStatus::Ok, "{var}");
        assert_eq!(report.recommended_transport, Some(TransportKind::DbManager));
    }
}

async fn backup_with_db_manager(
    var: &str,
    db: &str,
    major: u16,
    include_filestore: bool,
) -> appex_odoo::DownloadedBackup {
    let http = client();
    let base = url(var);
    let version = detect_version(&http, &base).await.unwrap();
    assert_eq!(version.major, major);
    let dir = tempfile::tempdir().unwrap();
    let request = BackupRequest {
        database: db.into(),
        include_filestore,
        dest_dir: dir.path().to_path_buf(),
        file_stem: format!("{db}_it"),
    };
    let transport =
        DbManagerTransport::new(http, base, env("APPEX_IT_MASTER_PASSWORD").into(), version, Duration::from_secs(600));
    let started = std::time::Instant::now();
    let backup = transport.run(&request, progress(), CancellationToken::new()).await.unwrap();
    eprintln!(
        "{var} filestore={include_filestore}: {} bytes, {} filestore files, pg {} in {:?}",
        backup.size,
        backup.manifest.filestore_files,
        backup.manifest.pg_version,
        started.elapsed()
    );
    assert!(backup.path.is_file());
    assert_eq!(backup.manifest.db_name, db);
    assert!(backup.manifest.module_count > 0);
    // Keep the temp dir alive only for the assertions above.
    drop(dir);
    backup
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_db_manager_backup_on_all_versions() {
    let http = client();
    for (var, db, credentials, protocol) in [
        ("APPEX_IT_ODOO15_URL", "it15", password(), RpcProtocol::XmlRpc),
        ("APPEX_IT_ODOO17_URL", "it17", password(), RpcProtocol::XmlRpc),
        ("APPEX_IT_ODOO19_URL", "it19", api_key(), RpcProtocol::Json2),
    ] {
        let rpc = connect(http.clone(), url(var), db.into(), credentials, protocol);
        add_filestore_content(rpc.as_ref()).await;
    }

    let b15 = backup_with_db_manager("APPEX_IT_ODOO15_URL", "it15", 15, true).await;
    assert!(b15.manifest.filestore_files > 0);
    let b17 = backup_with_db_manager("APPEX_IT_ODOO17_URL", "it17", 17, true).await;
    assert!(b17.manifest.filestore_files > 0);
    let b19 = backup_with_db_manager("APPEX_IT_ODOO19_URL", "it19", 19, true).await;
    assert!(b19.manifest.filestore_files > 0);
    let b19_no_fs = backup_with_db_manager("APPEX_IT_ODOO19_URL", "it19", 19, false).await;
    assert_eq!(b19_no_fs.manifest.filestore_files, 0);
    assert!(b19_no_fs.size < b19.size);
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_db_manager_wrong_master_password() {
    let http = client();
    for (var, db) in [("APPEX_IT_ODOO15_URL", "it15"), ("APPEX_IT_ODOO19_URL", "it19")] {
        let base = url(var);
        let version = detect_version(&http, &base).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let request = BackupRequest {
            database: db.into(),
            include_filestore: true,
            dest_dir: dir.path().into(),
            file_stem: "x".into(),
        };
        let transport =
            DbManagerTransport::new(http.clone(), base, "not-the-master".into(), version, Duration::from_secs(120));
        let err = transport.run(&request, progress(), CancellationToken::new()).await.unwrap_err();
        eprintln!("{var} wrong master password: {err:?}");
        assert!(matches!(err, OdooError::AccessDenied(_)), "{var}: {err:?}");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 0);
    }
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_db_manager_unknown_database() {
    let http = client();
    for var in ["APPEX_IT_ODOO15_URL", "APPEX_IT_ODOO19_URL"] {
        let base = url(var);
        let version = detect_version(&http, &base).await.unwrap();
        let dir = tempfile::tempdir().unwrap();
        let request = BackupRequest {
            database: "nope".into(),
            include_filestore: true,
            dest_dir: dir.path().into(),
            file_stem: "x".into(),
        };
        let transport = DbManagerTransport::new(
            http.clone(),
            base,
            env("APPEX_IT_MASTER_PASSWORD").into(),
            version,
            Duration::from_secs(120),
        );
        let err = transport.run(&request, progress(), CancellationToken::new()).await.unwrap_err();
        eprintln!("{var} unknown database: {err:?}");
        assert!(matches!(err, OdooError::ServerBackupError(_)), "{var}: {err:?}");
    }
}

#[tokio::test]
#[ignore = "needs dev/odoo/run-integration.sh"]
async fn it_list_db_disabled() {
    let http = client();
    for (var, db, credentials) in
        [("APPEX_IT_ODOO15_NOLIST_URL", "it15", password()), ("APPEX_IT_ODOO19_NOLIST_URL", "it19", api_key())]
    {
        let base = url(var);
        let report = probe(
            &http,
            ProbeInput {
                base_url: base.clone(),
                database: Some(db.into()),
                credentials: Some(credentials),
                master_password: Some(env("APPEX_IT_MASTER_PASSWORD").into()),
                protocol_preference: ProtocolPreference::Auto,
            },
        )
        .await
        .unwrap();
        eprintln!("{var}: {}", serde_json::to_string(&report).unwrap());
        assert_eq!(report.auth, CheckStatus::Ok, "{var}");
        assert!(
            matches!(&report.db_manager, CheckStatus::Failed { code, .. } if code == "db_manager_disabled"),
            "{var}"
        );
        assert_eq!(report.recommended_transport, None);

        let dir = tempfile::tempdir().unwrap();
        let request = BackupRequest {
            database: db.into(),
            include_filestore: true,
            dest_dir: dir.path().into(),
            file_stem: "x".into(),
        };
        let transport = DbManagerTransport::new(
            http.clone(),
            base,
            env("APPEX_IT_MASTER_PASSWORD").into(),
            report.version.clone(),
            Duration::from_secs(120),
        );
        let err = transport.run(&request, progress(), CancellationToken::new()).await.unwrap_err();
        eprintln!("{var} backup with list_db=False: {err:?}");
        assert!(matches!(err, OdooError::DatabaseManagerDisabled), "{var}: {err:?}");
    }
}
