import type { Tone } from "../../components/Badge";
import { formatOdooVersion } from "../../lib/format";
import { PROTOCOL_LABELS, TRANSPORT_LABELS } from "../../lib/labels";
import type { InstanceView, TransportKind } from "../../lib/types";

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

  const version = formatOdooVersion(probe.version);
  const transport: TransportKind | null = instance.transport === "auto" ? probe.recommendedTransport : instance.transport;
  const technical = [probe.protocol ? PROTOCOL_LABELS[probe.protocol] : null, transport ? TRANSPORT_LABELS[transport] : null]
    .filter(Boolean)
    .join(" · ") || null;
  const base = { version, technical };

  if (!probe.supported) {
    return { ...base, tone: "warning", label: "Versión no soportada", reason: `Odoo ${version} no está entre las versiones 15.0 a 19.0.` };
  }
  if (probe.auth.status === "failed") {
    return { ...base, tone: "danger", label: "Requiere atención", reason: "Las credenciales fueron rechazadas en la última prueba." };
  }

  const attention = (reason: string): ConnectionState => ({ ...base, tone: "warning", label: "Requiere atención", reason });
  if (!transport) {
    return attention("No hay un método de respaldo disponible: instala el módulo obd_backup o habilita el gestor de BD.");
  }
  if (transport === "db_manager") {
    if (probe.dbManager.status !== "ok") return attention("El gestor de bases de datos no está disponible en el servidor.");
    if (!instance.hasMasterPassword) return attention("Falta la contraseña maestra para usar el gestor de BD.");
  }
  if (transport === "obd_module") {
    if (probe.module.status !== "ok") return attention("El módulo obd_backup no respondió en la última prueba.");
    if (instance.secretKind !== "api_key") return attention("El módulo obd_backup necesita una API key.");
  }

  return { ...base, tone: "success", label: "Lista", reason: "La última prueba de conexión fue correcta." };
}
