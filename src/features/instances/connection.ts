import type { Tone } from "../../components/Badge";
import { messageForCode } from "../../lib/errors";
import { formatOdooVersion } from "../../lib/format";
import { PROTOCOL_LABELS, TRANSPORT_LABELS } from "../../lib/labels";
import type { InstanceView, TransportKind } from "../../lib/types";
import { assessReadiness, type Readiness } from "./readiness";

export type ConnectionState = {
  tone: Tone;
  /** Texto corto para la tabla. */
  label: string;
  /** Qué revisar (o por qué está lista), para tooltip y lectores de pantalla. */
  reason: string;
  /** "Odoo 17.0", o null si nunca se probó. */
  version: string | null;
  /** "XML-RPC · Gestor de BD" (lo detectado o lo elegido). */
  technical: string | null;
};

const ATTENTION_REASONS: Record<Exclude<Readiness["kind"], "ready" | "auth_failed">, string> = {
  credentials_missing: "La última prueba se hizo sin credenciales o sin base de datos.",
  master_password_missing: "Falta la contraseña maestra para usar el gestor de BD.",
  db_manager_disabled: "El gestor de bases de datos no está disponible en el servidor.",
  module_unavailable: "El módulo obd_backup no respondió en la última prueba.",
  api_key_required: "El módulo obd_backup necesita una API key.",
  no_method: "No hay un método de respaldo disponible: instala el módulo obd_backup o habilita el gestor de BD.",
};

/**
 * Resume la última prueba de conexión en un estado operativo:
 * Lista (se puede respaldar), Requiere atención, Versión no soportada o Sin probar.
 * Solo usa datos que ya trae `InstanceView` (sin llamadas al backend).
 */
export function connectionState(instance: InstanceView): ConnectionState {
  const probe = instance.lastProbe;
  if (!probe) {
    return {
      tone: "neutral",
      label: "Sin probar",
      reason: "Prueba la conexión para detectar la versión de Odoo y el método de respaldo.",
      version: null,
      technical: null,
    };
  }

  const readiness = assessReadiness(probe, instance);
  const version = formatOdooVersion(probe.version);
  const transport: TransportKind | null =
    readiness.kind === "ready" ? readiness.transport : instance.transport === "auto" ? null : instance.transport;
  const technical = [probe.protocol ? PROTOCOL_LABELS[probe.protocol] : null, transport ? TRANSPORT_LABELS[transport] : null]
    .filter(Boolean)
    .join(" · ") || null;
  const base = { version, technical };

  if (!probe.supported) {
    return { ...base, tone: "warning", label: "Versión no soportada", reason: `Odoo ${version} no está entre las versiones 15.0 a 19.0.` };
  }
  switch (readiness.kind) {
    case "ready":
      return { ...base, tone: "success", label: "Lista", reason: "La última prueba de conexión fue correcta." };
    case "auth_failed":
      return {
        ...base,
        tone: "danger",
        label: "Requiere atención",
        reason:
          readiness.code === "authentication_failed"
            ? "Las credenciales fueron rechazadas en la última prueba."
            : messageForCode(readiness.code),
      };
    default:
      return { ...base, tone: "warning", label: "Requiere atención", reason: ATTENTION_REASONS[readiness.kind] };
  }
}
