import type {
  ActiveJob,
  AppStatus,
  BackupEvent,
  DriveStatus,
  HistoryEntry,
  InstanceInput,
  InstanceView,
  ProbeReport,
  ProbeRequest,
  Settings,
  VaultLockedPayload,
} from "./types";

/**
 * Operaciones disponibles para la UI. Implementado por el backend Tauri real
 * (`ipc.ts`) y por el mock en memoria (`mock-backend.ts`) para el navegador.
 */
export interface Backend {
  readonly kind: "tauri" | "mock";

  // Estado y bóveda
  getAppStatus(): Promise<AppStatus>;
  createVault(args: { useKeychain: boolean; masterPassword?: string }): Promise<AppStatus>;
  unlockVault(args: { masterPassword?: string }): Promise<AppStatus>;
  lockVault(): Promise<AppStatus>;
  /** Sin `newPassword` = quitar la contraseña (requiere llavero activo). */
  setMasterPassword(args: { newPassword?: string }): Promise<AppStatus>;
  setKeychainEnabled(args: { enabled: boolean }): Promise<AppStatus>;
  onVaultLocked(handler: (payload: VaultLockedPayload) => void): Promise<() => void>;

  // Instancias
  listInstances(): Promise<InstanceView[]>;
  saveInstance(input: InstanceInput): Promise<InstanceView>;
  deleteInstance(id: string): Promise<void>;
  probeInstance(input: ProbeRequest): Promise<ProbeReport>;

  // Backups e historial
  startBackup(instanceId: string, onEvent: (event: BackupEvent) => void): Promise<string>;
  cancelBackup(jobId: string): Promise<void>;
  listActiveJobs(): Promise<ActiveJob[]>;
  listHistory(args?: { instanceId?: string; limit?: number }): Promise<HistoryEntry[]>;
  revealBackup(historyId: string): Promise<void>;

  // Ajustes y Google Drive
  getSettings(): Promise<Settings>;
  updateSettings(settings: Settings): Promise<Settings>;
  getDriveStatus(): Promise<DriveStatus>;
  /** `clientSecret`: ausente = conservar, null = borrar. */
  setDriveClient(args: { clientId: string; clientSecret?: string | null }): Promise<DriveStatus>;
  connectDrive(): Promise<DriveStatus>;
  cancelDriveConnect(): Promise<void>;
  disconnectDrive(): Promise<DriveStatus>;

  // Diálogos nativos
  pickDirectory(defaultPath?: string): Promise<string | null>;
}
