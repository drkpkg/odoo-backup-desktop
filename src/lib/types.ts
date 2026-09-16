// IPC contract types. Mirrors docs/architecture.md ("Contrato IPC") exactly.

// --- Estado y bóveda -------------------------------------------------------

export type VaultStatus = {
  exists: boolean;
  unlocked: boolean;
  keychainAvailable: boolean;
  keychainEnabled: boolean;
  passwordEnabled: boolean;
};

export type AppStatus = {
  appVersion: string;
  vault: VaultStatus;
};

export type VaultLockedPayload = { reason: "manual" | "idle" };

// --- Instancias ------------------------------------------------------------

export type SecretKind = "password" | "api_key";
export type TransportPreference = "auto" | "db_manager" | "appex_module";
export type ProtocolPreference = "auto" | "xml_rpc" | "json2";
export type TransportKind = "db_manager" | "appex_module";
export type RpcProtocol = "xml_rpc" | "json2";

export type OdooVersion = {
  major: number;
  minor: number;
  serverVersion: string;
  saas: boolean;
};

export type CheckStatus =
  | { status: "ok" }
  | { status: "failed"; code: string; message: string }
  | { status: "skipped"; reason: string };

export type ProbeWarning =
  | "insecure_http"
  | "unsupported_version"
  | "deprecated_xml_rpc"
  | "database_derived_from_host"
  | "master_password_over_wire";

export type ProbeReport = {
  baseUrl: string;
  https: boolean;
  version: OdooVersion;
  supported: boolean;
  database: string | null;
  protocol: RpcProtocol | null;
  uid: number | null;
  auth: CheckStatus;
  module: CheckStatus;
  moduleApiVersion: number | null;
  dbManager: CheckStatus;
  recommendedTransport: TransportKind | null;
  warnings: ProbeWarning[];
  checkedAt: string;
};

export type InstanceView = {
  id: string;
  name: string;
  url: string;
  database: string;
  login: string;
  secretKind: SecretKind;
  hasSecret: boolean;
  hasMasterPassword: boolean;
  transport: TransportPreference;
  protocol: ProtocolPreference;
  includeFilestore: boolean;
  uploadToDrive: boolean;
  lastProbe: ProbeReport | null;
  lastBackup: HistoryEntry | null;
  createdAt: string;
  updatedAt: string;
};

export type InstanceInput = {
  id?: string;
  name: string;
  url: string;
  database: string;
  login: string;
  secretKind: SecretKind;
  /** Ausente = conservar el secreto guardado. */
  secret?: string;
  /** Ausente = conservar, null = borrar. */
  masterPassword?: string | null;
  transport: TransportPreference;
  protocol: ProtocolPreference;
  includeFilestore: boolean;
  uploadToDrive: boolean;
};

export type ProbeRequest = {
  instanceId?: string;
  url: string;
  database?: string;
  login?: string;
  secretKind: SecretKind;
  secret?: string;
  masterPassword?: string;
  protocol: ProtocolPreference;
};

// --- Backups e historial ---------------------------------------------------

export type BackupStage =
  | "requesting"
  | "server_preparing"
  | "downloading"
  | "validating"
  | "uploading"
  | "retention";

export type BackupEvent =
  | { type: "started"; jobId: string; instanceId: string; transport: TransportKind }
  | {
      type: "progress";
      jobId: string;
      stage: BackupStage;
      elapsedSecs?: number;
      received?: number;
      total?: number | null;
      sent?: number;
    }
  | { type: "completed"; jobId: string; entry: HistoryEntry }
  | { type: "failed"; jobId: string; code: string; message: string }
  | { type: "cancelled"; jobId: string };

export type ActiveJob = {
  jobId: string;
  instanceId: string;
  stage: BackupStage;
  startedAt: string;
  received?: number;
  total?: number | null;
  sent?: number;
};

export type HistoryStatus = "running" | "success" | "failed" | "cancelled";
export type DriveUploadStatus = "skipped" | "success" | "failed";

export type HistoryEntry = {
  id: string;
  instanceId: string;
  instanceName: string;
  status: HistoryStatus;
  transport: TransportKind | null;
  startedAt: string;
  finishedAt: string | null;
  filePath: string | null;
  sizeBytes: number | null;
  sha256: string | null;
  odooVersion: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  drive: { status: DriveUploadStatus; fileId: string | null; errorMessage: string | null };
};

// --- Ajustes y Google Drive ------------------------------------------------

export type DriveSettings = {
  rootFolderName: string;
  keepLast: number | null;
  permanentDelete: boolean;
  sharedDriveId: string | null;
};

export type Settings = {
  downloadDir: string;
  keepLastLocal: number | null;
  maxConcurrentBackups: number;
  serverPrepareTimeoutMinutes: number;
  autoLockMinutes: number | null;
  drive: DriveSettings;
};

export type DriveStatus = {
  configured: boolean;
  clientId: string | null;
  hasClientSecret: boolean;
  connected: boolean;
  email: string | null;
  displayName: string | null;
};

// --- Errores ---------------------------------------------------------------

/** Forma de error de todos los comandos. */
export type CommandError = { code: string; message: string };
