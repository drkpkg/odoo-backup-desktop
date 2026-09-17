// Formularios generados desde el esquema de ajustes de un plugin (docs/plugins.md,
// "Esquema de ajustes"). Lógica pura: estado del borrador, validación y armado del envío.

import { toAppError } from "../../lib/errors";
import type { PluginSettings, SchemaProperty, SettingsSchema } from "../../lib/types";

export type FieldKind = "text" | "url" | "email" | "multiline" | "secret" | "number" | "integer" | "boolean" | "enum";

/** Valores del formulario: texto para entradas, booleanos para interruptores, índice para `enum`. */
export type Draft = Record<string, string | boolean>;
export type FieldErrors = Record<string, string>;

/** Clave para errores que no pertenecen a un campo conocido. */
export const FORM_ERROR_KEY = "_form";

export function isSecretProperty(prop: SchemaProperty): boolean {
  return prop.secret === true || prop.format === "password";
}

export function fieldKind(prop: SchemaProperty): FieldKind {
  if (prop.type === "boolean") return "boolean";
  if (Array.isArray(prop.enum) && prop.enum.length > 0) return "enum";
  if (prop.type === "integer") return "integer";
  if (prop.type === "number") return "number";
  if (isSecretProperty(prop)) return "secret";
  if (prop.format === "url") return "url";
  if (prop.format === "email") return "email";
  if (prop.format === "multiline") return "multiline";
  return "text";
}

/** Claves en el orden del manifiesto (`propertyOrder`), sin perder ninguna propiedad. */
export function orderedKeys(schema: SettingsSchema): string[] {
  const known = Object.keys(schema.properties);
  const ordered = (schema.propertyOrder ?? []).filter((key) => key in schema.properties);
  return [...ordered, ...known.filter((key) => !ordered.includes(key))];
}

export function isRequired(schema: SettingsSchema, key: string): boolean {
  return (schema.required ?? []).includes(key);
}

export function initialDraft(schema: SettingsSchema, settings: Pick<PluginSettings, "values">): Draft {
  const draft: Draft = {};
  for (const key of orderedKeys(schema)) {
    const prop = schema.properties[key];
    if (!prop) continue;
    const kind = fieldKind(prop);
    const stored = key in settings.values ? settings.values[key] : undefined;
    const value = stored === undefined || stored === null ? (kind === "secret" ? undefined : prop.default) : stored;
    switch (kind) {
      case "secret":
        draft[key] = "";
        break;
      case "boolean":
        draft[key] = value === true;
        break;
      case "enum": {
        const index = (prop.enum ?? []).findIndex((option) => option === value);
        draft[key] = index >= 0 ? String(index) : "";
        break;
      }
      case "number":
      case "integer":
        draft[key] = typeof value === "number" && Number.isFinite(value) ? String(value) : "";
        break;
      default:
        draft[key] = typeof value === "string" ? value : "";
    }
  }
  return draft;
}

function formatNumber(value: number): string {
  return Number.isInteger(value) ? String(value) : String(value);
}

const EMAIL = /^[^\s@]+@[^\s@]+\.[^\s@]+$/;

function isHttpUrl(text: string): boolean {
  try {
    const url = new URL(text);
    return (url.protocol === "http:" || url.protocol === "https:") && url.hostname !== "";
  } catch {
    return false;
  }
}

/** Mensaje en español para un código de error de campo (cliente o servidor). */
export function fieldErrorMessage(code: string, prop: SchemaProperty | undefined, fallback?: string): string {
  switch (code) {
    case "required":
      return "Campo obligatorio.";
    case "type":
      return prop?.type === "integer"
        ? "Debe ser un número entero."
        : prop?.type === "number"
          ? "Debe ser un número."
          : prop?.type === "boolean"
            ? "Debe ser verdadero o falso."
            : "Tipo de valor inválido.";
    case "minimum":
      return prop?.minimum !== undefined ? `Debe ser mayor o igual a ${formatNumber(prop.minimum)}.` : "Valor demasiado bajo.";
    case "maximum":
      return prop?.maximum !== undefined ? `Debe ser menor o igual a ${formatNumber(prop.maximum)}.` : "Valor demasiado alto.";
    case "min_length":
      return prop?.minLength !== undefined ? `Debe tener al menos ${prop.minLength} caracteres.` : "Texto demasiado corto.";
    case "max_length":
      return prop?.maxLength !== undefined ? `Debe tener como máximo ${prop.maxLength} caracteres.` : "Texto demasiado largo.";
    case "pattern":
      return "No tiene el formato esperado.";
    case "format":
      return prop?.format === "email"
        ? "Debe ser un correo electrónico válido."
        : prop?.format === "url"
          ? "Debe ser una URL http:// o https://."
          : "Formato inválido.";
    case "enum":
      return "Elige una de las opciones.";
    case "unknown_field":
      return "Campo desconocido para este plugin.";
    default:
      return fallback ?? "Valor inválido.";
  }
}

function checkText(prop: SchemaProperty, text: string, kind: FieldKind): string | null {
  const length = [...text].length;
  if (prop.minLength !== undefined && length < prop.minLength) return "min_length";
  if (prop.maxLength !== undefined && length > prop.maxLength) return "max_length";
  if (prop.pattern) {
    try {
      if (!new RegExp(prop.pattern, "u").test(text)) return "pattern";
    } catch {
      // Patrón inválido: lo valida el backend.
    }
  }
  if (kind === "url" && !isHttpUrl(text)) return "format";
  if (kind === "email" && !EMAIL.test(text)) return "format";
  return null;
}

/** Convierte texto a número según el tipo; `null` si no es válido. */
export function coerceNumber(text: string, integer: boolean): number | null {
  const trimmed = text.trim();
  if (trimmed === "") return null;
  const value = Number(trimmed);
  if (!Number.isFinite(value)) return null;
  if (integer && !Number.isInteger(value)) return null;
  return value;
}

/** Validación del lado del cliente (el backend vuelve a validar). */
export function validateDraft(schema: SettingsSchema, draft: Draft, secretsSet: string[], removedSecrets: string[]): FieldErrors {
  const errors: FieldErrors = {};
  for (const key of orderedKeys(schema)) {
    const prop = schema.properties[key];
    if (!prop) continue;
    const kind = fieldKind(prop);
    const required = isRequired(schema, key);
    const raw = draft[key];
    let code: string | null = null;

    switch (kind) {
      case "boolean":
        break;
      case "secret": {
        const text = typeof raw === "string" ? raw : "";
        if (text !== "") code = checkText(prop, text, kind);
        else if (required && (!secretsSet.includes(key) || removedSecrets.includes(key))) code = "required";
        break;
      }
      case "enum": {
        const text = typeof raw === "string" ? raw : "";
        if (text === "") {
          if (required) code = "required";
        } else {
          const index = Number(text);
          if (!Number.isInteger(index) || index < 0 || index >= (prop.enum ?? []).length) code = "enum";
        }
        break;
      }
      case "number":
      case "integer": {
        const text = typeof raw === "string" ? raw : "";
        if (text.trim() === "") {
          if (required) code = "required";
          break;
        }
        const value = coerceNumber(text, kind === "integer");
        if (value === null) code = "type";
        else if (prop.minimum !== undefined && value < prop.minimum) code = "minimum";
        else if (prop.maximum !== undefined && value > prop.maximum) code = "maximum";
        break;
      }
      default: {
        const text = typeof raw === "string" ? raw : "";
        if (text === "") {
          if (required) code = "required";
        } else {
          code = checkText(prop, text, kind);
        }
      }
    }

    if (code) errors[key] = fieldErrorMessage(code, prop);
  }
  return errors;
}

/**
 * Valores a enviar a `save_plugin_settings`: campos no secretos siempre (vacío opcional = `null`);
 * secretos solo si se escribieron (string) o se quitaron (`null`).
 */
export function buildSubmission(schema: SettingsSchema, draft: Draft, removedSecrets: string[]): Record<string, unknown> {
  const values: Record<string, unknown> = {};
  for (const key of orderedKeys(schema)) {
    const prop = schema.properties[key];
    if (!prop) continue;
    const kind = fieldKind(prop);
    const raw = draft[key];

    switch (kind) {
      case "secret": {
        const text = typeof raw === "string" ? raw : "";
        if (text !== "") values[key] = text;
        else if (removedSecrets.includes(key)) values[key] = null;
        break;
      }
      case "boolean":
        values[key] = raw === true;
        break;
      case "enum": {
        const text = typeof raw === "string" ? raw : "";
        const option = text === "" ? undefined : (prop.enum ?? [])[Number(text)];
        values[key] = option === undefined ? null : option;
        break;
      }
      case "number":
      case "integer": {
        const text = typeof raw === "string" ? raw : "";
        values[key] = coerceNumber(text, kind === "integer");
        break;
      }
      default: {
        const text = typeof raw === "string" ? raw : "";
        values[key] = text === "" ? null : text;
      }
    }
  }
  return values;
}

type ServerFieldError = { field: string; code: string; message: string };

function isServerFieldError(value: unknown): value is ServerFieldError {
  return (
    typeof value === "object" &&
    value !== null &&
    typeof (value as ServerFieldError).field === "string" &&
    typeof (value as ServerFieldError).code === "string"
  );
}

/** Errores por campo de `plugin_settings_invalid` (`message` es JSON). `null` si es otro error. */
export function parseServerErrors(error: unknown, schema: SettingsSchema): FieldErrors | null {
  const appError = toAppError(error);
  if (appError.code !== "plugin_settings_invalid") return null;
  let parsed: unknown;
  try {
    parsed = JSON.parse(appError.message);
  } catch {
    return { [FORM_ERROR_KEY]: fieldErrorMessage("invalid", undefined, "Hay ajustes con valores inválidos.") };
  }
  if (!Array.isArray(parsed)) return { [FORM_ERROR_KEY]: "Hay ajustes con valores inválidos." };
  const errors: FieldErrors = {};
  for (const item of parsed) {
    if (!isServerFieldError(item)) continue;
    const prop = schema.properties[item.field];
    const key = prop ? item.field : FORM_ERROR_KEY;
    const message = prop ? fieldErrorMessage(item.code, prop, item.message) : `${item.field}: ${fieldErrorMessage(item.code, undefined, item.message)}`;
    errors[key] = errors[key] ? `${errors[key]} ${message}` : message;
  }
  return Object.keys(errors).length > 0 ? errors : { [FORM_ERROR_KEY]: "Hay ajustes con valores inválidos." };
}

/**
 * Validación con semántica de backend, usada por el mock (`save_plugin_settings`): devuelve los
 * errores con códigos del contrato o los valores resultantes.
 */
export function validateSubmissionLikeBackend(
  schema: SettingsSchema,
  submitted: Record<string, unknown>,
  current: Record<string, unknown>,
  existingSecrets: string[],
): { ok: true; values: Record<string, unknown>; setSecrets: Record<string, string>; removeSecrets: string[] } | { ok: false; errors: ServerFieldError[] } {
  const errors: ServerFieldError[] = [];
  const values: Record<string, unknown> = {};
  const setSecrets: Record<string, string> = {};
  const removeSecrets: string[] = [];

  for (const key of Object.keys(submitted)) {
    if (!(key in schema.properties)) errors.push({ field: key, code: "unknown_field", message: "unknown field" });
  }

  for (const key of orderedKeys(schema)) {
    const prop = schema.properties[key];
    if (!prop) continue;
    const required = isRequired(schema, key);
    const has = Object.prototype.hasOwnProperty.call(submitted, key);
    const value = submitted[key];

    if (isSecretProperty(prop)) {
      if (has && (value === null || value === "")) {
        if (required) errors.push({ field: key, code: "required", message: "is required" });
        else removeSecrets.push(key);
      } else if (has) {
        if (typeof value !== "string") errors.push({ field: key, code: "type", message: "must be a string" });
        else {
          const code = checkText(prop, value, "secret");
          if (code) errors.push({ field: key, code, message: code });
          else setSecrets[key] = value;
        }
      } else if (required && !existingSecrets.includes(key)) {
        errors.push({ field: key, code: "required", message: "is required" });
      }
      continue;
    }

    // Ausente = conservar; `null` = volver al valor por defecto (docs/plugins.md, "Reglas de valores").
    const effective = has ? (value === null ? prop.default : value) : key in current ? current[key] : prop.default;
    if (effective === undefined || effective === null || (required && effective === "")) {
      if (required) errors.push({ field: key, code: "required", message: "is required" });
      continue;
    }
    const kind = fieldKind(prop);
    let code: string | null = null;
    if (prop.type === "boolean") {
      if (typeof effective !== "boolean") code = "type";
    } else if (prop.type === "number" || prop.type === "integer") {
      if (typeof effective !== "number" || !Number.isFinite(effective)) code = "type";
      else if (prop.type === "integer" && !Number.isInteger(effective)) code = "type";
      else if (prop.minimum !== undefined && effective < prop.minimum) code = "minimum";
      else if (prop.maximum !== undefined && effective > prop.maximum) code = "maximum";
    } else if (typeof effective !== "string") {
      code = "type";
    } else {
      code = checkText(prop, effective, kind === "enum" ? "text" : kind);
    }
    if (!code && prop.enum && !prop.enum.includes(effective as string | number)) code = "enum";
    if (code) errors.push({ field: key, code, message: code });
    else values[key] = effective;
  }

  return errors.length > 0 ? { ok: false, errors } : { ok: true, values, setSecrets, removeSecrets };
}
