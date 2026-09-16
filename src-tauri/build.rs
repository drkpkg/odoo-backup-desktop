const COMMANDS: &[&str] = &[
    "get_app_status",
    "create_vault",
    "unlock_vault",
    "lock_vault",
    "set_master_password",
    "set_keychain_enabled",
    "list_instances",
    "save_instance",
    "delete_instance",
    "probe_instance",
    "start_backup",
    "cancel_backup",
    "list_active_jobs",
    "list_history",
    "reveal_backup",
    "get_settings",
    "update_settings",
    "get_drive_status",
    "set_drive_client",
    "connect_drive",
    "cancel_drive_connect",
    "disconnect_drive",
];

fn main() {
    // Every command must be granted explicitly through a capability.
    tauri_build::try_build(
        tauri_build::Attributes::new().app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("failed to run tauri-build");
}
