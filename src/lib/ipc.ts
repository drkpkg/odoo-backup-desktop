import { Channel, invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
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
  ProbeReport,
  Settings,
  VaultLockedPayload,
} from "./types";

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
