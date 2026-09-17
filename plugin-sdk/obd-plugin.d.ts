// Tipos del SDK de plugins de Odoo Backup Desktop (API v1). Ver docs/plugins.md.

export declare const PROTOCOL_VERSION: 1;
export declare const DEFAULT_TIMEOUT_MS: number;

export type JsonValue = string | number | boolean | null | JsonValue[] | { [key: string]: JsonValue };

export type Theme = "light" | "dark";
export type Surface = "page" | "window";

export interface ObdContext {
  pluginId: string;
  pluginVersion: string | null;
  appVersion: string;
  theme: Theme;
  surface: Surface;
  /** Parámetros de navegación, p. ej. `{ instanceId }` desde «Más acciones». */
  params: Record<string, unknown>;
}

export interface ObdSettings {
  /** Valores no secretos. */
  values: Record<string, JsonValue>;
  /** Claves secretas que tienen valor guardado (los valores nunca llegan al plugin). */
  secretsSet: string[];
}

export type HistoryStatus = "running" | "success" | "failed" | "cancelled";

export interface ObdHistoryEntry {
  id: string;
  instanceId: string;
  instanceName: string;
  status: HistoryStatus;
  transport: "db_manager" | "obd_module" | null;
  startedAt: string;
  finishedAt: string | null;
  filePath: string | null;
  sizeBytes: number | null;
  sha256: string | null;
  odooVersion: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  drive: { status: "skipped" | "success" | "failed"; fileId: string | null; errorMessage: string | null };
}

export interface ObdInstance {
  id: string;
  name: string;
  url: string;
  database: string;
  /** Versión detectada en la última prueba de conexión (p. ej. "17.0"). */
  odooVersion: string | null;
  lastBackup: ObdHistoryEntry | null;
}

export type ToastKind = "info" | "success" | "error";

export interface ToastOptions {
  kind?: ToastKind;
  title: string;
  description?: string;
}

export interface BridgeEventMap {
  theme: { theme: Theme };
  "plugins-changed": Record<string, never>;
}

export declare class ObdError extends Error {
  readonly code: string;
  constructor(code: string, message?: string);
}

export interface ObdClient {
  /** Llamada genérica al puente. */
  call<T = unknown>(method: string, params?: Record<string, unknown>): Promise<T>;
  on<E extends keyof BridgeEventMap>(event: E, callback: (data: BridgeEventMap[E]) => void): () => void;
  on(event: string, callback: (data: unknown) => void): () => void;
  context(): Promise<ObdContext>;
  settings: {
    get(): Promise<ObdSettings>;
    /** Secretos: string = reemplazar, `null` = borrar, ausente = conservar. */
    save(values: Record<string, JsonValue>): Promise<ObdSettings>;
  };
  storage: {
    get<T extends JsonValue = JsonValue>(key: string): Promise<T | null>;
    /** `null` (o `undefined`) borra la clave. Límite: 1 MiB por plugin. */
    set(key: string, value: JsonValue | undefined): Promise<null>;
  };
  instances: {
    list(): Promise<ObdInstance[]>;
  };
  history: {
    list(options?: { instanceId?: string; limit?: number }): Promise<ObdHistoryEntry[]>;
  };
  ui: {
    toast(options: ToastOptions): Promise<null>;
    openWindow(windowId: string, params?: Record<string, JsonValue>): Promise<null>;
    /** Solo desde páginas (en ventanas falla con `unsupported_surface`). */
    navigate(pageId: string, params?: Record<string, JsonValue>): Promise<null>;
    openSettings(): Promise<null>;
  };
  dispose(): void;
}

export interface ObdTransport {
  send(message: unknown): void;
  subscribe(handler: (data: unknown) => void): () => void;
  timeoutMs?: number;
  setTimer?: (callback: () => void, ms: number) => unknown;
  clearTimer?: (timer: unknown) => void;
}

export declare function createObdClient(transport: ObdTransport): ObdClient;
export declare function isBridgeResponse(data: unknown): boolean;
export declare function isBridgeEvent(data: unknown): boolean;

/** Cliente conectado a la app (null fuera de un navegador). */
declare const obd: ObdClient;
export default obd;
