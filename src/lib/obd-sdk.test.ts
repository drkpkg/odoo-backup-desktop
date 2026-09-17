import { afterEach, describe, expect, it, vi } from "vitest";

import defaultClient, { createObdClient, isBridgeEvent, isBridgeResponse, ObdError } from "../../plugin-sdk/obd-plugin.js";

function fakeTransport() {
  const sent: { obd: number; id: string; method: string; params?: unknown }[] = [];
  let handler: ((data: unknown) => void) | null = null;
  const transport = {
    send: (message: unknown) => {
      sent.push(message as (typeof sent)[number]);
    },
    subscribe: (fn: (data: unknown) => void) => {
      handler = fn;
      return () => {
        handler = null;
      };
    },
  };
  return {
    transport,
    sent,
    deliver: (data: unknown) => handler?.(data),
    subscribed: () => handler !== null,
  };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("plugin SDK", () => {
  it("has no default client outside a browser", () => {
    expect(defaultClient).toBeNull();
  });

  it("recognizes protocol messages", () => {
    expect(isBridgeResponse({ obd: 1, id: "1", result: null })).toBe(true);
    expect(isBridgeResponse({ obd: 1, id: "1", error: { code: "x" } })).toBe(true);
    expect(isBridgeResponse({ obd: 1, method: "x" })).toBe(false);
    expect(isBridgeEvent({ obd: 1, event: "theme", data: {} })).toBe(true);
    expect(isBridgeEvent({ obd: 1, id: "1", event: "theme" })).toBe(false);
    expect(isBridgeEvent("theme")).toBe(false);
  });

  it("sends requests with unique ids and resolves responses", async () => {
    const t = fakeTransport();
    const client = createObdClient(t.transport);

    const context = client.context();
    const stored = client.storage.get("counter");
    expect(t.sent).toEqual([
      { obd: 1, id: "obd-1", method: "context.get" },
      { obd: 1, id: "obd-2", method: "storage.get", params: { key: "counter" } },
    ]);

    t.deliver({ obd: 1, id: "obd-2", result: 5 });
    t.deliver({ obd: 1, id: "obd-1", result: { pluginId: "hello-obd" } });
    await expect(stored).resolves.toBe(5);
    await expect(context).resolves.toEqual({ pluginId: "hello-obd" });

    // Respuestas desconocidas o repetidas se ignoran.
    t.deliver({ obd: 1, id: "obd-1", result: "again" });
    t.deliver({ obd: 1, id: "obd-99", result: 1 });
    client.dispose();
    expect(t.subscribed()).toBe(false);
  });

  it("maps helper calls to bridge methods", async () => {
    const t = fakeTransport();
    const client = createObdClient(t.transport);
    const calls = [
      client.settings.save({ token: null }),
      client.storage.set("k", undefined),
      client.history.list({ instanceId: "i1", limit: 5 }),
      client.ui.toast({ kind: "success", title: "Hola" }),
      client.ui.openWindow("detail", { instanceId: "i1" }),
      client.ui.navigate("main"),
      client.ui.openSettings(),
      client.instances.list(),
    ];
    // `dispose` rechaza las pendientes con `bridge_disposed`.
    const settled = Promise.allSettled(calls);
    expect(t.sent.map((m) => [m.method, m.params])).toEqual([
      ["settings.save", { values: { token: null } }],
      ["storage.set", { key: "k", value: null }],
      ["history.list", { instanceId: "i1", limit: 5 }],
      ["ui.toast", { kind: "success", title: "Hola" }],
      ["ui.openWindow", { windowId: "detail", params: { instanceId: "i1" } }],
      ["ui.navigate", { pageId: "main" }],
      ["ui.openSettings", undefined],
      ["instances.list", undefined],
    ]);
    client.dispose();
    const results = await settled;
    expect(results.every((r) => r.status === "rejected" && (r.reason as { code: string }).code === "bridge_disposed")).toBe(true);
  });

  it("rejects with ObdError on bridge errors", async () => {
    const t = fakeTransport();
    const client = createObdClient(t.transport);
    const pending = client.ui.navigate("main");
    t.deliver({ obd: 1, id: "obd-1", error: { code: "unsupported_surface", message: "pages only" } });
    const error = await pending.catch((e: unknown) => e);
    expect(error).toBeInstanceOf(ObdError);
    expect(error).toMatchObject({ code: "unsupported_surface", message: "pages only" });
    client.dispose();
  });

  it("times out after the configured delay", async () => {
    vi.useFakeTimers();
    const t = fakeTransport();
    const client = createObdClient({ ...t.transport, timeoutMs: 1000 });
    const pending = client.instances.list().catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(999);
    t.deliver({ obd: 1, id: "other", result: null });
    await vi.advanceTimersByTimeAsync(1);
    await expect(pending).resolves.toMatchObject({ code: "bridge_timeout" });
    // Una respuesta tardía ya no hace nada.
    t.deliver({ obd: 1, id: "obd-1", result: [] });
    client.dispose();
  });

  it("uses the default 30 s timeout", async () => {
    vi.useFakeTimers();
    const t = fakeTransport();
    const client = createObdClient(t.transport);
    const pending = client.context().catch((e: unknown) => e);
    await vi.advanceTimersByTimeAsync(29_999);
    let settled = false;
    void pending.then(() => {
      settled = true;
    });
    await Promise.resolve();
    expect(settled).toBe(false);
    await vi.advanceTimersByTimeAsync(1);
    await expect(pending).resolves.toMatchObject({ code: "bridge_timeout" });
    client.dispose();
  });

  it("dispatches events and supports unsubscribing", () => {
    const t = fakeTransport();
    const client = createObdClient(t.transport);
    const received: unknown[] = [];
    const off = client.on("theme", (data) => received.push(data));
    t.deliver({ obd: 1, event: "theme", data: { theme: "dark" } });
    off();
    t.deliver({ obd: 1, event: "theme", data: { theme: "light" } });
    expect(received).toEqual([{ theme: "dark" }]);
    client.dispose();
  });

  it("rejects immediately when the transport cannot send", async () => {
    const client = createObdClient({
      send: () => {
        throw new Error("not embedded");
      },
      subscribe: () => () => undefined,
    });
    await expect(client.context()).rejects.toMatchObject({ code: "bridge_unavailable" });
  });
});
