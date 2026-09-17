import { messageForCode } from "../../lib/errors";
import type { ProbeReport, ProtocolPreference, SecretKind, TransportKind, TransportPreference } from "../../lib/types";

/** Configuración actual con la que se evalúa una prueba (la instancia guardada o el formulario). */
export type ReadinessInput = {
  transport: TransportPreference;
  protocol: ProtocolPreference;
  secretKind: SecretKind;
  /** Hay contraseña maestra guardada o escrita (y no se va a eliminar). */
  hasMasterPassword: boolean;
};

export type Readiness =
  | { kind: "ready"; transport: TransportKind }
  | { kind: "credentials_missing"; missing: "credentials" | "database" }
  | { kind: "auth_failed"; code: string }
  | { kind: "master_password_missing"; moduleAlternative: boolean }
  | { kind: "db_manager_disabled" }
  | { kind: "module_unavailable"; code: string | null }
  | { kind: "api_key_required" }
  | { kind: "no_method"; moduleCode: string | null };

/**
 * ¿Se puede respaldar con esta configuración según la última prueba? Replica la resolución de
 * `backup.rs` (`resolve_transport`): en automático, módulo obd_backup con API key y si no, gestor
 * de BD con contraseña maestra. Usa la configuración actual y no solo `recommendedTransport`,
 * porque la contraseña maestra o la credencial pueden cambiar después de probar.
 */
export function assessReadiness(report: ProbeReport, input: ReadinessInput): Readiness {
  const { auth, module, dbManager } = report;
  if (auth.status === "skipped") {
    return { kind: "credentials_missing", missing: auth.reason === "no_database" ? "database" : "credentials" };
  }
  if (auth.status === "failed") return { kind: "auth_failed", code: auth.code };

  const apiKey = input.secretKind === "api_key";
  const moduleOk = module.status === "ok";
  const dbManagerOk = dbManager.status === "ok";
  const moduleCode = module.status === "failed" ? module.code : null;

  switch (input.transport) {
    case "db_manager":
      if (!dbManagerOk) return { kind: "db_manager_disabled" };
      return input.hasMasterPassword ? { kind: "ready", transport: "db_manager" } : { kind: "master_password_missing", moduleAlternative: false };
    case "obd_module":
      if (!moduleOk) return { kind: "module_unavailable", code: moduleCode };
      return apiKey ? { kind: "ready", transport: "obd_module" } : { kind: "api_key_required" };
    case "auto":
      if (moduleOk && apiKey) return { kind: "ready", transport: "obd_module" };
      if (dbManagerOk && input.hasMasterPassword) return { kind: "ready", transport: "db_manager" };
      if (moduleOk) return { kind: "api_key_required" };
      if (dbManagerOk) return { kind: "master_password_missing", moduleAlternative: true };
      return { kind: "no_method", moduleCode };
  }
}

/** Arreglo que la UI puede aplicar con un clic. */
export type GuidanceAction = "add_master_password" | "use_api_key" | "use_auto_transport" | "use_auto_protocol";

export type Guidance = {
  tone: "success" | "info" | "warning" | "danger";
  title: string;
  detail: string;
  action: GuidanceAction | null;
};

export const GUIDANCE_ACTION_LABELS: Record<GuidanceAction, string> = {
  add_master_password: "Agregar contraseña maestra",
  use_api_key: "Usar API key",
  use_auto_transport: "Usar método automático",
  use_auto_protocol: "Usar protocolo automático",
};

/** El siguiente paso concreto para dejar la instancia lista para respaldar. */
export function probeGuidance(readiness: Readiness, input: ReadinessInput): Guidance {
  const secret = input.secretKind === "api_key" ? "la API key" : "la contraseña";
  switch (readiness.kind) {
    case "ready":
      return readiness.transport === "obd_module"
        ? {
            tone: "success",
            title: "Lista para respaldar",
            detail: "Se usará el módulo obd_backup con la API key: no necesita la contraseña maestra y funciona aunque list_db esté deshabilitado.",
            action: null,
          }
        : {
            tone: "success",
            title: "Lista para respaldar",
            detail: "Se usará el gestor de bases de datos; la contraseña maestra se envía al servidor en cada respaldo.",
            action: null,
          };
    case "credentials_missing":
      return readiness.missing === "database"
        ? {
            tone: "info",
            title: "Indica la base de datos",
            detail: "No se pudo deducir del subdominio. Escríbela para comprobar el acceso.",
            action: null,
          }
        : {
            tone: "info",
            title: "Completa las credenciales",
            detail: `Escribe el usuario y ${secret} para comprobar el acceso y detectar el método de respaldo.`,
            action: null,
          };
    case "auth_failed":
      if (readiness.code === "authentication_failed") {
        return { tone: "danger", title: "Credenciales rechazadas", detail: `Revisa el usuario, ${secret} y la base de datos.`, action: null };
      }
      if (readiness.code === "unsupported_protocol") {
        return {
          tone: "danger",
          title: "Protocolo no disponible",
          detail: "JSON-2 necesita Odoo 19 o superior y una API key.",
          action: input.protocol !== "auto" ? "use_auto_protocol" : input.secretKind !== "api_key" ? "use_api_key" : null,
        };
      }
      return { tone: "danger", title: "No se pudo iniciar sesión", detail: messageForCode(readiness.code), action: null };
    case "master_password_missing":
      return {
        tone: "warning",
        title: "Siguiente paso: guarda la contraseña maestra",
        detail: readiness.moduleAlternative
          ? "El gestor de bases de datos del servidor está habilitado: con la contraseña maestra podrás respaldar esta instancia. También puedes instalar el módulo obd_backup y usar una API key."
          : "El método Gestor de BD necesita la contraseña maestra de Odoo (admin_passwd).",
        action: "add_master_password",
      };
    case "db_manager_disabled":
      return {
        tone: "warning",
        title: "El gestor de BD está deshabilitado",
        detail: "El servidor tiene list_db = False. Instala el módulo obd_backup y usa una API key, o elige el método automático.",
        action: "use_auto_transport",
      };
    case "module_unavailable":
      return {
        tone: "warning",
        title: "El módulo obd_backup no está disponible",
        detail: `${messageForCode(readiness.code ?? "module_not_installed")} Instálalo en Odoo o elige el método automático.`,
        action: "use_auto_transport",
      };
    case "api_key_required":
      return {
        tone: "warning",
        title: "Siguiente paso: usa una API key",
        detail: "El módulo obd_backup está instalado, pero solo acepta API keys. Créala en Odoo: Preferencias → Seguridad de la cuenta → Nueva API key.",
        action: "use_api_key",
      };
    case "no_method":
      return {
        tone: "warning",
        title: "Falta configurar un método de respaldo",
        detail:
          readiness.moduleCode === "module_not_installed" || readiness.moduleCode === null
            ? "El gestor de BD está deshabilitado (list_db = False) y el módulo obd_backup no está instalado. Instala obd_backup en Odoo y usa una API key."
            : `El gestor de BD está deshabilitado (list_db = False) y el módulo obd_backup no se puede usar: ${messageForCode(readiness.moduleCode)}`,
        action: input.secretKind !== "api_key" ? "use_api_key" : null,
      };
  }
}
