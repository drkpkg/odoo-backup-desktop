//! Plugin commands (see "Comandos de la app" in `docs/plugins.md`).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Map, Value};
use tauri::{AppHandle, Emitter, Manager, Runtime, State, WebviewUrl, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;

use crate::error::{CommandError, CommandResult};
use crate::models::deserialize_patch;
use crate::plugins::{PluginConfigView, PluginManager, PluginSettingsView, PluginView, WindowContext, window_label};
use crate::state::AppState;

#[derive(Clone, serde::Serialize)]
struct PluginsChangedPayload {
    reason: &'static str,
}

pub fn emit_plugins_changed<R: Runtime>(app: &AppHandle<R>, reason: &'static str) {
    if let Err(err) = app.emit("plugins-changed", PluginsChangedPayload { reason }) {
        tracing::debug!(error = %err, "could not emit plugins-changed");
    }
}

/// (Re)starts the dev-mode watcher; file changes rescan and notify the UI.
pub fn sync_watcher<R: Runtime>(app: &AppHandle<R>, manager: &Arc<PluginManager>) {
    let handle = app.clone();
    manager.sync_watcher(move || emit_plugins_changed(&handle, "watch"));
}

async fn blocking<T: Send + 'static>(
    manager: &Arc<PluginManager>,
    f: impl FnOnce(&PluginManager) -> CommandResult<T> + Send + 'static,
) -> CommandResult<T> {
    let manager = Arc::clone(manager);
    tokio::task::spawn_blocking(move || f(&manager)).await?
}

#[tauri::command]
pub async fn list_plugins(state: State<'_, AppState>) -> CommandResult<Vec<PluginView>> {
    Ok(state.plugins.views())
}

#[tauri::command]
pub async fn reload_plugins<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
) -> CommandResult<Vec<PluginView>> {
    let views = blocking(&state.plugins, |manager| {
        manager.reload();
        Ok(manager.views())
    })
    .await?;
    emit_plugins_changed(&app, "reload");
    Ok(views)
}

#[tauri::command]
pub async fn set_plugin_enabled<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    plugin_id: String,
    enabled: bool,
) -> CommandResult<Vec<PluginView>> {
    let views = blocking(&state.plugins, move |manager| {
        if !manager.exists(&plugin_id) {
            return Err(CommandError::new("plugin_not_found", format!("plugin {plugin_id:?} is not installed")));
        }
        manager.update_config(|config| {
            config.disabled.retain(|id| *id != plugin_id);
            if !enabled {
                config.disabled.push(plugin_id.clone());
            }
            Ok(())
        })?;
        Ok(manager.views())
    })
    .await?;
    emit_plugins_changed(&app, "config");
    Ok(views)
}

#[tauri::command]
pub async fn get_plugin_config(state: State<'_, AppState>) -> CommandResult<PluginConfigView> {
    Ok(state.plugins.config_view())
}

#[tauri::command]
pub async fn set_developer_mode<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    enabled: bool,
) -> CommandResult<PluginConfigView> {
    let view = blocking(&state.plugins, move |manager| {
        manager.update_config(|config| {
            config.developer_mode = enabled;
            Ok(())
        })?;
        Ok(manager.config_view())
    })
    .await?;
    sync_watcher(&app, &state.plugins);
    emit_plugins_changed(&app, "config");
    Ok(view)
}

#[tauri::command]
pub async fn add_dev_plugin<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<PluginConfigView> {
    let view = blocking(&state.plugins, move |manager| {
        let dir = PathBuf::from(path.trim());
        if !dir.is_absolute() || !dir.is_dir() {
            return Err(CommandError::new(
                "plugin_path_invalid",
                "the plugin folder must be an existing absolute path",
            ));
        }
        let dir = dir.canonicalize().map_err(|err| CommandError::new("plugin_path_invalid", err.to_string()))?;
        if !dir.join(obd_plugins::MANIFEST_FILE).is_file() {
            return Err(CommandError::new("plugin_path_invalid", "the folder does not contain plugin.json"));
        }
        let dir = dir.to_string_lossy().into_owned();
        manager.update_config(|config| {
            if !config.dev_plugin_paths.contains(&dir) {
                config.dev_plugin_paths.push(dir);
            }
            Ok(())
        })?;
        Ok(manager.config_view())
    })
    .await?;
    sync_watcher(&app, &state.plugins);
    emit_plugins_changed(&app, "config");
    Ok(view)
}

#[tauri::command]
pub async fn remove_dev_plugin<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    path: String,
) -> CommandResult<PluginConfigView> {
    let view = blocking(&state.plugins, move |manager| {
        manager.update_config(|config| {
            config.dev_plugin_paths.retain(|existing| *existing != path);
            Ok(())
        })?;
        Ok(manager.config_view())
    })
    .await?;
    sync_watcher(&app, &state.plugins);
    emit_plugins_changed(&app, "config");
    Ok(view)
}

#[tauri::command]
pub async fn open_plugins_folder<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CommandResult<()> {
    let dir = state.plugins.user_dir().to_path_buf();
    std::fs::create_dir_all(&dir)?;
    app.opener().open_path(dir.to_string_lossy(), None::<&str>).map_err(|err| CommandError::internal(err.to_string()))
}

fn settings_view(state: &AppState, plugin_id: &str) -> CommandResult<PluginSettingsView> {
    let schema = state.plugins.settings_schema(plugin_id)?;
    let mut values = schema.defaults();
    values.extend(state.plugins.stored_settings(plugin_id)?);
    let secret_keys: HashSet<&str> = schema.secret_keys().into_iter().collect();
    values.retain(|key, _| schema.properties.contains_key(key) && !secret_keys.contains(key.as_str()));
    let secrets_set = state.vault.read(|data| {
        data.plugin_secrets
            .get(plugin_id)
            .map(|secrets| {
                secrets
                    .iter()
                    .filter(|(key, value)| secret_keys.contains(key.as_str()) && !value.is_empty())
                    .map(|(key, _)| key.clone())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default()
    })?;
    Ok(PluginSettingsView { schema, values, secrets_set })
}

#[tauri::command]
pub async fn get_plugin_settings(state: State<'_, AppState>, plugin_id: String) -> CommandResult<PluginSettingsView> {
    settings_view(&state, &plugin_id)
}

#[tauri::command]
pub async fn save_plugin_settings(
    state: State<'_, AppState>,
    plugin_id: String,
    values: Map<String, Value>,
) -> CommandResult<PluginSettingsView> {
    let schema = state.plugins.settings_schema(&plugin_id)?;
    let current = state.plugins.stored_settings(&plugin_id)?;
    let existing: HashSet<String> = state.vault.read(|data| {
        data.plugin_secrets
            .get(&plugin_id)
            .map(|secrets| secrets.iter().filter(|(_, v)| !v.is_empty()).map(|(k, _)| k.clone()).collect())
            .unwrap_or_default()
    })?;
    let validated = schema.validate_values(&values, &current, &existing).map_err(|errors| {
        CommandError::new("plugin_settings_invalid", serde_json::to_string(&errors).unwrap_or_default())
    })?;

    let secrets = validated.secrets.clone();
    if !secrets.set.is_empty() || !secrets.remove.is_empty() {
        let id = plugin_id.clone();
        state
            .vault
            .update(move |data| {
                let entry = data.plugin_secrets.entry(id.clone()).or_default();
                for (key, value) in secrets.set {
                    entry.insert(key, obd_vault::SecretField::new(value));
                }
                for key in secrets.remove {
                    entry.remove(&key);
                }
                if entry.is_empty() {
                    data.plugin_secrets.remove(&id);
                }
                Ok(())
            })
            .await?;
    }
    let manager = Arc::clone(&state.plugins);
    let id = plugin_id.clone();
    let values = validated.values;
    tokio::task::spawn_blocking(move || manager.store_settings(&id, &values)).await??;
    settings_view(&state, &plugin_id)
}

#[tauri::command]
pub async fn plugin_storage_get(state: State<'_, AppState>, plugin_id: String, key: String) -> CommandResult<Value> {
    let value = blocking(&state.plugins, move |manager| manager.storage_get(&plugin_id, &key)).await?;
    Ok(value.unwrap_or(Value::Null))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct StorageSetArgs {
    plugin_id: String,
    key: String,
    #[serde(default, deserialize_with = "deserialize_patch")]
    value: Option<Option<Value>>,
}

/// Args `{ pluginId, key, value }`; `value: null` removes the key.
#[tauri::command]
pub async fn plugin_storage_set(state: State<'_, AppState>, request: tauri::ipc::Request<'_>) -> CommandResult<()> {
    let tauri::ipc::InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::invalid_input("expected a JSON body"));
    };
    let args: StorageSetArgs =
        serde_json::from_value(body.clone()).map_err(|err| CommandError::invalid_input(err.to_string()))?;
    let value = args.value.flatten().filter(|v| !v.is_null());
    blocking(&state.plugins, move |manager| manager.storage_set(&args.plugin_id, &args.key, value)).await
}

#[tauri::command]
pub async fn open_plugin_window<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    plugin_id: String,
    window_id: String,
    params: Option<Value>,
) -> CommandResult<()> {
    let plugin = state.plugins.enabled(&plugin_id)?;
    let manifest = plugin.manifest.as_ref().ok_or_else(|| CommandError::new("plugin_invalid", "invalid plugin"))?;
    let window = manifest.window(&window_id).ok_or_else(|| {
        CommandError::new("plugin_window_not_found", format!("plugin {plugin_id:?} has no window {window_id:?}"))
    })?;
    let label = window_label(&plugin_id, &window_id);
    let context = WindowContext {
        plugin_id: plugin_id.clone(),
        window_id: window_id.clone(),
        params: params.unwrap_or(Value::Null),
    };
    state.plugins.set_window_context(&label, context);

    if let Some(existing) = app.get_webview_window(&label) {
        // Reload so the shell picks up the new params.
        let _ = existing.eval("window.location.reload()");
        let _ = existing.unminimize();
        let _ = existing.set_focus();
        return Ok(());
    }

    let manager = Arc::clone(&state.plugins);
    let destroyed_label = label.clone();
    WebviewWindowBuilder::new(&app, &label, WebviewUrl::App("index.html".into()))
        .title(format!("{} — {}", window.title, manifest.name))
        .inner_size(f64::from(window.width.unwrap_or(900)), f64::from(window.height.unwrap_or(640)))
        .min_inner_size(320.0, 240.0)
        .center()
        .build()
        .map_err(|err| CommandError::internal(err.to_string()))?
        .on_window_event(move |event| {
            if matches!(event, tauri::WindowEvent::Destroyed) {
                manager.forget_window(&destroyed_label);
            }
        });
    Ok(())
}

#[tauri::command]
pub async fn get_plugin_window_context<R: Runtime>(
    window: WebviewWindow<R>,
    state: State<'_, AppState>,
) -> CommandResult<WindowContext> {
    state
        .plugins
        .window_context(window.label())
        .ok_or_else(|| CommandError::new("plugin_window_not_found", "this window is not a plugin window"))
}
