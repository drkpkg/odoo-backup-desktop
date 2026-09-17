// Puente plugin ↔ app (docs/plugins.md, "SDK y puente"). Funciones puras: el componente
// PluginFrame solo filtra mensajes por iframe y reenvía la respuesta.

import type { Backend } from "../../lib/backend";
import { toAppError } from "../../lib/errors";
import type { InstanceView, PluginView } from "../../lib/types";

export const BRIDGE_PROTOCOL = 1;

export type BridgeSurface = "page" | "window";
export type BridgeTheme = "light" | "dark";

export type BridgeRequest = { obd: 1; id: string; method: string; params?: unknown };
export type BridgeError = { code: string; message: string };
export type BridgeResponse =
  | { obd: 1; id: string; result: unknown }
  | { obd: 1; id: string; error: BridgeError };
export type BridgeEvent = { obd: 1; event: string; data: unknown };

export type BridgeBackend = Pick<
  Backend,
  | "getPluginSettings"
  | "savePluginSettings"
  | "pluginStorageGet"
  | "pluginStorageSet"
  | "listInstances"
  | "listHistory"
  | "openPluginWindow"
>;

export type ToastKind = "info" | "success" | "error";

/** Todo lo que el puente necesita de la app. `pluginId` sale de aquí, nunca del mensaje. */
export type BridgeHost = {
  plugin: PluginView;
  appVersion: string;
  surface: BridgeSurface;
  params: Record<string, unknown>;
  theme: () => BridgeTheme;
  backend: BridgeBackend;
  toast: (kind: ToastKind, title: string, description?: string) => void;
  /** Solo en páginas. */
  navigate?: (pageId: string, params: Record<string, unknown>) => void;
  openSettings: () => void | Promise<void>;
};

export type PluginInstance = {
  id: string;
  name: string;
  url: string;
  database: string;
  odooVersion: string | null;
  lastBackup: InstanceView["lastBackup"];
};

const STORAGE_KEY = /^[A-Za-z0-9._-]{1,128}$/;
const MAX_ID_LENGTH = 64;
const MAX_TEXT = 2000;

class BridgeFailure extends Error {
  constructor(
    readonly code: string,
    message: string,
  ) {
    super(message);
  }
}

const invalid = (message: string) => new BridgeFailure("bridge_invalid_request", message);

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function paramsObject(params: unknown): Record<string, unknown> {
  if (params === undefined || params === null) return {};
  if (!isRecord(params)) throw invalid("params must be an object");
  return params;
}

function requiredString(params: Record<string, unknown>, key: string, max = MAX_TEXT): string {
  const value = params[key];
  if (typeof value !== "string" || value.trim() === "") throw invalid(`${key} must be a non-empty string`);
  if (value.length > max) throw invalid(`${key} is too long`);
  return value;
}

function optionalParams(params: Record<string, unknown>): Record<string, unknown> {
  const value = params.params;
  if (value === undefined || value === null) return {};
  if (!isRecord(value)) throw invalid("params.params must be an object");
  return value;
}

export function toPluginInstance(instance: InstanceView): PluginInstance {
  const version = instance.lastProbe?.version;
  return {
    id: instance.id,
    name: instance.name,
    url: instance.url,
    database: instance.database,
    odooVersion: version ? (version.saas ? `saas~${version.major}.${version.minor}` : `${version.major}.${version.minor}`) : null,
    lastBackup: instance.lastBackup,
  };
}

/**
 * Valida la forma mínima de un mensaje del puente. `null` = no es para nosotros (se ignora
 * sin responder): sin `obd: 1` o sin `id` utilizable.
 */
export function parseBridgeEnvelope(data: unknown): { id: string; method: unknown; params: unknown } | null {
  if (!isRecord(data) || data.obd !== BRIDGE_PROTOCOL) return null;
  if (typeof data.id !== "string" || data.id === "" || data.id.length > MAX_ID_LENGTH) return null;
  if ("result" in data || "error" in data || "event" in data) return null;
  return { id: data.id, method: data.method, params: data.params };
}

async function dispatch(method: string, rawParams: unknown, host: BridgeHost): Promise<unknown> {
  const pluginId = host.plugin.id;
  const params = paramsObject(rawParams);

  switch (method) {
    case "context.get":
      return {
        pluginId,
        pluginVersion: host.plugin.version,
        appVersion: host.appVersion,
        theme: host.theme(),
        surface: host.surface,
        params: host.params,
      };

    case "settings.get": {
      const settings = await host.backend.getPluginSettings(pluginId);
      return { values: settings.values, secretsSet: settings.secretsSet };
    }

    case "settings.save": {
      if (!isRecord(params.values)) throw invalid("values must be an object");
      const settings = await host.backend.savePluginSettings(pluginId, params.values);
      return { values: settings.values, secretsSet: settings.secretsSet };
    }

    case "storage.get": {
      const key = requiredString(params, "key", 128);
      if (!STORAGE_KEY.test(key)) throw new BridgeFailure("plugin_storage_key_invalid", "invalid storage key");
      const value = await host.backend.pluginStorageGet(pluginId, key);
      return value === undefined ? null : value;
    }

    case "storage.set": {
      const key = requiredString(params, "key", 128);
      if (!STORAGE_KEY.test(key)) throw new BridgeFailure("plugin_storage_key_invalid", "invalid storage key");
      if (!("value" in params) || params.value === undefined) throw invalid("value is required (use null to delete)");
      await host.backend.pluginStorageSet(pluginId, key, params.value);
      return null;
    }

    case "instances.list": {
      const instances = await host.backend.listInstances();
      return instances.map(toPluginInstance);
    }

    case "history.list": {
      const args: { instanceId?: string; limit?: number } = {};
      if (params.instanceId !== undefined && params.instanceId !== null) {
        args.instanceId = requiredString(params, "instanceId", 128);
      }
      if (params.limit !== undefined && params.limit !== null) {
        const limit = params.limit;
        if (typeof limit !== "number" || !Number.isInteger(limit) || limit < 1 || limit > 1000) {
          throw invalid("limit must be an integer between 1 and 1000");
        }
        args.limit = limit;
      }
      return host.backend.listHistory(args);
    }

    case "ui.toast": {
      const kind = params.kind ?? "info";
      if (kind !== "info" && kind !== "success" && kind !== "error") throw invalid("kind must be info, success or error");
      const title = requiredString(params, "title", 200);
      let description: string | undefined;
      if (params.description !== undefined && params.description !== null) {
        if (typeof params.description !== "string" || params.description.length > MAX_TEXT) {
          throw invalid("description must be a string");
        }
        description = params.description;
      }
      host.toast(kind, title, description);
      return null;
    }

    case "ui.openWindow": {
      const windowId = requiredString(params, "windowId", 64);
      if (!host.plugin.windows.some((w) => w.id === windowId)) {
        throw new BridgeFailure("plugin_window_not_found", `window ${windowId} is not declared`);
      }
      const windowParams = optionalParams(params);
      await host.backend.openPluginWindow(pluginId, windowId, windowParams);
      return null;
    }

    case "ui.navigate": {
      if (host.surface !== "page" || !host.navigate) {
        throw new BridgeFailure("unsupported_surface", "ui.navigate is only available in pages");
      }
      const pageId = requiredString(params, "pageId", 64);
      if (!host.plugin.pages.some((p) => p.id === pageId)) throw invalid(`page ${pageId} is not declared`);
      host.navigate(pageId, optionalParams(params));
      return null;
    }

    case "ui.openSettings": {
      if (!host.plugin.hasSettings) throw new BridgeFailure("plugin_no_settings", "the plugin has no settings");
      await host.openSettings();
      return null;
    }

    default:
      throw new BridgeFailure("bridge_unknown_method", `unknown method ${method}`);
  }
}

/**
 * Procesa un mensaje recibido del iframe del plugin. Devuelve la respuesta a enviar o `null`
 * si el mensaje no pertenece al protocolo.
 */
export async function handleBridgeMessage(data: unknown, host: BridgeHost): Promise<BridgeResponse | null> {
  const envelope = parseBridgeEnvelope(data);
  if (!envelope) return null;
  const { id } = envelope;
  if (typeof envelope.method !== "string" || envelope.method === "") {
    return { obd: 1, id, error: { code: "bridge_invalid_request", message: "method must be a string" } };
  }
  try {
    const result = await dispatch(envelope.method, envelope.params, host);
    return { obd: 1, id, result: result === undefined ? null : result };
  } catch (err) {
    if (err instanceof BridgeFailure) return { obd: 1, id, error: { code: err.code, message: err.message } };
    const appError = toAppError(err);
    return { obd: 1, id, error: { code: appError.code, message: appError.message } };
  }
}

export function bridgeEvent(event: string, data: unknown): BridgeEvent {
  return { obd: 1, event, data };
}

/** URL de una página/ventana del plugin con la revisión para forzar recarga. */
export function pluginAssetUrl(plugin: Pick<PluginView, "baseUrl" | "revision">, path: string): string {
  const base = plugin.baseUrl.endsWith("/") ? plugin.baseUrl : `${plugin.baseUrl}/`;
  const clean = path.replace(/^\/+/, "");
  return `${base}${clean}?rev=${plugin.revision}`;
}
