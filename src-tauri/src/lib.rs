//! Odoo Backup Desktop desktop application (Tauri 2).

mod backup;
mod commands;
mod drive;
mod error;
mod history;
mod jobs;
mod models;
mod plugin_commands;
mod plugins;
mod settings;
mod state;
mod vault;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use obd_vault::{KdfParams, KeyStore, OsKeyStore, VaultFile};
use tauri::{AppHandle, Manager, Runtime};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use crate::history::History;
use crate::jobs::{JobRegistry, Limiter};
use crate::state::{AppPaths, AppState, KEYCHAIN_ACCOUNT, KEYCHAIN_SERVICE};
use crate::vault::VaultManager;

const AUTO_LOCK_CHECK_INTERVAL: Duration = Duration::from_secs(30);

/// Keeps the log writer alive for the lifetime of the app.
struct LogGuard(#[allow(dead_code)] WorkerGuard);

/// Overrides used by integration tests. `Default` is the production behavior.
#[derive(Default)]
pub struct AppOptions {
    /// Replaces the platform app data directory.
    pub data_dir: Option<PathBuf>,
    /// Replaces the default download directory (`~/Downloads/Odoo Backup Desktop`).
    pub download_dir: Option<PathBuf>,
    /// Replaces the OS keychain.
    pub keystore: Option<Arc<dyn KeyStore>>,
    /// Replaces the Argon2id cost (tests only).
    pub kdf: Option<KdfParams>,
    /// Skips file logging.
    pub disable_logging: bool,
    /// Skips desktop notifications.
    pub disable_notifications: bool,
    /// Replaces `<resources>/plugins` (built-in plugins).
    pub builtin_plugins_dir: Option<PathBuf>,
    /// Replaces `<data>/plugins` (user plugins).
    pub user_plugins_dir: Option<PathBuf>,
}

pub fn run() {
    let builder = tauri::Builder::default()
        // Must be the first plugin: a second launch focuses the running window instead.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }));
    app_builder(builder, AppOptions::default())
        .run(tauri::generate_context!())
        .expect("error while running Odoo Backup Desktop");
}

/// Plugins, state setup and command handlers, shared by the app and the IPC tests.
pub fn app_builder<R: Runtime>(builder: tauri::Builder<R>, options: AppOptions) -> tauri::Builder<R> {
    let options = Mutex::new(Some(options));
    builder
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        // Plugin pages and the plugin SDK (`obd-plugin://localhost/<id>/...`).
        .register_asynchronous_uri_scheme_protocol(obd_plugins::PROTOCOL_SCHEME, |ctx, request, responder| {
            let app = ctx.app_handle().clone();
            let path = request.uri().path().to_owned();
            tauri::async_runtime::spawn_blocking(move || {
                let response = match app.try_state::<AppState>() {
                    Some(state) => state.plugins.respond(&path),
                    None => {
                        let mut response = tauri::http::Response::new(Vec::new());
                        *response.status_mut() = tauri::http::StatusCode::SERVICE_UNAVAILABLE;
                        response
                    }
                };
                responder.respond(response);
            });
        })
        .setup(move |app| {
            let options = options.lock().ok().and_then(|mut o| o.take()).unwrap_or_default();
            setup(app.handle(), options)?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_app_status,
            commands::create_vault,
            commands::unlock_vault,
            commands::lock_vault,
            commands::set_master_password,
            commands::set_keychain_enabled,
            commands::list_instances,
            commands::save_instance,
            commands::delete_instance,
            commands::probe_instance,
            commands::start_backup,
            commands::cancel_backup,
            commands::list_active_jobs,
            commands::list_history,
            commands::reveal_backup,
            commands::get_settings,
            commands::update_settings,
            commands::get_drive_status,
            commands::set_drive_client,
            commands::connect_drive,
            commands::cancel_drive_connect,
            commands::disconnect_drive,
            plugin_commands::list_plugins,
            plugin_commands::reload_plugins,
            plugin_commands::set_plugin_enabled,
            plugin_commands::get_plugin_config,
            plugin_commands::set_developer_mode,
            plugin_commands::add_dev_plugin,
            plugin_commands::remove_dev_plugin,
            plugin_commands::open_plugins_folder,
            plugin_commands::get_plugin_settings,
            plugin_commands::save_plugin_settings,
            plugin_commands::plugin_storage_get,
            plugin_commands::plugin_storage_set,
            plugin_commands::open_plugin_window,
            plugin_commands::get_plugin_window_context,
        ])
}

fn setup<R: Runtime>(app: &AppHandle<R>, options: AppOptions) -> Result<(), Box<dyn std::error::Error>> {
    let data_dir = match options.data_dir {
        Some(dir) => dir,
        None => app.path().app_data_dir()?,
    };
    create_private_dir(&data_dir)?;
    if !options.disable_logging {
        let log_dir = app.path().app_log_dir()?;
        std::fs::create_dir_all(&log_dir)?;
        app.manage(LogGuard(init_logging(&log_dir)));
    }

    let app_version = app.package_info().version.to_string();
    tracing::info!(version = %app_version, "starting Odoo Backup Desktop");

    let paths = AppPaths::new(data_dir);
    let default_download_dir = options.download_dir.unwrap_or_else(|| {
        app.path()
            .download_dir()
            .or_else(|_| app.path().home_dir())
            .map(|dir| dir.join("Odoo Backup Desktop"))
            .unwrap_or_else(|_| paths.data_dir.join("backups"))
    });
    let settings = settings::load(&paths.settings_file, &default_download_dir);

    let keystore: Arc<dyn KeyStore> = match options.keystore {
        Some(keystore) => keystore,
        None => {
            if let Err(err) = obd_vault::init_os_keystore() {
                tracing::warn!(code = err.code(), "OS keychain not available; the master password will be required");
            }
            Arc::new(OsKeyStore::new(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT))
        }
    };
    let vault_file = VaultFile::new(&paths.vault_file);
    let vault = match options.kdf {
        Some(kdf) => VaultManager::with_kdf(vault_file, keystore, kdf),
        None => VaultManager::new(vault_file, keystore),
    };

    let builtin_plugins_dir =
        options.builtin_plugins_dir.or_else(|| app.path().resource_dir().ok().map(|dir| dir.join("plugins")));
    let user_plugins_dir = options.user_plugins_dir.unwrap_or_else(|| paths.data_dir.join("plugins"));
    let plugins = plugins::PluginManager::new(builtin_plugins_dir, user_plugins_dir, &paths.data_dir);
    plugin_commands::sync_watcher(app, &plugins);

    let state = AppState {
        plugins,
        http: state::build_http_client(&app_version),
        app_version,
        notifications: !options.disable_notifications,
        vault,
        settings: RwLock::new(settings),
        history: History::open(&paths.history_file).map_err(|err| err.to_string())?,
        jobs: JobRegistry::default(),
        limiter: Limiter::default(),
        drive_connect: Mutex::new(None),
        paths,
    };
    app.manage(state);

    spawn_auto_lock(app.clone());
    Ok(())
}

fn spawn_auto_lock<R: Runtime>(app: AppHandle<R>) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(AUTO_LOCK_CHECK_INTERVAL);
        loop {
            interval.tick().await;
            let state = app.state::<AppState>();
            let Some(minutes) = state.settings().auto_lock_minutes else { continue };
            let idle_limit = Duration::from_secs(u64::from(minutes) * 60);
            if state.vault.is_unlocked()
                && state.jobs.is_empty()
                && state.vault.idle_for() >= idle_limit
                && state.vault.lock()
            {
                tracing::info!("vault locked after inactivity");
                commands::emit_vault_locked(&app, "idle");
            }
        }
    });
}

fn init_logging(log_dir: &std::path::Path) -> WorkerGuard {
    let file_appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("odoo-backup-desktop")
        .filename_suffix("log")
        .max_log_files(14)
        .build(log_dir)
        .unwrap_or_else(|_| tracing_appender::rolling::never(log_dir, "odoo-backup-desktop.log"));
    let (writer, guard) = tracing_appender::non_blocking(file_appender);

    let level = std::env::var("OBD_LOG_LEVEL").unwrap_or_else(|_| "info".into());
    let filter = tracing_subscriber::EnvFilter::try_new(format!("{level},hyper=warn,reqwest=warn"))
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    let registry = tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(writer).with_ansi(false).with_target(false));
    #[cfg(debug_assertions)]
    let registry = registry.with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr));
    if registry.try_init().is_err() {
        eprintln!("logging was already initialized");
    }
    guard
}

fn create_private_dir(dir: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}
