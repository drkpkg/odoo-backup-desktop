import type {
  ActiveJob,
  AppStatus,
  BackupEvent,
  DriveStatus,
  HistoryEntry,
  InstanceInput,
  InstanceView,
  PluginConfig,
  PluginSettings,
  PluginsChangedPayload,
  PluginView,
  PluginWindowContext,
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

  // Plugins (docs/plugins.md)
  listPlugins(): Promise<PluginView[]>;
  reloadPlugins(): Promise<PluginView[]>;
  setPluginEnabled(pluginId: string, enabled: boolean): Promise<PluginView[]>;
  getPluginConfig(): Promise<PluginConfig>;
  setDeveloperMode(enabled: boolean): Promise<PluginConfig>;
  addDevPlugin(path: string): Promise<PluginConfig>;
  removeDevPlugin(path: string): Promise<PluginConfig>;
  openPluginsFolder(): Promise<void>;
  getPluginSettings(pluginId: string): Promise<PluginSettings>;
  /** Secretos: string = reemplazar, null = borrar, ausente = conservar. */
  savePluginSettings(pluginId: string, values: Record<string, unknown>): Promise<PluginSettings>;
  pluginStorageGet(pluginId: string, key: string): Promise<unknown>;
  /** `value` null borra la clave. */
  pluginStorageSet(pluginId: string, key: string, value: unknown): Promise<void>;
  openPluginWindow(pluginId: string, windowId: string, params?: Record<string, unknown>): Promise<void>;
  /** Contexto de la ventana de plugin actual (solo en ventanas `plugin--*`). */
  getPluginWindowContext(): Promise<PluginWindowContext>;
  onPluginsChanged(handler: (payload: PluginsChangedPayload) => void): Promise<() => void>;
  /** Desde una ventana de plugin: pide a la ventana principal abrir los ajustes del plugin. */
  requestOpenPluginSettings(pluginId: string): Promise<void>;
  /** Ventana principal: escucha las peticiones anteriores. */
  onOpenPluginSettings(handler: (pluginId: string) => void): Promise<() => void>;
  /** Etiqueta de la ventana actual ("main" o "plugin--…"). */
  windowLabel(): string;

  // Diálogos nativos
  pickDirectory(defaultPath?: string): Promise<string | null>;
}
