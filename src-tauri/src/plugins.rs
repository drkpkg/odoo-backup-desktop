//! Plugin manager: discovery snapshot, config, settings, storage, assets and dev-mode watcher.
//! Contract: `docs/plugins.md`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use notify_debouncer_mini::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_mini::{Debouncer, new_debouncer};
use obd_plugins::{
    AssetError, DiscoveredPlugin, MenuLocation, PluginConfig, PluginConfigStore, PluginRoot, PluginSettingsStore,
    PluginSource, PluginStorage, RootKind, SettingsSchema,
};
use serde::Serialize;
use serde_json::{Map, Value};
use tauri::http;

use crate::error::{CommandError, CommandResult};

const WATCH_DEBOUNCE: Duration = Duration::from_millis(500);

/// Files of the plugin SDK served at `/_sdk/<name>`.
const SDK_FILES: &[(&str, &str, &str)] = &[
    ("obd-plugin.js", "text/javascript; charset=utf-8", include_str!("../../plugin-sdk/obd-plugin.js")),
    ("obd-plugin.css", "text/css; charset=utf-8", include_str!("../../plugin-sdk/obd-plugin.css")),
    ("obd-plugin.d.ts", "text/plain; charset=utf-8", include_str!("../../plugin-sdk/obd-plugin.d.ts")),
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PluginStatus {
    Enabled,
    Disabled,
    Error,
    Shadowed,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageView {
    pub id: String,
    pub title: String,
    pub path: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MenuView {
    pub id: String,
    pub location: MenuLocation,
    pub label: String,
    pub icon: Option<String>,
    pub page: Option<String>,
    pub window: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowView {
    pub id: String,
    pub title: String,
    pub path: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DestinationView {
    pub id: String,
    pub label: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionsView {
    pub network: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub id: String,
    pub name: String,
    pub version: Option<String>,
    pub description: Option<String>,
    pub author: Option<String>,
    pub homepage: Option<String>,
    pub source: PluginSource,
    pub path: String,
    pub status: PluginStatus,
    pub issues: Vec<obd_plugins::Issue>,
    pub base_url: String,
    pub revision: u64,
    pub pages: Vec<PageView>,
    pub menus: Vec<MenuView>,
    pub windows: Vec<WindowView>,
    pub has_settings: bool,
    pub destinations: Vec<DestinationView>,
    pub hooks: Vec<String>,
    pub permissions: PermissionsView,
    pub has_backend: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginConfigView {
    pub developer_mode: bool,
    pub dev_plugin_paths: Vec<String>,
    pub user_plugins_dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginSettingsView {
    pub schema: SettingsSchema,
    pub values: Map<String, Value>,
    pub secrets_set: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowContext {
    pub plugin_id: String,
    pub window_id: String,
    pub params: Value,
}

/// Base URL of a plugin's files for the current platform (always ends with `/`).
pub fn base_url(plugin_id: &str) -> String {
    if cfg!(any(windows, target_os = "android")) {
        format!("http://{}.localhost/{plugin_id}/", obd_plugins::PROTOCOL_SCHEME)
    } else {
        format!("{}://localhost/{plugin_id}/", obd_plugins::PROTOCOL_SCHEME)
    }
}

/// Window label for a plugin window: `plugin--<pluginId>--<windowId>`.
pub fn window_label(plugin_id: &str, window_id: &str) -> String {
    format!("plugin--{plugin_id}--{window_id}")
}

struct Snapshot {
    plugins: Vec<DiscoveredPlugin>,
    revision: u64,
}

pub struct PluginManager {
    builtin_dir: Option<PathBuf>,
    user_dir: PathBuf,
    config_store: PluginConfigStore,
    settings_store: PluginSettingsStore,
    storage: PluginStorage,
    config: RwLock<PluginConfig>,
    snapshot: RwLock<Snapshot>,
    window_contexts: Mutex<HashMap<String, WindowContext>>,
    watcher: Mutex<Option<Debouncer<RecommendedWatcher>>>,
}

impl PluginManager {
    pub fn new(builtin_dir: Option<PathBuf>, user_dir: PathBuf, data_dir: &Path) -> Arc<Self> {
        let config_store = PluginConfigStore::new(data_dir.join("plugins.json"));
        let config = config_store.load();
        let manager = Arc::new(Self {
            builtin_dir,
            user_dir,
            config_store,
            settings_store: PluginSettingsStore::new(data_dir.join("plugin-settings.json")),
            storage: PluginStorage::new(data_dir.join("plugin-data")),
            config: RwLock::new(config),
            snapshot: RwLock::new(Snapshot { plugins: Vec::new(), revision: 0 }),
            window_contexts: Mutex::new(HashMap::new()),
            watcher: Mutex::new(None),
        });
        manager.reload();
        manager
    }

    pub fn user_dir(&self) -> &Path {
        &self.user_dir
    }

    fn roots(&self, config: &PluginConfig) -> Vec<PluginRoot> {
        let mut roots = Vec::new();
        if let Some(dir) = &self.builtin_dir {
            roots.push(PluginRoot { path: dir.clone(), source: PluginSource::Builtin, kind: RootKind::Container });
        }
        roots.push(PluginRoot { path: self.user_dir.clone(), source: PluginSource::User, kind: RootKind::Container });
        for path in &config.dev_plugin_paths {
            roots.push(PluginRoot { path: PathBuf::from(path), source: PluginSource::Dev, kind: RootKind::Single });
        }
        roots
    }

    /// Rescans every root (blocking file I/O).
    pub fn reload(&self) {
        let config = self.config();
        let plugins = obd_plugins::discover(&self.roots(&config));
        if let Ok(mut snapshot) = self.snapshot.write() {
            snapshot.plugins = plugins;
            snapshot.revision += 1;
        }
        tracing::info!(count = self.views().len(), "plugins loaded");
    }

    pub fn config(&self) -> PluginConfig {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    pub fn config_view(&self) -> PluginConfigView {
        let config = self.config();
        PluginConfigView {
            developer_mode: config.developer_mode,
            dev_plugin_paths: config.dev_plugin_paths,
            user_plugins_dir: self.user_dir.to_string_lossy().into_owned(),
        }
    }

    /// Applies `f` to the config, persists it and rescans.
    pub fn update_config(&self, f: impl FnOnce(&mut PluginConfig) -> CommandResult<()>) -> CommandResult<()> {
        let mut config = self.config();
        f(&mut config)?;
        self.config_store.save(&config).map_err(plugin_error)?;
        if let Ok(mut current) = self.config.write() {
            *current = self.config_store.load();
        }
        self.reload();
        Ok(())
    }

    fn status(plugin: &DiscoveredPlugin, config: &PluginConfig) -> PluginStatus {
        if plugin.shadowed {
            PluginStatus::Shadowed
        } else if plugin.has_errors() || plugin.manifest.is_none() {
            PluginStatus::Error
        } else if config.disabled.iter().any(|id| *id == plugin.id()) {
            PluginStatus::Disabled
        } else {
            PluginStatus::Enabled
        }
    }

    pub fn views(&self) -> Vec<PluginView> {
        let config = self.config();
        let Ok(snapshot) = self.snapshot.read() else { return Vec::new() };
        snapshot.plugins.iter().map(|plugin| view(plugin, Self::status(plugin, &config), snapshot.revision)).collect()
    }

    /// The loaded (non-shadowed) plugin with `id`, whatever its status.
    fn find(&self, id: &str) -> Option<(DiscoveredPlugin, PluginStatus)> {
        let config = self.config();
        let snapshot = self.snapshot.read().ok()?;
        snapshot
            .plugins
            .iter()
            .find(|plugin| !plugin.shadowed && plugin.id() == id)
            .map(|plugin| (plugin.clone(), Self::status(plugin, &config)))
    }

    /// An enabled, valid plugin or a `plugin_*` error.
    pub fn enabled(&self, id: &str) -> CommandResult<DiscoveredPlugin> {
        match self.find(id) {
            None => Err(CommandError::new("plugin_not_found", format!("plugin {id:?} is not installed"))),
            Some((_, PluginStatus::Disabled)) => {
                Err(CommandError::new("plugin_disabled", format!("plugin {id:?} is disabled")))
            }
            Some((_, PluginStatus::Error | PluginStatus::Shadowed)) => {
                Err(CommandError::new("plugin_invalid", format!("plugin {id:?} has errors")))
            }
            Some((plugin, PluginStatus::Enabled)) => Ok(plugin),
        }
    }

    pub fn exists(&self, id: &str) -> bool {
        self.find(id).is_some()
    }

    pub fn settings_schema(&self, id: &str) -> CommandResult<SettingsSchema> {
        self.enabled(id)?
            .settings_schema
            .ok_or_else(|| CommandError::new("plugin_no_settings", format!("plugin {id:?} has no settings")))
    }

    pub fn stored_settings(&self, id: &str) -> CommandResult<Map<String, Value>> {
        self.settings_store.get(id).map_err(plugin_error)
    }

    pub fn store_settings(&self, id: &str, values: &Map<String, Value>) -> CommandResult<()> {
        self.settings_store.set(id, values).map_err(plugin_error)
    }

    pub fn storage_get(&self, id: &str, key: &str) -> CommandResult<Option<Value>> {
        self.enabled(id)?;
        self.storage.get(id, key).map_err(plugin_error)
    }

    pub fn storage_set(&self, id: &str, key: &str, value: Option<Value>) -> CommandResult<()> {
        self.enabled(id)?;
        self.storage.set(id, key, value).map_err(plugin_error)
    }

    pub fn set_window_context(&self, label: &str, context: WindowContext) {
        if let Ok(mut contexts) = self.window_contexts.lock() {
            contexts.insert(label.to_owned(), context);
        }
    }

    pub fn window_context(&self, label: &str) -> Option<WindowContext> {
        self.window_contexts.lock().ok().and_then(|contexts| contexts.get(label).cloned())
    }

    pub fn forget_window(&self, label: &str) {
        if let Ok(mut contexts) = self.window_contexts.lock() {
            contexts.remove(label);
        }
    }

    /// Serves `/<plugin-id>/<path>` and `/_sdk/<file>` for the `obd-plugin` protocol.
    pub fn respond(&self, uri_path: &str) -> http::Response<Vec<u8>> {
        let Some((first, rest)) = obd_plugins::split_protocol_path(uri_path) else {
            return error_response(http::StatusCode::NOT_FOUND);
        };
        if first == obd_plugins::SDK_SEGMENT {
            return match SDK_FILES.iter().find(|(name, _, _)| *name == rest) {
                Some((_, mime, body)) => file_response(mime, body.as_bytes().to_vec(), None),
                None => error_response(http::StatusCode::NOT_FOUND),
            };
        }
        let Ok(plugin) = self.enabled(&first) else {
            return error_response(http::StatusCode::NOT_FOUND);
        };
        let asset = match obd_plugins::resolve_asset(&plugin.dir, &rest) {
            Ok(asset) => asset,
            Err(AssetError::NotFound) => return error_response(http::StatusCode::NOT_FOUND),
            Err(AssetError::Forbidden) => return error_response(http::StatusCode::FORBIDDEN),
        };
        match std::fs::read(&asset.path) {
            Ok(body) => {
                let csp = asset.mime.starts_with("text/html").then(|| {
                    let network = plugin.manifest.as_ref().map(|m| m.permissions.network.clone()).unwrap_or_default();
                    obd_plugins::page_csp(&network)
                });
                file_response(asset.mime, body, csp)
            }
            Err(err) => {
                tracing::warn!(plugin = %first, error = %err, "could not read plugin asset");
                error_response(http::StatusCode::NOT_FOUND)
            }
        }
    }

    /// Starts or stops the dev-mode watcher according to the config. `on_change` runs after a
    /// rescan triggered by file changes.
    pub fn sync_watcher(self: &Arc<Self>, on_change: impl Fn() + Send + 'static) {
        let Ok(mut slot) = self.watcher.lock() else { return };
        *slot = None;
        let config = self.config();
        if !config.developer_mode {
            return;
        }
        let weak = Arc::downgrade(self);
        let debouncer = new_debouncer(WATCH_DEBOUNCE, move |result: notify_debouncer_mini::DebounceEventResult| {
            if result.is_err() {
                return;
            }
            if let Some(manager) = weak.upgrade() {
                manager.reload();
                on_change();
            }
        });
        let mut debouncer = match debouncer {
            Ok(debouncer) => debouncer,
            Err(err) => {
                tracing::warn!(error = %err, "plugin watcher could not start");
                return;
            }
        };
        let mut watched = HashSet::new();
        let _ = std::fs::create_dir_all(&self.user_dir);
        let mut paths = vec![self.user_dir.clone()];
        paths.extend(config.dev_plugin_paths.iter().map(PathBuf::from));
        for path in paths {
            if path.is_dir()
                && watched.insert(path.clone())
                && let Err(err) = debouncer.watcher().watch(&path, RecursiveMode::Recursive)
            {
                tracing::warn!(path = %path.display(), error = %err, "could not watch plugin folder");
            }
        }
        *slot = Some(debouncer);
    }
}

fn view(plugin: &DiscoveredPlugin, status: PluginStatus, revision: u64) -> PluginView {
    let id = plugin.id();
    let manifest = plugin.manifest.as_ref();
    let contributes = manifest.map(|m| &m.contributes);
    PluginView {
        name: manifest.map(|m| m.name.clone()).unwrap_or_else(|| id.clone()),
        version: manifest.map(|m| m.version.clone()),
        description: manifest.and_then(|m| m.description.clone()),
        author: manifest.and_then(|m| m.author.clone()),
        homepage: manifest.and_then(|m| m.homepage.clone()),
        source: plugin.source,
        path: plugin.dir.to_string_lossy().into_owned(),
        status,
        issues: plugin.issues.clone(),
        base_url: base_url(&id),
        revision,
        pages: contributes
            .map(|c| {
                c.pages
                    .iter()
                    .map(|p| PageView { id: p.id.clone(), title: p.title.clone(), path: p.path.clone() })
                    .collect()
            })
            .unwrap_or_default(),
        menus: contributes
            .map(|c| {
                c.menus
                    .iter()
                    .map(|m| MenuView {
                        id: m.id.clone(),
                        location: m.location,
                        label: m.label.clone(),
                        icon: m.icon.clone(),
                        page: m.page.clone(),
                        window: m.window.clone(),
                    })
                    .collect()
            })
            .unwrap_or_default(),
        windows: contributes
            .map(|c| {
                c.windows
                    .iter()
                    .map(|w| WindowView {
                        id: w.id.clone(),
                        title: w.title.clone(),
                        path: w.path.clone(),
                        width: w.width,
                        height: w.height,
                    })
                    .collect()
            })
            .unwrap_or_default(),
        has_settings: plugin.settings_schema.is_some(),
        destinations: contributes
            .map(|c| {
                c.destinations.iter().map(|d| DestinationView { id: d.id.clone(), label: d.label.clone() }).collect()
            })
            .unwrap_or_default(),
        hooks: contributes.map(|c| c.hooks.clone()).unwrap_or_default(),
        permissions: PermissionsView { network: manifest.map(|m| m.permissions.network.clone()).unwrap_or_default() },
        has_backend: manifest.is_some_and(|m| m.backend.is_some()),
        id,
    }
}

fn plugin_error(err: obd_plugins::PluginError) -> CommandError {
    CommandError::new(err.code(), err.to_string())
}

fn file_response(mime: &str, body: Vec<u8>, csp: Option<String>) -> http::Response<Vec<u8>> {
    let mut builder = http::Response::builder()
        .status(http::StatusCode::OK)
        .header(http::header::CONTENT_TYPE, mime)
        // Plugin pages run in sandboxed iframes (opaque origin): module scripts need CORS.
        .header(http::header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
        .header(http::header::CACHE_CONTROL, "no-cache")
        .header("X-Content-Type-Options", "nosniff");
    if let Some(csp) = csp {
        builder = builder.header(http::header::CONTENT_SECURITY_POLICY, csp);
    }
    builder.body(body).unwrap_or_else(|_| error_response(http::StatusCode::INTERNAL_SERVER_ERROR))
}

fn error_response(status: http::StatusCode) -> http::Response<Vec<u8>> {
    let mut response = http::Response::new(Vec::new());
    *response.status_mut() = status;
    response
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    fn manager_with_plugin() -> (tempfile::TempDir, Arc<PluginManager>) {
        let dir = tempfile::tempdir().unwrap();
        let plugin = dir.path().join("user/hello");
        write(
            &plugin.join("plugin.json"),
            r#"{"id":"hello","name":"Hello","version":"0.1.0","apiVersion":1,
                "contributes":{"pages":[{"id":"main","title":"Main","path":"ui/index.html"}]},
                "permissions":{"network":["api.example.com"]}}"#,
        );
        write(&plugin.join("ui/index.html"), "<h1>hello</h1>");
        write(&plugin.join("ui/app.js"), "export const x = 1;");
        write(&dir.path().join("user/secret.txt"), "outside");
        let manager = PluginManager::new(None, dir.path().join("user"), &dir.path().join("data"));
        (dir, manager)
    }

    #[test]
    fn serves_plugin_files_with_csp_and_cors() {
        let (_dir, manager) = manager_with_plugin();
        let response = manager.respond("/hello/ui/index.html");
        assert_eq!(response.status(), http::StatusCode::OK);
        assert!(response.headers()[http::header::CONTENT_TYPE].to_str().unwrap().starts_with("text/html"));
        let csp = response.headers()[http::header::CONTENT_SECURITY_POLICY].to_str().unwrap();
        assert!(csp.contains("https://api.example.com"), "{csp}");
        assert_eq!(response.headers()[http::header::ACCESS_CONTROL_ALLOW_ORIGIN], "*");
        assert_eq!(response.body(), b"<h1>hello</h1>");

        let js = manager.respond("/hello/ui/app.js");
        assert_eq!(js.status(), http::StatusCode::OK);
        assert!(js.headers().get(http::header::CONTENT_SECURITY_POLICY).is_none());
    }

    #[test]
    fn rejects_traversal_unknown_and_disabled_plugins() {
        let (_dir, manager) = manager_with_plugin();
        assert_ne!(manager.respond("/hello/../secret.txt").status(), http::StatusCode::OK);
        assert_ne!(manager.respond("/hello/%2e%2e/secret.txt").status(), http::StatusCode::OK);
        assert_eq!(manager.respond("/hello/ui/missing.js").status(), http::StatusCode::NOT_FOUND);
        assert_eq!(manager.respond("/nope/ui/index.html").status(), http::StatusCode::NOT_FOUND);
        assert_eq!(manager.respond("/").status(), http::StatusCode::NOT_FOUND);

        manager
            .update_config(|config| {
                config.disabled.push("hello".into());
                Ok(())
            })
            .unwrap();
        assert_eq!(manager.respond("/hello/ui/index.html").status(), http::StatusCode::NOT_FOUND);
        assert_eq!(manager.views()[0].status, PluginStatus::Disabled);
    }

    #[test]
    fn serves_the_sdk() {
        let (_dir, manager) = manager_with_plugin();
        let response = manager.respond("/_sdk/obd-plugin.js");
        assert_eq!(response.status(), http::StatusCode::OK);
        assert!(response.headers()[http::header::CONTENT_TYPE].to_str().unwrap().starts_with("text/javascript"));
        assert!(!response.body().is_empty());
        assert_eq!(manager.respond("/_sdk/other.js").status(), http::StatusCode::NOT_FOUND);
    }

    #[test]
    fn labels_and_urls() {
        assert_eq!(window_label("hello-obd", "detail"), "plugin--hello-obd--detail");
        let url = base_url("hello-obd");
        assert!(url.ends_with("/hello-obd/"));
        assert!(url.contains("obd-plugin"));
    }
}
