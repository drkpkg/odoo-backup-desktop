import type {
  BackupStage,
  DriveUploadStatus,
  HistoryStatus,
  ProbeWarning,
  ProtocolPreference,
  RpcProtocol,
  SecretKind,
  TransportKind,
  TransportPreference,
} from "./types";

export const SECRET_KIND_LABELS: Record<SecretKind, string> = {
  password: "Contraseña",
  api_key: "API key",
};

export const TRANSPORT_PREFERENCE_LABELS: Record<TransportPreference, string> = {
  auto: "Automático",
  db_manager: "Gestor de BD",
  obd_module: "Módulo obd_backup",
};

export const TRANSPORT_LABELS: Record<TransportKind, string> = {
  db_manager: "Gestor de BD",
  obd_module: "Módulo obd_backup",
};

export const PROTOCOL_PREFERENCE_LABELS: Record<ProtocolPreference, string> = {
  auto: "Automático",
  xml_rpc: "XML-RPC",
  json2: "JSON-2",
};

export const PROTOCOL_LABELS: Record<RpcProtocol, string> = {
  xml_rpc: "XML-RPC",
  json2: "JSON-2",
};

export const STAGE_LABELS: Record<BackupStage, string> = {
  requesting: "Solicitando",
  server_preparing: "El servidor está preparando el backup",
  downloading: "Descargando",
  validating: "Validando",
  uploading: "Subiendo a Google Drive",
  retention: "Aplicando retención",
};

export const HISTORY_STATUS_LABELS: Record<HistoryStatus, string> = {
  running: "En curso",
  success: "Correcto",
  failed: "Fallido",
  cancelled: "Cancelado",
};

export const DRIVE_STATUS_LABELS: Record<DriveUploadStatus, string> = {
  skipped: "No aplica",
  success: "Subido",
  failed: "Error",
};

export type WarningInfo = { title: string; detail: string; severity: "danger" | "warning" | "info" };

export const PROBE_WARNINGS: Record<ProbeWarning, WarningInfo> = {
  insecure_http: {
    title: "Conexión sin cifrar (http://)",
    detail:
      "Las credenciales y la contraseña maestra viajan en texto plano. Usa https:// para instancias en producción.",
    severity: "danger",
  },
  master_password_over_wire: {
    title: "Se envía la contraseña maestra",
    detail:
      "El gestor de BD recibe la contraseña maestra en cada backup. Si el servidor todavía usa la contraseña por defecto «admin», Odoo la reemplazará por la que envíes y la guardará en su archivo de configuración.",
    severity: "warning",
  },
  unsupported_version: {
    title: "Versión no soportada",
    detail: "Odoo Backup Desktop admite Odoo 15.0 a 19.0. Otras versiones pueden fallar.",
    severity: "warning",
  },
  deprecated_xml_rpc: {
    title: "XML-RPC obsoleto en Odoo 19",
    detail: "Odoo 19 marca XML-RPC como obsoleto (se elimina en Odoo 22). Usa JSON-2 con una API key.",
    severity: "info",
  },
  database_derived_from_host: {
    title: "Base de datos deducida del subdominio",
    detail: "Se usó el primer segmento del dominio como nombre de base de datos (dbfilter = ^%d$). Verifica que sea correcto.",
    severity: "info",
  },
};

/** Orden de severidad para mostrar advertencias. */
export const WARNING_ORDER: ProbeWarning[] = [
  "insecure_http",
  "master_password_over_wire",
  "unsupported_version",
  "deprecated_xml_rpc",
  "database_derived_from_host",
];
