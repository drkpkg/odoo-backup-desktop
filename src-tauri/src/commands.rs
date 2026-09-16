//! Tauri commands. See `docs/architecture.md` (Contrato IPC). Secrets are accepted
//! as input only; every response is secret-free.

use chrono::Utc;
use obd_odoo::ProbeInput;
use obd_storage::gdrive::PendingAuthorization;
use secrecy::SecretString;
use serde::Deserialize;
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::{AppHandle, Emitter, Runtime, State};
use tauri_plugin_opener::OpenerExt;
use tokio_util::sync::CancellationToken;

use crate::backup::{self, BackupEvent};
use crate::error::{CommandError, CommandResult};
use crate::history::HistoryEntry;
use crate::jobs::ActiveJob;
use crate::models::{
    AppStatus, DriveStatus, InstanceInput, InstanceRecord, InstanceView, ProbeRequest, ProbeSnapshot,
    deserialize_patch, non_empty,
};
use crate::settings::Settings;
use crate::state::AppState;

const MIN_PASSWORD_LEN: usize = 8;
const DRIVE_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5 * 60);

#[derive(Clone, serde::Serialize)]
struct VaultLockedPayload {
    reason: &'static str,
}

pub fn emit_vault_locked<R: Runtime>(app: &AppHandle<R>, reason: &'static str) {
    if let Err(err) = app.emit("vault-locked", VaultLockedPayload { reason }) {
        tracing::debug!(error = %err, "could not emit vault-locked");
    }
}

fn password(value: Option<String>) -> CommandResult<Option<SecretString>> {
    match value.filter(|v| !v.is_empty()) {
        Some(v) if v.chars().count() < MIN_PASSWORD_LEN => Err(CommandError::new(
            "password_too_short",
            format!("the password needs at least {MIN_PASSWORD_LEN} characters"),
        )),
        other => Ok(other.map(SecretString::from)),
    }
}

async fn app_status(state: &AppState) -> CommandResult<AppStatus> {
    Ok(AppStatus { app_version: state.app_version.clone(), vault: state.vault.status().await? })
}

// ---- vault -----------------------------------------------------------------------------

#[tauri::command]
pub async fn get_app_status(state: State<'_, AppState>) -> CommandResult<AppStatus> {
    app_status(&state).await
}

#[tauri::command]
pub async fn create_vault(
    state: State<'_, AppState>,
    use_keychain: bool,
    master_password: Option<String>,
) -> CommandResult<AppStatus> {
    let master_password = password(master_password)?;
    if !use_keychain && master_password.is_none() {
        return Err(CommandError::new("password_required", "a master password is required without the OS keychain"));
    }
    state.vault.create(use_keychain, master_password).await?;
    app_status(&state).await
}

#[tauri::command]
pub async fn unlock_vault(state: State<'_, AppState>, master_password: Option<String>) -> CommandResult<AppStatus> {
    let master_password = master_password.filter(|p| !p.is_empty()).map(SecretString::from);
    state.vault.unlock(master_password).await?;
    app_status(&state).await
}

#[tauri::command]
pub async fn lock_vault<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CommandResult<AppStatus> {
    if state.vault.lock() {
        emit_vault_locked(&app, "manual");
    }
    app_status(&state).await
}

#[tauri::command]
pub async fn set_master_password(state: State<'_, AppState>, new_password: Option<String>) -> CommandResult<AppStatus> {
    state.vault.set_master_password(password(new_password)?).await?;
    app_status(&state).await
}

#[tauri::command]
pub async fn set_keychain_enabled(state: State<'_, AppState>, enabled: bool) -> CommandResult<AppStatus> {
    state.vault.set_keychain_enabled(enabled).await?;
    app_status(&state).await
}

// ---- instances -------------------------------------------------------------------------

async fn instance_views(state: &AppState) -> CommandResult<Vec<InstanceView>> {
    let latest = state.history.latest_per_instance().await?;
    state.vault.read(|data| {
        let mut views: Vec<InstanceView> = data
            .instances
            .iter()
            .map(|instance| {
                let last = latest.iter().find(|e| e.instance_id == instance.id).cloned().map(|mut entry| {
                    entry.instance_name = instance.name.clone();
                    entry
                });
                instance.view(last)
            })
            .collect();
        views.sort_by_key(|view| view.name.to_lowercase());
        views
    })
}

#[tauri::command]
pub async fn list_instances(state: State<'_, AppState>) -> CommandResult<Vec<InstanceView>> {
    state.vault.touch();
    instance_views(&state).await
}

#[tauri::command]
pub async fn save_instance(state: State<'_, AppState>, input: InstanceInput) -> CommandResult<InstanceView> {
    let name = input.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(CommandError::invalid_input("name must have between 1 and 100 characters"));
    }
    let base_url = backup::parse_base_url(&input.url)?;
    let database = non_empty(Some(input.database.clone()))
        .or_else(|| obd_odoo::database_from_host(&base_url))
        .ok_or_else(|| CommandError::invalid_input("database is required"))?;
    let secret = non_empty(input.secret.clone());
    let master_password = input.master_password.clone();

    let id =
        state
            .vault
            .update(move |data| {
                let slug = obd_storage::local::slugify(&name);
                if data.instances.iter().any(|other| {
                    Some(&other.id) != input.id.as_ref() && obd_storage::local::slugify(&other.name) == slug
                }) {
                    return Err(CommandError::new("duplicate_name", "another instance already uses this name"));
                }
                let now = Utc::now();
                let url = base_url.to_string();
                match &input.id {
                    Some(id) => {
                        let record = data
                            .instances
                            .iter_mut()
                            .find(|instance| &instance.id == id)
                            .ok_or_else(|| CommandError::not_found("instance not found"))?;
                        if record.url != url || record.database != database {
                            record.last_probe = None;
                        }
                        record.name = name;
                        record.url = url;
                        record.database = database;
                        record.login = input.login.trim().to_owned();
                        record.secret_kind = input.secret_kind;
                        if let Some(secret) = secret {
                            record.secret = Some(obd_vault::SecretField::new(secret));
                        }
                        match master_password {
                            None => {}
                            Some(None) => record.master_password = None,
                            Some(Some(value)) if value.is_empty() => {}
                            Some(Some(value)) => record.master_password = Some(obd_vault::SecretField::new(value)),
                        }
                        record.transport = input.transport;
                        record.protocol = input.protocol;
                        record.include_filestore = input.include_filestore;
                        record.upload_to_drive = input.upload_to_drive;
                        record.updated_at = now;
                        Ok(id.clone())
                    }
                    None => {
                        let id = uuid::Uuid::new_v4().to_string();
                        data.instances.push(InstanceRecord {
                            id: id.clone(),
                            name,
                            url,
                            database,
                            login: input.login.trim().to_owned(),
                            secret_kind: input.secret_kind,
                            secret: secret.map(obd_vault::SecretField::new),
                            master_password: master_password
                                .flatten()
                                .filter(|v| !v.is_empty())
                                .map(obd_vault::SecretField::new),
                            transport: input.transport,
                            protocol: input.protocol,
                            include_filestore: input.include_filestore,
                            upload_to_drive: input.upload_to_drive,
                            last_probe: None,
                            created_at: now,
                            updated_at: now,
                        });
                        Ok(id)
                    }
                }
            })
            .await?;

    instance_views(&state)
        .await?
        .into_iter()
        .find(|view| view.id == id)
        .ok_or_else(|| CommandError::internal("saved instance not found"))
}

#[tauri::command]
pub async fn delete_instance(state: State<'_, AppState>, id: String) -> CommandResult<()> {
    if state.jobs.is_instance_running(&id) {
        return Err(CommandError::new("backup_in_progress", "wait for the running backup to finish"));
    }
    let target = id.clone();
    state
        .vault
        .update(move |data| {
            let before = data.instances.len();
            data.instances.retain(|instance| instance.id != target);
            if data.instances.len() == before {
                return Err(CommandError::not_found("instance not found"));
            }
            Ok(())
        })
        .await?;
    state.history.delete_for_instance(&id).await
}

#[tauri::command]
pub async fn probe_instance(state: State<'_, AppState>, input: ProbeRequest) -> CommandResult<ProbeSnapshot> {
    state.vault.touch();
    let stored = match &input.instance_id {
        Some(id) => state.vault.read(|data| data.instance(id).cloned())?,
        None => None,
    };
    let base_url = backup::parse_base_url(&input.url)?;
    let secret = non_empty(input.secret.clone()).map(SecretString::from).or_else(|| {
        stored.as_ref().and_then(|s| s.secret.as_ref()).filter(|s| !s.is_empty()).map(|s| s.to_secret_string())
    });
    let login = non_empty(input.login.clone()).or_else(|| stored.as_ref().map(|s| s.login.clone())).unwrap_or_default();
    let master_password = non_empty(input.master_password.clone()).map(SecretString::from).or_else(|| {
        stored.as_ref().and_then(|s| s.master_password.as_ref()).filter(|s| !s.is_empty()).map(|s| s.to_secret_string())
    });

    let report = obd_odoo::probe(
        &state.http,
        ProbeInput {
            base_url,
            database: non_empty(input.database.clone()),
            credentials: secret.map(|secret| obd_odoo::Credentials { login, secret, kind: input.secret_kind }),
            master_password,
            protocol_preference: input.protocol,
        },
    )
    .await?;
    let snapshot = ProbeSnapshot { report, checked_at: Utc::now() };

    if let Some(id) = input.instance_id.filter(|_| stored.is_some()) {
        let saved = snapshot.clone();
        state
            .vault
            .update(move |data| {
                if let Some(record) = data.instances.iter_mut().find(|instance| instance.id == id) {
                    record.last_probe = Some(saved);
                }
                Ok(())
            })
            .await?;
    }
    Ok(snapshot)
}

// ---- backups ---------------------------------------------------------------------------

#[tauri::command]
pub async fn start_backup<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    instance_id: String,
    on_event: Channel<BackupEvent>,
) -> CommandResult<String> {
    state.vault.touch();
    backup::start(app, &state, &instance_id, on_event).await
}

#[tauri::command]
pub async fn cancel_backup(state: State<'_, AppState>, job_id: String) -> CommandResult<()> {
    if state.jobs.cancel(&job_id) { Ok(()) } else { Err(CommandError::new("job_not_found", "job not found")) }
}

#[tauri::command]
pub async fn list_active_jobs(state: State<'_, AppState>) -> CommandResult<Vec<ActiveJob>> {
    Ok(state.jobs.snapshots())
}

#[tauri::command]
pub async fn list_history(
    state: State<'_, AppState>,
    instance_id: Option<String>,
    limit: Option<u32>,
) -> CommandResult<Vec<HistoryEntry>> {
    let mut entries = state.history.list(instance_id, limit.unwrap_or(200)).await?;
    state.vault.read(|data| {
        for entry in &mut entries {
            entry.instance_name = data.instance(&entry.instance_id).map(|i| i.name.clone()).unwrap_or_default();
        }
    })?;
    Ok(entries)
}

#[tauri::command]
pub async fn reveal_backup<R: Runtime>(
    app: AppHandle<R>,
    state: State<'_, AppState>,
    history_id: String,
) -> CommandResult<()> {
    let entry =
        state.history.get(&history_id).await?.ok_or_else(|| CommandError::not_found("history entry not found"))?;
    let path = entry.file_path.ok_or_else(|| CommandError::not_found("this backup has no local file"))?;
    let path = std::path::PathBuf::from(path);
    if !backup::backup_exists(&path) {
        return Err(CommandError::new("file_missing", "the backup file no longer exists"));
    }
    app.opener().reveal_item_in_dir(&path).map_err(|err| CommandError::internal(err.to_string()))
}

// ---- settings --------------------------------------------------------------------------

#[tauri::command]
pub async fn get_settings(state: State<'_, AppState>) -> CommandResult<Settings> {
    Ok(state.settings())
}

#[tauri::command]
pub async fn update_settings(state: State<'_, AppState>, settings: Settings) -> CommandResult<Settings> {
    state.vault.touch();
    state.replace_settings(settings)
}

// ---- Google Drive ----------------------------------------------------------------------

#[tauri::command]
pub async fn get_drive_status(state: State<'_, AppState>) -> CommandResult<DriveStatus> {
    state.vault.read(|data| crate::drive::status(&data.drive))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DriveClientInput {
    client_id: String,
    #[serde(default, deserialize_with = "deserialize_patch")]
    client_secret: Option<Option<String>>,
}

/// Args `{ clientId, clientSecret? }`: absent secret keeps it, `null` removes it.
#[tauri::command]
pub async fn set_drive_client(state: State<'_, AppState>, request: Request<'_>) -> CommandResult<DriveStatus> {
    let InvokeBody::Json(body) = request.body() else {
        return Err(CommandError::invalid_input("expected a JSON body"));
    };
    let input: DriveClientInput =
        serde_json::from_value(body.clone()).map_err(|err| CommandError::invalid_input(err.to_string()))?;
    let client_id = input.client_id.trim().to_owned();

    state
        .vault
        .update(move |data| {
            let drive = &mut data.drive;
            let new_id = Some(client_id).filter(|id| !id.is_empty());
            if drive.client_id != new_id {
                // Tokens belong to the previous OAuth client.
                drive.refresh_token = None;
                drive.email = None;
                drive.display_name = None;
            }
            drive.client_id = new_id;
            match input.client_secret {
                None => {}
                Some(None) => drive.client_secret = None,
                Some(Some(secret)) if secret.trim().is_empty() => drive.client_secret = None,
                Some(Some(secret)) => drive.client_secret = Some(obd_vault::SecretField::new(secret.trim())),
            }
            Ok(crate::drive::status(drive))
        })
        .await
}

#[tauri::command]
pub async fn connect_drive<R: Runtime>(app: AppHandle<R>, state: State<'_, AppState>) -> CommandResult<DriveStatus> {
    let client = state
        .vault
        .read(|data| crate::drive::oauth_client(&data.drive))?
        .ok_or_else(|| CommandError::new("drive_not_configured", "configure the Google OAuth client first"))?;

    // Only one authorization at a time: a new attempt cancels the previous one.
    let attempt = uuid::Uuid::new_v4().to_string();
    let cancel = CancellationToken::new();
    if let Ok(mut current) = state.drive_connect.lock()
        && let Some((_, previous)) = current.replace((attempt.clone(), cancel.clone()))
    {
        previous.cancel();
    }

    let result = async {
        let pending = PendingAuthorization::start(state.http.clone(), client).await?;
        app.opener()
            .open_url(pending.authorize_url().as_str(), None::<&str>)
            .map_err(|err| CommandError::new("browser_open_failed", err.to_string()))?;
        Ok::<_, CommandError>(pending.finish(cancel.clone(), DRIVE_CONNECT_TIMEOUT).await?)
    }
    .await;

    if let Ok(mut current) = state.drive_connect.lock()
        && current.as_ref().is_some_and(|(id, _)| *id == attempt)
    {
        current.take();
    }
    let account = result?;

    state
        .vault
        .update(move |data| {
            let drive = &mut data.drive;
            drive.refresh_token =
                Some(obd_vault::SecretField::new(secrecy::ExposeSecret::expose_secret(&account.refresh_token)));
            drive.email = account.email;
            drive.display_name = account.display_name;
            Ok(crate::drive::status(drive))
        })
        .await
}

#[tauri::command]
pub async fn cancel_drive_connect(state: State<'_, AppState>) -> CommandResult<()> {
    if let Ok(mut current) = state.drive_connect.lock()
        && let Some((_, token)) = current.take()
    {
        token.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn disconnect_drive(state: State<'_, AppState>) -> CommandResult<DriveStatus> {
    let settings = state.settings();
    let config = state.vault.read(|data| data.drive.clone())?;
    if let Ok(adapter) = crate::drive::adapter(&state.http, &config, &settings)
        && let Err(err) = adapter.revoke().await
    {
        tracing::warn!(code = err.code(), "could not revoke Google token");
    }
    state
        .vault
        .update(|data| {
            data.drive.refresh_token = None;
            data.drive.email = None;
            data.drive.display_name = None;
            Ok(crate::drive::status(&data.drive))
        })
        .await
}
