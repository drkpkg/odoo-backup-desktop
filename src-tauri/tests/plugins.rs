//! Plugin commands through Tauri's mock runtime with the real config and capabilities
//! (`default.json` for `main`, `plugin-windows.json` for `plugin--*`).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use obd_vault::{KdfParams, MemoryKeyStore};
use odoo_backup_desktop_lib::{AppOptions, app_builder};
use serde_json::{Value, json};
use tauri::test::{INVOKE_KEY, MockRuntime, get_ipc_response, mock_builder};
use tauri::webview::InvokeRequest;
use tauri::{App, Manager, WebviewWindow};

struct TestApp {
    app: App<MockRuntime>,
    _dir: tempfile::TempDir,
    user_plugins: PathBuf,
    root: PathBuf,
}

fn write(path: &Path, content: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, content).unwrap();
}

const SCHEMA: &str = r#"{
  "type": "object",
  "properties": {
    "endpoint": { "type": "string", "title": "URL", "format": "url", "default": "https://api.example.com" },
    "token": { "type": "string", "title": "Token", "secret": true, "minLength": 10 },
    "retries": { "type": "integer", "minimum": 0, "maximum": 10, "default": 3 },
    "mode": { "type": "string", "enum": ["fast", "safe"], "default": "safe" },
    "notify": { "type": "boolean", "default": true }
  },
  "required": ["endpoint"]
}"#;

fn write_hello_plugin(dir: &Path) {
    write(
        &dir.join("plugin.json"),
        r#"{
          "id": "hello", "name": "Hello", "version": "0.1.0", "apiVersion": 1,
          "contributes": {
            "pages": [{ "id": "main", "title": "Hello", "path": "ui/index.html" }],
            "menus": [
              { "id": "side", "location": "sidebar", "label": "Hello", "icon": "sparkles", "page": "main" },
              { "id": "inst", "location": "instance_actions", "label": "Detail", "window": "detail" }
            ],
            "windows": [{ "id": "detail", "title": "Detail", "path": "ui/detail.html", "width": 640, "height": 480 }],
            "settings": "settings.schema.json"
          }
        }"#,
    );
    write(&dir.join("ui/index.html"), "<h1>hello</h1>");
    write(&dir.join("ui/detail.html"), "<h1>detail</h1>");
    write(&dir.join("settings.schema.json"), SCHEMA);
}

fn test_app() -> TestApp {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().to_path_buf();
    let user_plugins = root.join("plugins");
    write_hello_plugin(&user_plugins.join("hello"));
    write(&user_plugins.join("broken/plugin.json"), "{ not json");

    let options = AppOptions {
        data_dir: Some(root.join("data")),
        download_dir: Some(root.join("downloads")),
        keystore: Some(Arc::new(MemoryKeyStore::new(true))),
        kdf: Some(KdfParams::insecure_for_tests()),
        disable_logging: true,
        disable_notifications: true,
        builtin_plugins_dir: Some(root.join("builtin")),
        user_plugins_dir: Some(user_plugins.clone()),
    };
    let mut app = app_builder(mock_builder(), options).build(tauri::generate_context!(test = true)).unwrap();
    #[allow(deprecated)]
    app.run_iteration(|_, _| {});
    TestApp { app, _dir: dir, user_plugins, root }
}

fn window(app: &TestApp, label: &str) -> WebviewWindow<MockRuntime> {
    app.app.get_webview_window(label).unwrap_or_else(|| panic!("window {label} not found"))
}

fn invoke_in(app: &TestApp, label: &str, cmd: &str, args: Value) -> Result<Value, Value> {
    let request = InvokeRequest {
        cmd: cmd.into(),
        callback: tauri::ipc::CallbackFn(0),
        error: tauri::ipc::CallbackFn(1),
        url: "tauri://localhost".parse().unwrap(),
        body: tauri::ipc::InvokeBody::Json(args),
        headers: Default::default(),
        invoke_key: INVOKE_KEY.to_string(),
    };
    get_ipc_response(&window(app, label), request).map(|body| body.deserialize::<Value>().unwrap())
}

fn ok(app: &TestApp, cmd: &str, args: Value) -> Value {
    invoke_in(app, "main", cmd, args).unwrap_or_else(|err| panic!("{cmd} failed: {err}"))
}

fn err_code(app: &TestApp, cmd: &str, args: Value) -> String {
    let err = invoke_in(app, "main", cmd, args).expect_err(&format!("{cmd} should fail"));
    err.get("code").and_then(Value::as_str).unwrap_or_else(|| panic!("{cmd}: unexpected error {err}")).to_owned()
}

fn plugin<'a>(list: &'a Value, id: &str) -> &'a Value {
    list.as_array().unwrap().iter().find(|p| p["id"] == id).unwrap_or_else(|| panic!("plugin {id} missing: {list}"))
}

#[test]
fn discovery_views_and_enable_toggle() {
    let app = test_app();
    let list = ok(&app, "list_plugins", json!({}));

    let hello = plugin(&list, "hello");
    assert_eq!(hello["status"], "enabled");
    assert_eq!(hello["source"], "user");
    assert!(hello["baseUrl"].as_str().unwrap().ends_with("/hello/"));
    assert_eq!(hello["pages"][0]["path"], "ui/index.html");
    assert_eq!(hello["menus"][1]["location"], "instance_actions");
    assert_eq!(hello["windows"][0]["width"], 640);
    assert_eq!(hello["hasSettings"], true);
    let revision = hello["revision"].as_u64().unwrap();

    let broken = plugin(&list, "broken");
    assert_eq!(broken["status"], "error");
    assert_eq!(broken["issues"][0]["code"], "manifest_invalid");

    let list = ok(&app, "set_plugin_enabled", json!({ "pluginId": "hello", "enabled": false }));
    assert_eq!(plugin(&list, "hello")["status"], "disabled");
    assert_eq!(err_code(&app, "plugin_storage_get", json!({ "pluginId": "hello", "key": "k" })), "plugin_disabled");
    let list = ok(&app, "set_plugin_enabled", json!({ "pluginId": "hello", "enabled": true }));
    assert_eq!(plugin(&list, "hello")["status"], "enabled");
    assert!(plugin(&list, "hello")["revision"].as_u64().unwrap() > revision);
    assert_eq!(
        err_code(&app, "set_plugin_enabled", json!({ "pluginId": "nope", "enabled": true })),
        "plugin_not_found"
    );

    // A new plugin copied into the folder shows up after a reload.
    let mut copy = std::fs::read_to_string(app.user_plugins.join("hello/plugin.json")).unwrap();
    copy = copy.replace("\"id\": \"hello\"", "\"id\": \"hello-two\"");
    write(&app.user_plugins.join("hello-two/plugin.json"), &copy);
    write(&app.user_plugins.join("hello-two/ui/index.html"), "x");
    write(&app.user_plugins.join("hello-two/ui/detail.html"), "x");
    write(&app.user_plugins.join("hello-two/settings.schema.json"), SCHEMA);
    let list = ok(&app, "reload_plugins", json!({}));
    assert_eq!(plugin(&list, "hello-two")["status"], "enabled");
}

#[test]
fn settings_with_secrets_in_the_vault() {
    let app = test_app();
    // Secrets need the vault.
    assert_eq!(err_code(&app, "get_plugin_settings", json!({ "pluginId": "hello" })), "vault_locked");
    ok(&app, "create_vault", json!({ "useKeychain": true }));

    let settings = ok(&app, "get_plugin_settings", json!({ "pluginId": "hello" }));
    assert_eq!(settings["values"]["endpoint"], "https://api.example.com");
    assert_eq!(settings["values"]["retries"], 3);
    assert_eq!(settings["secretsSet"], json!([]));
    assert_eq!(settings["schema"]["propertyOrder"], json!(["endpoint", "token", "retries", "mode", "notify"]));

    let err = invoke_in(
        &app,
        "main",
        "save_plugin_settings",
        json!({ "pluginId": "hello", "values": { "retries": 99, "token": "short" } }),
    )
    .unwrap_err();
    assert_eq!(err["code"], "plugin_settings_invalid");
    let fields: Vec<Value> = serde_json::from_str(err["message"].as_str().unwrap()).unwrap();
    let names: Vec<&str> = fields.iter().map(|f| f["field"].as_str().unwrap()).collect();
    assert!(names.contains(&"retries") && names.contains(&"token"), "{fields:?}");

    const TOKEN: &str = "super-secret-token-123";
    let saved = ok(
        &app,
        "save_plugin_settings",
        json!({ "pluginId": "hello", "values": { "retries": 5, "mode": "fast", "token": TOKEN } }),
    );
    assert_eq!(saved["values"]["retries"], 5);
    assert_eq!(saved["values"]["mode"], "fast");
    assert_eq!(saved["secretsSet"], json!(["token"]));
    assert!(!saved.to_string().contains(TOKEN));
    // The secret is not written to the plain settings file.
    let plain = std::fs::read_to_string(app.root.join("data/plugin-settings.json")).unwrap();
    assert!(!plain.contains(TOKEN) && plain.contains("fast"));

    // Absent secret keeps it; null removes it.
    let kept = ok(&app, "save_plugin_settings", json!({ "pluginId": "hello", "values": { "notify": false } }));
    assert_eq!(kept["secretsSet"], json!(["token"]));
    assert_eq!(kept["values"]["notify"], false);
    let removed = ok(&app, "save_plugin_settings", json!({ "pluginId": "hello", "values": { "token": null } }));
    assert_eq!(removed["secretsSet"], json!([]));

    assert_eq!(err_code(&app, "get_plugin_settings", json!({ "pluginId": "broken" })), "plugin_invalid");
}

#[test]
fn storage_config_dev_folders_and_windows() {
    let app = test_app();
    ok(&app, "create_vault", json!({ "useKeychain": true }));

    // Key/value storage.
    assert_eq!(ok(&app, "plugin_storage_get", json!({ "pluginId": "hello", "key": "count" })), Value::Null);
    ok(&app, "plugin_storage_set", json!({ "pluginId": "hello", "key": "count", "value": { "n": 2 } }));
    assert_eq!(ok(&app, "plugin_storage_get", json!({ "pluginId": "hello", "key": "count" })), json!({ "n": 2 }));
    ok(&app, "plugin_storage_set", json!({ "pluginId": "hello", "key": "count", "value": null }));
    assert_eq!(ok(&app, "plugin_storage_get", json!({ "pluginId": "hello", "key": "count" })), Value::Null);
    assert_eq!(
        err_code(&app, "plugin_storage_set", json!({ "pluginId": "hello", "key": "bad key!", "value": 1 })),
        "plugin_storage_key_invalid"
    );

    // Developer mode and "load from folder".
    let config = ok(&app, "get_plugin_config", json!({}));
    assert_eq!(config["developerMode"], false);
    assert_eq!(config["userPluginsDir"], app.user_plugins.to_string_lossy().as_ref());
    let dev_dir = app.root.join("elsewhere/dev-hello");
    write_hello_plugin(&dev_dir);
    let manifest = std::fs::read_to_string(dev_dir.join("plugin.json")).unwrap().replace("\"hello\"", "\"dev-hello\"");
    write(&dev_dir.join("plugin.json"), &manifest);
    assert_eq!(
        err_code(&app, "add_dev_plugin", json!({ "path": app.root.join("missing").to_string_lossy() })),
        "plugin_path_invalid"
    );
    let config = ok(&app, "set_developer_mode", json!({ "enabled": true }));
    assert_eq!(config["developerMode"], true);
    let config = ok(&app, "add_dev_plugin", json!({ "path": dev_dir.to_string_lossy() }));
    let dev_path = config["devPluginPaths"][0].as_str().unwrap().to_owned();
    let list = ok(&app, "list_plugins", json!({}));
    assert_eq!(plugin(&list, "dev-hello")["source"], "dev");
    ok(&app, "remove_dev_plugin", json!({ "path": dev_path }));
    let list = ok(&app, "list_plugins", json!({}));
    assert!(list.as_array().unwrap().iter().all(|p| p["id"] != "dev-hello"));
    ok(&app, "set_developer_mode", json!({ "enabled": false }));

    // Plugin windows.
    assert_eq!(
        err_code(&app, "open_plugin_window", json!({ "pluginId": "hello", "windowId": "nope" })),
        "plugin_window_not_found"
    );
    assert_eq!(err_code(&app, "get_plugin_window_context", json!({})), "plugin_window_not_found");
    ok(
        &app,
        "open_plugin_window",
        json!({ "pluginId": "hello", "windowId": "detail", "params": { "instanceId": "i-1" } }),
    );
    let label = "plugin--hello--detail";
    let context = invoke_in(&app, label, "get_plugin_window_context", json!({})).unwrap();
    assert_eq!(context, json!({ "pluginId": "hello", "windowId": "detail", "params": { "instanceId": "i-1" } }));

    // The plugin window capability allows the bridge commands only.
    assert!(invoke_in(&app, label, "list_instances", json!({})).is_ok());
    assert!(invoke_in(&app, label, "plugin_storage_get", json!({ "pluginId": "hello", "key": "x" })).is_ok());
    for forbidden in ["lock_vault", "delete_instance", "start_backup", "set_plugin_enabled", "get_settings"] {
        let err = invoke_in(&app, label, forbidden, json!({})).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("not allowed"), "{forbidden}: {err}");
    }
}

fn repo_path(relative: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(relative)
}

fn assert_loads_cleanly(dir: &Path) {
    let plugin = obd_plugins::load_plugin(dir, obd_plugins::PluginSource::Dev);
    assert!(plugin.manifest.is_some(), "{}: {:?}", dir.display(), plugin.issues);
    assert!(plugin.issues.is_empty(), "{}: {:?}", dir.display(), plugin.issues);
}

#[test]
fn shipped_example_plugin_is_valid() {
    let plugin = obd_plugins::load_plugin(&repo_path("examples/plugins/hello-obd"), obd_plugins::PluginSource::Dev);
    assert!(plugin.settings_schema.is_some());
    assert_loads_cleanly(&repo_path("examples/plugins/hello-obd"));
}

#[cfg(unix)]
#[test]
fn generated_plugin_from_template_is_valid() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("mi-plugin");
    let status = std::process::Command::new("bash")
        .arg(repo_path("scripts/new-plugin.sh"))
        .args(["mi-plugin", "Mi \"plugin\" & <prueba>"])
        .arg(&target)
        .current_dir(repo_path(""))
        .output()
        .unwrap();
    assert!(status.status.success(), "{}", String::from_utf8_lossy(&status.stderr));
    assert_loads_cleanly(&target);
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(target.join("plugin.json")).unwrap()).unwrap();
    assert_eq!(manifest["id"], "mi-plugin");
    assert_eq!(manifest["name"], "Mi \"plugin\" & <prueba>");
}
