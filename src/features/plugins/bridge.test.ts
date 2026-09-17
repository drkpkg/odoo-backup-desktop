import { describe, expect, it, vi } from "vitest";

import { AppError } from "../../lib/errors";
import type { InstanceView, PluginSettings, PluginView } from "../../lib/types";
import { handleBridgeMessage, parseBridgeEnvelope, pluginAssetUrl, toPluginInstance, type BridgeHost } from "./bridge";

function plugin(overrides: Partial<PluginView> = {}): PluginView {
  return {
    id: "hello-obd",
    name: "Hola OBD",
    version: "0.1.0",
    description: null,
    author: null,
    homepage: null,
    source: "dev",
    path: "/plugins/hello-obd",
    status: "enabled",
    issues: [],
    baseUrl: "obd-plugin://localhost/hello-obd/",
    revision: 3,
    pages: [{ id: "main", title: "Hola", path: "ui/index.html" }],
    menus: [],
    windows: [{ id: "detail", title: "Detalle", path: "ui/detail.html", width: null, height: null }],
    hasSettings: true,
    destinations: [],
    hooks: [],
    permissions: { network: [] },
    hasBackend: false,
    ...overrides,
  };
}

const settings: PluginSettings = {
  schema: { type: "object", properties: {}, propertyOrder: [], required: [] },
  values: { endpoint: "https://api.example.com" },
  secretsSet: ["token"],
};

const instance: InstanceView = {
  id: "i1",
  name: "Andina",
  url: "https://andina.example.com/",
  database: "andina",
  login: "admin",
  secretKind: "api_key",
  hasSecret: true,
  hasMasterPassword: false,
  transport: "auto",
  protocol: "auto",
  includeFilestore: true,
  uploadToDrive: false,
  lastProbe: {
    baseUrl: "https://andina.example.com/",
    https: true,
    version: { major: 17, minor: 0, serverVersion: "17.0-2026", saas: false },
    supported: true,
    database: "andina",
    protocol: "xml_rpc",
    uid: 2,
    auth: { status: "ok" },
    module: { status: "skipped", reason: "x" },
    moduleApiVersion: null,
    dbManager: { status: "ok" },
    recommendedTransport: "db_manager",
    warnings: [],
    checkedAt: "2026-09-16T10:00:00Z",
  },
  lastBackup: null,
  createdAt: "2026-09-01T00:00:00Z",
  updatedAt: "2026-09-01T00:00:00Z",
};

function makeHost(overrides: Partial<BridgeHost> = {}) {
  const backend = {
    getPluginSettings: vi.fn(async () => settings),
    savePluginSettings: vi.fn(async () => settings),
    pluginStorageGet: vi.fn(async () => 42 as unknown),
    pluginStorageSet: vi.fn(async () => undefined),
    listInstances: vi.fn(async () => [instance]),
    listHistory: vi.fn(async () => []),
    openPluginWindow: vi.fn(async () => undefined),
  };
  const host: BridgeHost = {
    plugin: plugin(),
    appVersion: "0.3.0",
    surface: "page",
    params: { instanceId: "i1" },
    theme: () => "dark",
    backend,
    toast: vi.fn(),
    navigate: vi.fn(),
    openSettings: vi.fn(),
    ...overrides,
  };
  return { host, backend };
}

const req = (method: string, params?: unknown, id = "1") => ({ obd: 1, id, method, params });

describe("parseBridgeEnvelope", () => {
  it("ignores foreign messages, responses and events", () => {
    expect(parseBridgeEnvelope(null)).toBeNull();
    expect(parseBridgeEnvelope("hello")).toBeNull();
    expect(parseBridgeEnvelope({ obd: 2, id: "1", method: "context.get" })).toBeNull();
    expect(parseBridgeEnvelope({ obd: 1, method: "context.get" })).toBeNull();
    expect(parseBridgeEnvelope({ obd: 1, id: "", method: "context.get" })).toBeNull();
    expect(parseBridgeEnvelope({ obd: 1, id: "x".repeat(65), method: "context.get" })).toBeNull();
    expect(parseBridgeEnvelope({ obd: 1, id: "1", result: 1 })).toBeNull();
    expect(parseBridgeEnvelope({ obd: 1, event: "theme", data: {} })).toBeNull();
    expect(parseBridgeEnvelope(req("context.get"))).toEqual({ id: "1", method: "context.get", params: undefined });
  });
});

describe("handleBridgeMessage", () => {
  it("returns null for messages outside the protocol", async () => {
    const { host } = makeHost();
    expect(await handleBridgeMessage({ type: "webpack" }, host)).toBeNull();
  });

  it("answers context.get with host data", async () => {
    const { host } = makeHost();
    expect(await handleBridgeMessage(req("context.get"), host)).toEqual({
      obd: 1,
      id: "1",
      result: {
        pluginId: "hello-obd",
        pluginVersion: "0.1.0",
        appVersion: "0.3.0",
        theme: "dark",
        surface: "page",
        params: { instanceId: "i1" },
      },
    });
  });

  it("never takes the pluginId from the message", async () => {
    const { host, backend } = makeHost();
    await handleBridgeMessage({ ...req("settings.get", { pluginId: "other-plugin" }), pluginId: "other-plugin" }, host);
    await handleBridgeMessage(req("storage.set", { key: "k", value: 1, pluginId: "other-plugin" }), host);
    expect(backend.getPluginSettings).toHaveBeenCalledWith("hello-obd");
    expect(backend.pluginStorageSet).toHaveBeenCalledWith("hello-obd", "k", 1);
  });

  it("strips the schema from settings responses", async () => {
    const { host, backend } = makeHost();
    const get = await handleBridgeMessage(req("settings.get"), host);
    expect(get).toEqual({ obd: 1, id: "1", result: { values: settings.values, secretsSet: ["token"] } });

    const save = await handleBridgeMessage(req("settings.save", { values: { token: null } }), host);
    expect(backend.savePluginSettings).toHaveBeenCalledWith("hello-obd", { token: null });
    expect(save).toMatchObject({ result: { secretsSet: ["token"] } });

    expect(await handleBridgeMessage(req("settings.save", { values: [1] }), host)).toMatchObject({
      error: { code: "bridge_invalid_request" },
    });
  });

  it("validates storage keys and values", async () => {
    const { host, backend } = makeHost();
    expect(await handleBridgeMessage(req("storage.get", { key: "counter" }), host)).toEqual({ obd: 1, id: "1", result: 42 });
    expect(await handleBridgeMessage(req("storage.get", { key: "bad key!" }), host)).toMatchObject({
      error: { code: "plugin_storage_key_invalid" },
    });
    expect(await handleBridgeMessage(req("storage.get", {}), host)).toMatchObject({ error: { code: "bridge_invalid_request" } });
    expect(await handleBridgeMessage(req("storage.set", { key: "counter" }), host)).toMatchObject({
      error: { code: "bridge_invalid_request" },
    });
    expect(await handleBridgeMessage(req("storage.set", { key: "counter", value: null }), host)).toEqual({ obd: 1, id: "1", result: null });
    expect(backend.pluginStorageSet).toHaveBeenCalledWith("hello-obd", "counter", null);
  });

  it("lists instances without secrets and history with validated args", async () => {
    const { host, backend } = makeHost();
    const response = await handleBridgeMessage(req("instances.list"), host);
    expect(response).toEqual({
      obd: 1,
      id: "1",
      result: [{ id: "i1", name: "Andina", url: "https://andina.example.com/", database: "andina", odooVersion: "17.0", lastBackup: null }],
    });
    expect(JSON.stringify(response)).not.toContain("hasSecret");

    await handleBridgeMessage(req("history.list", { instanceId: "i1", limit: 5 }), host);
    expect(backend.listHistory).toHaveBeenCalledWith({ instanceId: "i1", limit: 5 });
    expect(await handleBridgeMessage(req("history.list", { limit: 0 }), host)).toMatchObject({ error: { code: "bridge_invalid_request" } });
    expect(await handleBridgeMessage(req("history.list", { limit: 2.5 }), host)).toMatchObject({ error: { code: "bridge_invalid_request" } });
  });

  it("shows toasts with validated kinds", async () => {
    const { host } = makeHost();
    await handleBridgeMessage(req("ui.toast", { kind: "success", title: "Listo", description: "ok" }), host);
    expect(host.toast).toHaveBeenCalledWith("success", "Listo", "ok");
    await handleBridgeMessage(req("ui.toast", { title: "Sin tipo" }), host);
    expect(host.toast).toHaveBeenCalledWith("info", "Sin tipo", undefined);
    expect(await handleBridgeMessage(req("ui.toast", { kind: "warning", title: "x" }), host)).toMatchObject({
      error: { code: "bridge_invalid_request" },
    });
    expect(await handleBridgeMessage(req("ui.toast", { kind: "info" }), host)).toMatchObject({ error: { code: "bridge_invalid_request" } });
  });

  it("opens declared windows only", async () => {
    const { host, backend } = makeHost();
    await handleBridgeMessage(req("ui.openWindow", { windowId: "detail", params: { instanceId: "i1" } }), host);
    expect(backend.openPluginWindow).toHaveBeenCalledWith("hello-obd", "detail", { instanceId: "i1" });
    expect(await handleBridgeMessage(req("ui.openWindow", { windowId: "nope" }), host)).toMatchObject({
      error: { code: "plugin_window_not_found" },
    });
    expect(await handleBridgeMessage(req("ui.openWindow", { windowId: "detail", params: "x" }), host)).toMatchObject({
      error: { code: "bridge_invalid_request" },
    });
  });

  it("navigates only from pages to declared pages", async () => {
    const { host } = makeHost();
    await handleBridgeMessage(req("ui.navigate", { pageId: "main", params: { a: 1 } }), host);
    expect(host.navigate).toHaveBeenCalledWith("main", { a: 1 });
    expect(await handleBridgeMessage(req("ui.navigate", { pageId: "missing" }), host)).toMatchObject({
      error: { code: "bridge_invalid_request" },
    });

    const { host: windowHost } = makeHost({ surface: "window", navigate: undefined });
    expect(await handleBridgeMessage(req("ui.navigate", { pageId: "main" }), windowHost)).toMatchObject({
      error: { code: "unsupported_surface" },
    });
  });

  it("opens settings only when the plugin declares them", async () => {
    const { host } = makeHost();
    expect(await handleBridgeMessage(req("ui.openSettings"), host)).toEqual({ obd: 1, id: "1", result: null });
    expect(host.openSettings).toHaveBeenCalled();
    const { host: noSettings } = makeHost({ plugin: plugin({ hasSettings: false }) });
    expect(await handleBridgeMessage(req("ui.openSettings"), noSettings)).toMatchObject({ error: { code: "plugin_no_settings" } });
  });

  it("reports unknown methods, invalid params and backend errors", async () => {
    const { host, backend } = makeHost();
    expect(await handleBridgeMessage(req("fs.readFile"), host)).toMatchObject({ error: { code: "bridge_unknown_method" } });
    expect(await handleBridgeMessage({ obd: 1, id: "9", method: 5 }, host)).toEqual({
      obd: 1,
      id: "9",
      error: { code: "bridge_invalid_request", message: "method must be a string" },
    });
    expect(await handleBridgeMessage(req("storage.get", "key"), host)).toMatchObject({ error: { code: "bridge_invalid_request" } });

    backend.listInstances.mockRejectedValueOnce(new AppError("vault_locked", "vault is locked"));
    expect(await handleBridgeMessage(req("instances.list"), host)).toEqual({
      obd: 1,
      id: "1",
      error: { code: "vault_locked", message: "vault is locked" },
    });
    backend.pluginStorageSet.mockRejectedValueOnce({ code: "plugin_storage_limit", message: "limit" });
    expect(await handleBridgeMessage(req("storage.set", { key: "k", value: "x" }), host)).toMatchObject({
      error: { code: "plugin_storage_limit" },
    });
  });
});

describe("helpers", () => {
  it("formats saas versions for plugins", () => {
    const saas = { ...instance, lastProbe: instance.lastProbe && { ...instance.lastProbe, version: { major: 17, minor: 2, serverVersion: "saas~17.2", saas: true } } };
    expect(toPluginInstance(saas).odooVersion).toBe("saas~17.2");
    expect(toPluginInstance({ ...instance, lastProbe: null }).odooVersion).toBeNull();
  });

  it("builds asset URLs with the revision", () => {
    expect(pluginAssetUrl({ baseUrl: "obd-plugin://localhost/hello-obd/", revision: 3 }, "ui/index.html")).toBe(
      "obd-plugin://localhost/hello-obd/ui/index.html?rev=3",
    );
    expect(pluginAssetUrl({ baseUrl: "http://obd-plugin.localhost/hello-obd", revision: 1 }, "/ui/a.html")).toBe(
      "http://obd-plugin.localhost/hello-obd/ui/a.html?rev=1",
    );
  });
});
