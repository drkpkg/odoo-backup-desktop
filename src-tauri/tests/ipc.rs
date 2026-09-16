//! IPC contract tests: invoke the real commands through Tauri's mock runtime, with the
//! app's real `tauri.conf.json` and capabilities (ACL), an in-memory keychain and
//! temporary directories.
//!
//! `backup_against_real_odoo` is ignored: run it with
//! `IT_COMMAND='cargo test -p appex-backup --test ipc -- --ignored --nocapture' dev/odoo/run-integration.sh`.

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use appex_backup_lib::{AppOptions, app_builder};
use appex_vault::{KdfParams, MemoryKeyStore};
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow};

struct TestApp {
    _app: App<MockRuntime>,
    window: WebviewWindow<MockRuntime>,
    _dir: tempfile::TempDir,
    downloads: std::path::PathBuf,
}

fn test_app(keychain: bool) -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let downloads = dir.path().join("downloads");
    let options = AppOptions {
        data_dir: Some(dir.path().join("data")),
        download_dir: Some(downloads.clone()),
        keystore: Some(Arc::new(MemoryKeyStore::new(keychain))),
        kdf: Some(KdfParams::insecure_for_tests()),
        disable_logging: true,
        disable_notifications: true,
    };
    let mut app = app_builder(mock_builder(), options).build(tauri::generate_context!(test = true)).unwrap();
    // A single iteration (not a loop) runs the setup hook and creates the "main" window
    // from tauri.conf.json.
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    let window = app.get_webview_window("main").expect("main window from tauri.conf.json");
    TestApp { _app: app, window, _dir: dir, downloads }
}

fn invoke(app: &TestApp, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(&app.window, request).map(|body| body.deserialize::<Value>().unwrap())
}

fn ok(app: &TestApp, cmd: &str, args: Value) -> Value {
    invoke(app, cmd, args).unwrap_or_else(|err| panic!("{cmd} failed: {err}"))
}

fn err_code(app: &TestApp, cmd: &str, args: Value) -> String {
    let err = invoke(app, cmd, args).expect_err(&format!("{cmd} should fail"));
    err.get("code").and_then(Value::as_str).unwrap_or_else(|| panic!("{cmd}: unexpected error shape {err}")).to_owned()
}

fn assert_no_secrets(value: &Value, secrets: &[&str]) {
    let text = value.to_string();
    for secret in secrets {
        assert!(!text.contains(secret), "response leaked a secret: {text}");
    }
}

fn wait_for_history(app: &TestApp, instance_id: &str, timeout: Duration) -> Value {
    let deadline = Instant::now() + timeout;
    loop {
        let history = ok(app, "list_history", json!({ "instanceId": instance_id }));
        if let Some(entry) = history.as_array().and_then(|list| list.first())
            && entry["status"] != "running"
        {
            return entry.clone();
        }
        assert!(Instant::now() < deadline, "backup did not finish in time: {history}");
        std::thread::sleep(Duration::from_millis(200));
    }
}

#[test]
fn vault_instances_settings_and_drive_contract() {
    let app = test_app(true);
    const SECRET: &str = "instance-api-key-123";
    const MASTER: &str = "server-master-pwd";

    let status = ok(&app, "get_app_status", json!({}));
    assert_eq!(status["vault"]["exists"], false);
    assert_eq!(status["vault"]["keychainAvailable"], true);
    assert_eq!(err_code(&app, "list_instances", json!({})), "vault_locked");

    assert_eq!(err_code(&app, "create_vault", json!({ "useKeychain": false })), "password_required");
    assert_eq!(
        err_code(&app, "create_vault", json!({ "useKeychain": true, "masterPassword": "short" })),
        "password_too_short"
    );
    let status = ok(&app, "create_vault", json!({ "useKeychain": true, "masterPassword": "correct horse battery" }));
    assert_eq!(status["vault"]["unlocked"], true);
    assert_eq!(status["vault"]["keychainEnabled"], true);
    assert_eq!(status["vault"]["passwordEnabled"], true);

    // Settings round trip (camelCase in and out, validation applied).
    let mut settings = ok(&app, "get_settings", json!({}));
    assert_eq!(settings["downloadDir"], app.downloads.to_string_lossy().as_ref());
    settings["maxConcurrentBackups"] = json!(9);
    let settings = ok(&app, "update_settings", json!({ "settings": settings }));
    assert_eq!(settings["maxConcurrentBackups"], 4);
    assert!(app.downloads.is_dir());

    // Instances: create, secrets are write-only.
    let input = json!({
        "name": "Cliente Uno", "url": "https://cliente1.nube.example/web?debug=1", "database": "",
        "login": "admin", "secretKind": "api_key", "secret": SECRET, "masterPassword": MASTER,
        "transport": "auto", "protocol": "auto", "includeFilestore": true, "uploadToDrive": false
    });
    let view = ok(&app, "save_instance", json!({ "input": input }));
    assert_no_secrets(&view, &[SECRET, MASTER]);
    assert_eq!(view["url"], "https://cliente1.nube.example/");
    assert_eq!(view["database"], "cliente1");
    assert_eq!(view["hasSecret"], true);
    assert_eq!(view["hasMasterPassword"], true);
    let id = view["id"].as_str().unwrap().to_owned();

    let mut duplicate = input.clone();
    duplicate["name"] = json!("cliente uno");
    assert_eq!(err_code(&app, "save_instance", json!({ "input": duplicate })), "duplicate_name");

    // Edit: absent secret keeps it, null master password removes it.
    let edit = json!({
        "id": id, "name": "Cliente Uno", "url": "https://cliente1.nube.example", "database": "cliente1",
        "login": "admin", "secretKind": "api_key", "masterPassword": null,
        "transport": "db_manager", "protocol": "json2", "includeFilestore": false, "uploadToDrive": true
    });
    let view = ok(&app, "save_instance", json!({ "input": edit }));
    assert_eq!(view["hasSecret"], true);
    assert_eq!(view["hasMasterPassword"], false);
    assert_eq!(view["transport"], "db_manager");
    assert_eq!(view["protocol"], "json2");

    let list = ok(&app, "list_instances", json!({}));
    assert_eq!(list.as_array().unwrap().len(), 1);
    assert_no_secrets(&list, &[SECRET, MASTER]);

    // Drive client: absent secret keeps, null removes; secrets never returned.
    let drive = ok(
        &app,
        "set_drive_client",
        json!({ "clientId": "abc.apps.googleusercontent.com", "clientSecret": "GOCSPX-x" }),
    );
    assert_eq!(drive["hasClientSecret"], true);
    assert_eq!(drive["configured"], true);
    assert_no_secrets(&drive, &["GOCSPX-x"]);
    let drive = ok(&app, "set_drive_client", json!({ "clientId": "abc.apps.googleusercontent.com" }));
    assert_eq!(drive["hasClientSecret"], true);
    let drive =
        ok(&app, "set_drive_client", json!({ "clientId": "abc.apps.googleusercontent.com", "clientSecret": null }));
    assert_eq!(drive["hasClientSecret"], false);
    assert_eq!(ok(&app, "get_drive_status", json!({}))["connected"], false);
    ok(&app, "cancel_drive_connect", json!({}));

    // Lock / unlock with keychain and with password.
    let status = ok(&app, "lock_vault", json!({}));
    assert_eq!(status["vault"]["unlocked"], false);
    assert_eq!(ok(&app, "unlock_vault", json!({}))["vault"]["unlocked"], true);
    ok(&app, "lock_vault", json!({}));
    assert_eq!(err_code(&app, "unlock_vault", json!({ "masterPassword": "wrong password" })), "vault_wrong_password");
    ok(&app, "unlock_vault", json!({ "masterPassword": "correct horse battery" }));
    assert_eq!(ok(&app, "list_instances", json!({})).as_array().unwrap().len(), 1);

    // Unknown ids and jobs.
    assert_eq!(err_code(&app, "cancel_backup", json!({ "jobId": "nope" })), "job_not_found");
    assert_eq!(err_code(&app, "delete_instance", json!({ "id": "nope" })), "not_found");
    assert_eq!(ok(&app, "list_active_jobs", json!({})), json!([]));

    ok(&app, "delete_instance", json!({ "id": id }));
    assert_eq!(ok(&app, "list_instances", json!({})), json!([]));
}

#[test]
fn backup_failure_is_recorded_and_probe_reports_connection_errors() {
    let app = test_app(false);
    ok(&app, "create_vault", json!({ "useKeychain": false, "masterPassword": "correct horse battery" }));
    assert_eq!(ok(&app, "get_app_status", json!({}))["vault"]["keychainEnabled"], false);

    let unreachable = "http://127.0.0.1:9";
    assert_eq!(
        err_code(
            &app,
            "probe_instance",
            json!({ "input": { "url": unreachable, "secretKind": "password", "protocol": "auto" } })
        ),
        "connection"
    );

    let view = ok(
        &app,
        "save_instance",
        json!({ "input": {
            "name": "Caída", "url": unreachable, "database": "db", "login": "admin",
            "secretKind": "password", "secret": "admin", "masterPassword": "master-password",
            "transport": "auto", "protocol": "auto", "includeFilestore": true, "uploadToDrive": false
        }}),
    );
    let id = view["id"].as_str().unwrap().to_owned();
    let job_id = ok(&app, "start_backup", json!({ "instanceId": id, "onEvent": "__CHANNEL__:7" }));
    assert!(job_id.as_str().is_some_and(|j| !j.is_empty()));

    let entry = wait_for_history(&app, &id, Duration::from_secs(60));
    assert_eq!(entry["status"], "failed");
    assert_eq!(entry["errorCode"], "connection");
    assert_eq!(entry["instanceName"], "Caída");
    assert_eq!(ok(&app, "list_active_jobs", json!({})), json!([]));
    assert_eq!(err_code(&app, "reveal_backup", json!({ "historyId": entry["id"] })), "not_found");

    // Invalid input shapes are rejected by the command layer, not by a panic.
    assert_eq!(
        err_code(
            &app,
            "save_instance",
            json!({ "input": { "name": "x", "url": "ftp://x", "database": "x", "secretKind": "password",
            "transport": "auto", "protocol": "auto", "includeFilestore": true, "uploadToDrive": false } })
        ),
        "invalid_url"
    );
}

#[test]
fn capabilities_block_plugin_commands_that_are_not_granted() {
    let app = test_app(true);
    // opener is used from Rust only; the webview must not be able to open URLs itself.
    let err = invoke(&app, "plugin:opener|open_url", json!({ "url": "https://example.com" })).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("not allowed"), "unexpected: {err}");
    let err = invoke(&app, "plugin:notification|notify", json!({ "options": { "title": "x" } })).unwrap_err();
    assert!(err.to_string().to_lowercase().contains("not allowed"), "unexpected: {err}");
}

fn it_env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is not set; run through dev/odoo/run-integration.sh"))
}

fn run_backup(app: &TestApp, instance: Value) -> Value {
    let view = ok(app, "save_instance", json!({ "input": instance }));
    let id = view["id"].as_str().unwrap().to_owned();
    let probe = ok(
        app,
        "probe_instance",
        json!({ "input": { "instanceId": id, "url": view["url"], "database": view["database"], "login": view["login"],
            "secretKind": view["secretKind"], "protocol": view["protocol"] } }),
    );
    println!("probe {}: {}", view["name"], probe);
    ok(app, "start_backup", json!({ "instanceId": id, "onEvent": "__CHANNEL__:1" }));
    let entry = wait_for_history(app, &id, Duration::from_secs(600));
    println!("backup {}: {}", view["name"], entry);
    entry
}

#[test]
#[ignore = "needs the dockerized Odoo stack (dev/odoo/run-integration.sh)"]
fn backup_against_real_odoo() {
    let app = test_app(true);
    ok(&app, "create_vault", json!({ "useKeychain": true }));
    let master = it_env("APPEX_IT_MASTER_PASSWORD");

    let cases = [
        ("Odoo 15", it_env("APPEX_IT_ODOO15_URL"), "it15", "password", it_env("APPEX_IT_ADMIN_PASSWORD"), "15.0"),
        ("Odoo 17", it_env("APPEX_IT_ODOO17_URL"), "it17", "password", it_env("APPEX_IT_ADMIN_PASSWORD"), "17.0"),
        ("Odoo 19", it_env("APPEX_IT_ODOO19_URL"), "it19", "api_key", it_env("APPEX_IT_ODOO19_API_KEY"), "19.0"),
    ];
    for (name, url, db, kind, secret, version) in cases {
        let entry = run_backup(
            &app,
            json!({ "name": name, "url": url, "database": db, "login": it_env("APPEX_IT_ADMIN_LOGIN"),
                "secretKind": kind, "secret": secret, "masterPassword": master,
                "transport": "auto", "protocol": "auto", "includeFilestore": true, "uploadToDrive": false }),
        );
        assert_eq!(entry["status"], "success", "{name}: {entry}");
        assert_eq!(entry["transport"], "db_manager");
        assert_eq!(entry["odooVersion"], version);
        let path = entry["filePath"].as_str().unwrap();
        assert!(Path::new(path).is_file() && path.ends_with(".zip"), "{path}");
        assert!(path.starts_with(app.downloads.to_string_lossy().as_ref()));
        assert_eq!(entry["sha256"].as_str().unwrap().len(), 64);
        assert!(entry["sizeBytes"].as_u64().unwrap() > 0);
    }

    // list_db = False: the database manager is blocked and there is no module either.
    let entry = run_backup(
        &app,
        json!({ "name": "Odoo 19 sin list_db", "url": it_env("APPEX_IT_ODOO19_NOLIST_URL"), "database": "it19",
            "login": it_env("APPEX_IT_ADMIN_LOGIN"), "secretKind": "api_key", "secret": it_env("APPEX_IT_ODOO19_API_KEY"),
            "masterPassword": master, "transport": "auto", "protocol": "auto", "includeFilestore": true, "uploadToDrive": false }),
    );
    assert_eq!(entry["status"], "failed");
    assert_eq!(entry["errorCode"], "db_manager_disabled");

    // Local retention keeps the configured number of backups per instance.
    let mut settings = ok(&app, "get_settings", json!({}));
    settings["keepLastLocal"] = json!(1);
    ok(&app, "update_settings", json!({ "settings": settings }));
    let instances = ok(&app, "list_instances", json!({}));
    let odoo17 = instances.as_array().unwrap().iter().find(|i| i["name"] == "Odoo 17").unwrap();
    let id = odoo17["id"].as_str().unwrap();
    std::thread::sleep(Duration::from_secs(1));
    ok(&app, "start_backup", json!({ "instanceId": id, "onEvent": "__CHANNEL__:2" }));
    let deadline = Instant::now() + Duration::from_secs(600);
    loop {
        let history = ok(&app, "list_history", json!({ "instanceId": id }));
        let list = history.as_array().unwrap();
        if list.len() == 2 && list[0]["status"] != "running" {
            assert_eq!(list[0]["status"], "success");
            let dir = Path::new(list[0]["filePath"].as_str().unwrap()).parent().unwrap().to_owned();
            let zips = std::fs::read_dir(dir)
                .unwrap()
                .filter(|e| e.as_ref().unwrap().path().extension().is_some_and(|ext| ext == "zip"));
            assert_eq!(zips.count(), 1, "retention should keep only the newest zip");
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(250));
    }
}
