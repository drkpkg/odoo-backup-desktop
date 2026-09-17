import { Channel, invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { open } from "@tauri-apps/plugin-dialog";

import type { Backend } from "./backend";
import { toAppError } from "./errors";
import { createMockBackend } from "./mock-backend";
import type {
  ActiveJob,
  AppStatus,
  BackupEvent,
  DriveStatus,
  HistoryEntry,
  InstanceView,
  PluginConfig,
  PluginSettings,
  PluginsChangedPayload,
  PluginView,
  PluginWindowContext,
  ProbeReport,
  Settings,
  VaultLockedPayload,
} from "./types";

/** Evento que una ventana de plugin envía a la principal para abrir los ajustes del plugin. */
export const OPEN_PLUGIN_SETTINGS_EVENT = "plugin-open-settings";

/** Invoca un comando y normaliza el rechazo a `AppError`. */
async function call<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    throw toAppError(error);
  }
}

export function createTauriBackend(): Backend {
  return {
    kind: "tauri",

    getAppStatus: () => call<AppStatus>("get_app_status"),
    createVault: (args) => call<AppStatus>("create_vault", args),
    unlockVault: (args) => call<AppStatus>("unlock_vault", args),
    lockVault: () => call<AppStatus>("lock_vault"),
    setMasterPassword: (args) => call<AppStatus>("set_master_password", args),
    setKeychainEnabled: (args) => call<AppStatus>("set_keychain_enabled", args),
    onVaultLocked: (handler) =>
      listen<VaultLockedPayload>("vault-locked", (event) => handler(event.payload)),

    listInstances: () => call<InstanceView[]>("list_instances"),
    saveInstance: (input) => call<InstanceView>("save_instance", { input }),
    deleteInstance: (id) => call<void>("delete_instance", { id }),
    probeInstance: (input) => call<ProbeReport>("probe_instance", { input }),

    startBackup: (instanceId, onEvent) => {
      const channel = new Channel<BackupEvent>();
      channel.onmessage = onEvent;
      return call<string>("start_backup", { instanceId, onEvent: channel });
    },
    cancelBackup: (jobId) => call<void>("cancel_backup", { jobId }),
    listActiveJobs: () => call<ActiveJob[]>("list_active_jobs"),
    listHistory: (args) => call<HistoryEntry[]>("list_history", args ?? {}),
    revealBackup: (historyId) => call<void>("reveal_backup", { historyId }),

    getSettings: () => call<Settings>("get_settings"),
    updateSettings: (settings) => call<Settings>("update_settings", { settings }),
    getDriveStatus: () => call<DriveStatus>("get_drive_status"),
    setDriveClient: (args) => call<DriveStatus>("set_drive_client", args),
    connectDrive: () => call<DriveStatus>("connect_drive"),
    cancelDriveConnect: () => call<void>("cancel_drive_connect"),
    disconnectDrive: () => call<DriveStatus>("disconnect_drive"),

    listPlugins: () => call<PluginView[]>("list_plugins"),
    reloadPlugins: () => call<PluginView[]>("reload_plugins"),
    setPluginEnabled: (pluginId, enabled) => call<PluginView[]>("set_plugin_enabled", { pluginId, enabled }),
    getPluginConfig: () => call<PluginConfig>("get_plugin_config"),
    setDeveloperMode: (enabled) => call<PluginConfig>("set_developer_mode", { enabled }),
    addDevPlugin: (path) => call<PluginConfig>("add_dev_plugin", { path }),
    removeDevPlugin: (path) => call<PluginConfig>("remove_dev_plugin", { path }),
    openPluginsFolder: () => call<void>("open_plugins_folder"),
    getPluginSettings: (pluginId) => call<PluginSettings>("get_plugin_settings", { pluginId }),
    savePluginSettings: (pluginId, values) => call<PluginSettings>("save_plugin_settings", { pluginId, values }),
    pluginStorageGet: (pluginId, key) => call<unknown>("plugin_storage_get", { pluginId, key }),
    pluginStorageSet: (pluginId, key, value) => call<void>("plugin_storage_set", { pluginId, key, value }),
    openPluginWindow: (pluginId, windowId, params) =>
      call<void>("open_plugin_window", params ? { pluginId, windowId, params } : { pluginId, windowId }),
    getPluginWindowContext: () => call<PluginWindowContext>("get_plugin_window_context"),
    onPluginsChanged: (handler) =>
      listen<PluginsChangedPayload>("plugins-changed", (event) => handler(event.payload)),
    requestOpenPluginSettings: async (pluginId) => {
      try {
        await emitTo("main", OPEN_PLUGIN_SETTINGS_EVENT, { pluginId });
      } catch (error) {
        throw toAppError(error);
      }
    },
    onOpenPluginSettings: (handler) =>
      listen<{ pluginId: string }>(OPEN_PLUGIN_SETTINGS_EVENT, (event) => {
        if (typeof event.payload?.pluginId === "string") handler(event.payload.pluginId);
      }),
    windowLabel: () => getCurrentWebviewWindow().label,

    pickDirectory: async (defaultPath) => {
      try {
        const selected = await open({ directory: true, multiple: false, defaultPath });
        return typeof selected === "string" ? selected : null;
      } catch (error) {
        throw toAppError(error);
      }
    },
  };
}

/** `true` dentro de la ventana de Tauri; `false` en un navegador normal (`pnpm dev`). */
export function isTauriRuntime(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Backend activo: Tauri en la app, mock en memoria en el navegador. */
export const ipc: Backend = isTauriRuntime() ? createTauriBackend() : createMockBackend();
