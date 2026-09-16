import { z } from "zod";

import type { InstanceInput, InstanceView, ProbeRequest } from "../../lib/types";

const DATABASE_PATTERN = /^[A-Za-z0-9][A-Za-z0-9_.-]*$/;
const IPV4_PATTERN = /^\d{1,3}(\.\d{1,3}){3}$/;

/** `true` si es una URL absoluta http:// o https:// con host. */
export function isHttpUrl(value: string): boolean {
  try {
    const url = new URL(value.trim());
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname.length > 0;
  } catch {
    return false;
  }
}

/** Quita espacios y barras finales: "https://x.com/ " → "https://x.com". */
export function normalizeUrl(value: string): string {
  return value.trim().replace(/\/+$/, "");
}

/**
 * Deduce el nombre de base de datos desde el subdominio (dbfilter = ^%d$), igual que
 * `obd_odoo::database_from_host`: primer segmento del host sin "www."; null para IPs
 * y hosts de un solo segmento.
 */
export function databaseFromUrl(value: string): string | null {
  if (!isHttpUrl(value)) return null;
  const host = new URL(value.trim()).hostname.toLowerCase();
  if (host.startsWith("[") || IPV4_PATTERN.test(host)) return null;
  let labels = host.split(".").filter(Boolean);
  if (labels[0] === "www" && labels.length > 2) labels = labels.slice(1);
  if (labels.length < 2) return null;
  const candidate = labels[0];
  return candidate && DATABASE_PATTERN.test(candidate) ? candidate : null;
}

export const secretKindSchema = z.enum(["password", "api_key"]);
export const transportSchema = z.enum(["auto", "db_manager", "obd_module"]);
export const protocolSchema = z.enum(["auto", "xml_rpc", "json2"]);

export const instanceFormBaseSchema = z.object({
  name: z.string().trim().min(1, "Ingresa un nombre.").max(80, "Máximo 80 caracteres."),
  url: z
    .string()
    .trim()
    .min(1, "Ingresa la URL de la instancia.")
    .refine(isHttpUrl, "Usa una URL válida que empiece con http:// o https://."),
  database: z
    .string()
    .trim()
    .min(1, "Ingresa el nombre de la base de datos.")
    .regex(DATABASE_PATTERN, "Solo letras, números, punto, guion y guion bajo."),
  login: z.string().trim().min(1, "Ingresa el usuario."),
  secretKind: secretKindSchema,
  secret: z.string(),
  masterPassword: z.string(),
  removeMasterPassword: z.boolean(),
  transport: transportSchema,
  protocol: protocolSchema,
  includeFilestore: z.boolean(),
  uploadToDrive: z.boolean(),
});

export type InstanceFormValues = z.infer<typeof instanceFormBaseSchema>;

export type InstanceFormContext = {
  /** Editando una instancia que ya tiene secreto guardado. */
  hasSecret: boolean;
  /** Editando una instancia que ya tiene contraseña maestra guardada. */
  hasMasterPassword: boolean;
};

/** Esquema con reglas que dependen de los secretos ya guardados. */
export function makeInstanceFormSchema(ctx: InstanceFormContext) {
  return instanceFormBaseSchema.superRefine((values, issues) => {
    if (!ctx.hasSecret && values.secret.length === 0) {
      issues.addIssue({
        code: "custom",
        path: ["secret"],
        message: values.secretKind === "api_key" ? "Ingresa la API key." : "Ingresa la contraseña.",
      });
    }
    if (values.protocol === "json2" && values.secretKind !== "api_key") {
      issues.addIssue({ code: "custom", path: ["protocol"], message: "JSON-2 requiere una API key." });
    }
    if (values.transport === "obd_module" && values.secretKind !== "api_key") {
      issues.addIssue({
        code: "custom",
        path: ["transport"],
        message: "El módulo obd_backup requiere una API key.",
      });
    }
    const hasMaster = !values.removeMasterPassword && (values.masterPassword.length > 0 || ctx.hasMasterPassword);
    if (values.transport === "db_manager" && !hasMaster) {
      issues.addIssue({
        code: "custom",
        path: ["masterPassword"],
        message: "El gestor de BD requiere la contraseña maestra.",
      });
    }
  });
}

export function defaultFormValues(instance?: InstanceView | null): InstanceFormValues {
  return {
    name: instance?.name ?? "",
    url: instance?.url ?? "",
    database: instance?.database ?? "",
    login: instance?.login ?? "",
    secretKind: instance?.secretKind ?? "api_key",
    secret: "",
    masterPassword: "",
    removeMasterPassword: false,
    transport: instance?.transport ?? "auto",
    protocol: instance?.protocol ?? "auto",
    includeFilestore: instance?.includeFilestore ?? true,
    uploadToDrive: instance?.uploadToDrive ?? false,
  };
}

/** Convierte el formulario en `InstanceInput` (secretos vacíos = conservar). */
export function buildInstanceInput(values: InstanceFormValues, existing?: InstanceView | null): InstanceInput {
  const input: InstanceInput = {
    name: values.name.trim(),
    url: normalizeUrl(values.url),
    database: values.database.trim(),
    login: values.login.trim(),
    secretKind: values.secretKind,
    transport: values.transport,
    protocol: values.protocol,
    includeFilestore: values.includeFilestore,
    uploadToDrive: values.uploadToDrive,
  };
  if (existing) input.id = existing.id;
  if (values.secret.length > 0) input.secret = values.secret;
  if (values.removeMasterPassword) {
    if (existing) input.masterPassword = null;
  } else if (values.masterPassword.length > 0) {
    input.masterPassword = values.masterPassword;
  }
  return input;
}

/** Solicitud de prueba de conexión con lo escrito en el formulario. */
export function buildProbeRequest(values: InstanceFormValues, existing?: InstanceView | null): ProbeRequest {
  const request: ProbeRequest = {
    url: normalizeUrl(values.url),
    secretKind: values.secretKind,
    protocol: values.protocol,
  };
  if (existing) request.instanceId = existing.id;
  const database = values.database.trim();
  if (database) request.database = database;
  const login = values.login.trim();
  if (login) request.login = login;
  if (values.secret.length > 0) request.secret = values.secret;
  if (!values.removeMasterPassword && values.masterPassword.length > 0) {
    request.masterPassword = values.masterPassword;
  }
  return request;
}

/** Solicitud de prueba para una instancia guardada (usa los secretos almacenados). */
export function probeRequestForInstance(instance: InstanceView): ProbeRequest {
  return {
    instanceId: instance.id,
    url: instance.url,
    database: instance.database,
    login: instance.login,
    secretKind: instance.secretKind,
    protocol: instance.protocol,
  };
}
