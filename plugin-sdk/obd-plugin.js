// SDK de plugins de Odoo Backup Desktop (API v1). Sin dependencias.
// La app lo sirve en `/_sdk/obd-plugin.js`; ver docs/plugins.md.
//
//   import obd from "../../_sdk/obd-plugin.js";
//   const ctx = await obd.context();

export const PROTOCOL_VERSION = 1;
export const DEFAULT_TIMEOUT_MS = 30000;

/** Error del puente: `code` estable (p. ej. `bridge_timeout`) y `message` técnico. */
export class ObdError extends Error {
  constructor(code, message) {
    super(message || code);
    this.name = "ObdError";
    this.code = code;
  }
}

function isRecord(value) {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** `true` si `data` es una respuesta del puente (`{ obd: 1, id, result | error }`). */
export function isBridgeResponse(data) {
  return isRecord(data) && data.obd === PROTOCOL_VERSION && typeof data.id === "string" && ("result" in data || "error" in data);
}

/** `true` si `data` es un evento del puente (`{ obd: 1, event, data }`). */
export function isBridgeEvent(data) {
  return isRecord(data) && data.obd === PROTOCOL_VERSION && typeof data.event === "string" && !("id" in data);
}

/**
 * Crea un cliente sobre un transporte:
 * - `send(message)`: envía una petición a la app.
 * - `subscribe(handler)`: registra el receptor de mensajes de la app; devuelve una función para quitarlo.
 * - `timeoutMs`: tiempo máximo de espera por respuesta.
 */
export function createObdClient({ send, subscribe, timeoutMs = DEFAULT_TIMEOUT_MS, setTimer = setTimeout, clearTimer = clearTimeout }) {
  let counter = 0;
  const pending = new Map();
  const listeners = new Map();

  const unsubscribe = subscribe((data) => {
    if (isBridgeResponse(data)) {
      const entry = pending.get(data.id);
      if (!entry) return;
      pending.delete(data.id);
      clearTimer(entry.timer);
      if ("error" in data) {
        const error = isRecord(data.error) ? data.error : {};
        entry.reject(new ObdError(String(error.code ?? "bridge_error"), String(error.message ?? "")));
      } else {
        entry.resolve(data.result);
      }
      return;
    }
    if (isBridgeEvent(data)) {
      for (const callback of [...(listeners.get(data.event) ?? [])]) {
        try {
          callback(data.data);
        } catch (error) {
          console.error("[obd] event listener failed", error);
        }
      }
    }
  });

  function call(method, params) {
    counter += 1;
    const id = `obd-${counter}`;
    return new Promise((resolve, reject) => {
      const timer = setTimer(() => {
        if (!pending.has(id)) return;
        pending.delete(id);
        reject(new ObdError("bridge_timeout", `no response for ${method} after ${timeoutMs} ms`));
      }, timeoutMs);
      pending.set(id, { resolve, reject, timer });
      try {
        send(params === undefined ? { obd: PROTOCOL_VERSION, id, method } : { obd: PROTOCOL_VERSION, id, method, params });
      } catch (error) {
        pending.delete(id);
        clearTimer(timer);
        reject(new ObdError("bridge_unavailable", error instanceof Error ? error.message : String(error)));
      }
    });
  }

  function on(event, callback) {
    const set = listeners.get(event) ?? new Set();
    set.add(callback);
    listeners.set(event, set);
    return () => {
      set.delete(callback);
      if (set.size === 0) listeners.delete(event);
    };
  }

  return {
    call,
    on,
    context: () => call("context.get"),
    settings: {
      get: () => call("settings.get"),
      save: (values) => call("settings.save", { values }),
    },
    storage: {
      get: (key) => call("storage.get", { key }),
      set: (key, value) => call("storage.set", { key, value: value === undefined ? null : value }),
    },
    instances: {
      list: () => call("instances.list"),
    },
    history: {
      list: (options = {}) => call("history.list", options),
    },
    ui: {
      toast: (options) => call("ui.toast", options),
      openWindow: (windowId, params) => call("ui.openWindow", params === undefined ? { windowId } : { windowId, params }),
      navigate: (pageId, params) => call("ui.navigate", params === undefined ? { pageId } : { pageId, params }),
      openSettings: () => call("ui.openSettings"),
    },
    /** Rechaza las peticiones pendientes y deja de escuchar (útil en pruebas). */
    dispose: () => {
      unsubscribe();
      for (const [id, entry] of pending) {
        clearTimer(entry.timer);
        entry.reject(new ObdError("bridge_disposed", "client disposed"));
        pending.delete(id);
      }
      listeners.clear();
    },
  };
}

function createWindowClient() {
  const parentWindow = window.parent;
  const client = createObdClient({
    send: (message) => {
      if (!parentWindow || parentWindow === window) throw new Error("the plugin page is not embedded in Odoo Backup Desktop");
      parentWindow.postMessage(message, "*");
    },
    subscribe: (handler) => {
      const listener = (event) => {
        // Solo se aceptan mensajes de la app que contiene al iframe.
        if (event.source !== parentWindow || parentWindow === window) return;
        handler(event.data);
      };
      window.addEventListener("message", listener);
      return () => window.removeEventListener("message", listener);
    },
  });
  client.on("theme", (data) => {
    if (isRecord(data) && (data.theme === "light" || data.theme === "dark")) {
      document.documentElement.dataset.theme = data.theme;
    }
  });
  return client;
}

const obd = typeof window !== "undefined" && typeof document !== "undefined" ? createWindowClient() : null;

export default obd;
