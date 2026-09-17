import type { CommandError } from "./types";

/** Error normalizado para la UI: `code` estable + `message` técnico (inglés). */
export class AppError extends Error implements CommandError {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "AppError";
    this.code = code;
  }
}

/** Mensajes en español por código de error (crates + capa de la app). */
export const ERROR_MESSAGES: Record<string, string> = {
  // Bóveda / llavero
  vault_not_found: "No existe una bóveda. Crea una para empezar.",
  vault_exists: "Ya existe una bóveda en este equipo.",
  vault_wrong_password: "La contraseña maestra no es correcta.",
  vault_password_not_enabled: "Esta bóveda no tiene contraseña maestra configurada.",
  vault_locked: "La bóveda está bloqueada. Desbloquéala para continuar.",
  vault_corrupted: "El archivo de la bóveda está dañado o fue modificado.",
  vault_unsupported_version: "La bóveda fue creada con una versión más nueva de la aplicación.",
  vault_invalid_options: "Opciones inválidas: se necesita el llavero del sistema o una contraseña maestra.",
  vault_serde: "No se pudo leer el contenido de la bóveda.",
  keychain_unavailable: "El llavero del sistema no está disponible en este equipo.",
  keychain_key_missing: "La clave de la bóveda no está en el llavero del sistema.",
  keychain_key_mismatch: "La clave guardada en el llavero no corresponde a esta bóveda.",
  keychain_error: "Error al acceder al llavero del sistema.",
  keychain_not_enabled: "Esta bóveda no usa el llavero del sistema.",

  // Odoo
  invalid_url: "La URL no es válida.",
  connection: "No se pudo conectar con el servidor.",
  timeout: "La operación superó el tiempo de espera.",
  http_status: "El servidor respondió con un estado HTTP inesperado.",
  version_detection: "No se pudo detectar la versión de Odoo.",
  unsupported_version: "Versión de Odoo no soportada (se admite de 15.0 a 19.0).",
  unsupported_protocol: "El protocolo elegido no está disponible para esta instancia.",
  authentication_failed: "Usuario, contraseña o API key incorrectos.",
  access_denied: "Acceso denegado por el servidor.",
  db_manager_disabled: "El gestor de bases de datos está deshabilitado en el servidor (list_db = False).",
  module_not_installed: "El módulo obd_backup no está instalado en esta base de datos.",
  database_not_listed: "El gestor de BD no lista esta base de datos (revisa dbfilter y el dominio).",
  module_api_incompatible: "La versión del módulo obd_backup no es compatible con esta aplicación.",
  rpc: "El servidor devolvió un error RPC.",
  server_backup_error: "El servidor informó un error al generar el backup.",
  prepare_timeout: "El servidor no terminó de preparar el backup a tiempo.",
  invalid_backup: "El archivo descargado no es un backup válido.",
  protocol: "Respuesta inesperada del servidor.",
  cancelled: "Operación cancelada.",
  io: "Error de lectura o escritura en disco.",

  // Almacenamiento
  storage_not_configured: "El destino de almacenamiento no está configurado.",
  storage_auth_expired: "La autorización de Google Drive expiró o fue revocada. Vuelve a conectar la cuenta.",
  storage_authorization_denied: "Se rechazó la autorización en Google.",
  storage_quota_exceeded: "No hay espacio suficiente en el destino.",
  storage_rate_limited: "Demasiadas solicitudes al destino; se reintentará más tarde.",
  storage_not_found: "No se encontró el archivo o carpeta en el destino.",
  storage_transient: "Error temporal del destino de almacenamiento.",
  storage_fatal: "Error del destino de almacenamiento.",

  // Aplicación
  not_found: "No se encontró el elemento solicitado.",
  invalid_input: "Hay datos inválidos en el formulario.",
  backup_in_progress: "Ya hay un backup en curso para esta instancia.",
  job_not_found: "El backup ya no está en curso.",
  drive_not_configured: "Configura primero el cliente OAuth de Google Drive.",
  drive_not_connected: "Google Drive no está conectado.",
  drive_connect_in_progress: "Ya hay una conexión con Google Drive en curso.",
  download_dir_invalid: "La carpeta de descarga no existe o no se puede escribir.",
  missing_secret: "Falta la contraseña o API key de la instancia.",
  missing_master_password: "El gestor de bases de datos requiere la contraseña maestra.",
  no_transport_available:
    "No hay transporte disponible: instala el módulo obd_backup o habilita list_db con contraseña maestra.",
  api_key_required: "El módulo obd_backup requiere una API key (no una contraseña).",
  password_required: "Sin llavero del sistema se necesita una contraseña maestra.",
  password_too_short: "La contraseña maestra es demasiado corta.",
  duplicate_name: "Ya existe otra instancia con ese nombre.",
  file_missing: "El archivo del backup ya no existe en el disco.",
  browser_open_failed: "No se pudo abrir el navegador para autorizar Google Drive.",
  history_db: "No se pudo leer o guardar el historial de backups.",
  interrupted: "La aplicación se cerró mientras se hacía el backup.",

  // Plugins (docs/plugins.md)
  plugin_not_found: "No se encontró el plugin.",
  plugin_disabled: "El plugin está desactivado.",
  plugin_invalid: "El plugin tiene errores y no se puede cargar.",
  plugin_no_settings: "Este plugin no tiene ajustes.",
  plugin_window_not_found: "El plugin no declara esa ventana.",
  plugin_storage_limit: "El plugin superó el límite de almacenamiento (1 MiB).",
  plugin_storage_key_invalid: "Clave de almacenamiento inválida (solo letras, números, «.», «_» y «-», hasta 128).",
  plugin_path_invalid: "La ruta del plugin no es válida.",
  plugin_settings_invalid: "Hay ajustes del plugin con valores inválidos.",
  plugin_store_corrupted: "No se pudieron leer los datos guardados del plugin.",
  bridge_invalid_request: "El plugin envió una petición inválida.",
  bridge_unknown_method: "El plugin llamó a una función que no existe.",
  bridge_timeout: "La aplicación no respondió a tiempo al plugin.",
  unsupported_surface: "Esta acción no está disponible desde una ventana de plugin.",

  internal: "Error interno de la aplicación.",
};

export const FALLBACK_ERROR_MESSAGE = "Ocurrió un error inesperado.";

function isCommandError(value: unknown): value is CommandError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as { code?: unknown }).code === "string" &&
    typeof (value as { message?: unknown }).message === "string"
  );
}

/** Normaliza cualquier rechazo (`invoke`, excepciones JS, strings) a `AppError`. */
export function toAppError(error: unknown): AppError {
  if (error instanceof AppError) return error;
  if (isCommandError(error)) return new AppError(error.code, error.message);
  if (error instanceof Error) return new AppError("internal", error.message);
  if (typeof error === "string") {
    try {
      const parsed: unknown = JSON.parse(error);
      if (isCommandError(parsed)) return new AppError(parsed.code, parsed.message);
    } catch {
      // not JSON
    }
    return new AppError("internal", error);
  }
  return new AppError("internal", String(error));
}

/** Mensaje en español para un código de error. */
export function messageForCode(code: string): string {
  return ERROR_MESSAGES[code] ?? FALLBACK_ERROR_MESSAGE;
}

/** Mensaje en español para cualquier error. */
export function errorMessage(error: unknown): string {
  return messageForCode(toAppError(error).code);
}
