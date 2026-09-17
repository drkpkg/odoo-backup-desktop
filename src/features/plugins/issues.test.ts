import { describe, expect, it } from "vitest";

import type { PluginIssue, PluginView } from "../../lib/types";
import { issueText, needsBackend, pluginHealth } from "./issues";

function plugin(patch: Partial<PluginView> = {}): PluginView {
  return {
    id: "hello",
    name: "Hola",
    version: "1.0.0",
    description: null,
    author: null,
    homepage: null,
    source: "user",
    path: "/plugins/hello",
    status: "enabled",
    issues: [],
    baseUrl: "obd-plugin://localhost/hello/",
    revision: 1,
    pages: [],
    menus: [],
    windows: [],
    hasSettings: false,
    destinations: [],
    hooks: [],
    permissions: { network: [] },
    hasBackend: false,
    ...patch,
  };
}

const issue = (severity: PluginIssue["severity"], code: string, message = "technical"): PluginIssue => ({ severity, code, message, field: null });

describe("plugin issues", () => {
  it("translates known codes and falls back to the technical message", () => {
    expect(issueText(issue("error", "manifest_missing"))).toBe("Falta plugin.json en la carpeta del plugin.");
    expect(issueText(issue("error", "something_new", "raw detail"))).toBe("raw detail");
  });

  it("shows the first error as the visible cause", () => {
    const health = pluginHealth(
      plugin({ status: "error", issues: [issue("error", "manifest_invalid"), issue("error", "invalid_icon"), issue("warning", "invalid_label")] }),
    );
    expect(health).toMatchObject({ tone: "danger", title: "No se pudo cargar" });
    expect(health?.detail).toContain("El plugin.json no es válido.");
    expect(health?.detail).toContain("Hay 1 problema más");
    expect(health?.detail).toContain("Recargar");
  });

  it("explains shadowed plugins and warnings", () => {
    expect(pluginHealth(plugin({ status: "shadowed" }))?.title).toBe("Reemplazado");
    expect(pluginHealth(plugin({ issues: [issue("warning", "invalid_network_host")] }))).toMatchObject({ tone: "warning", title: "Funciona con avisos" });
  });

  it("does not treat the phase B backend notice as a problem", () => {
    const s3 = plugin({ hasBackend: true, hooks: ["after_backup"], issues: [issue("warning", "backend_not_supported")] });
    expect(pluginHealth(s3)).toBeNull();
    expect(needsBackend(s3)).toBe(true);
    expect(needsBackend(plugin())).toBe(false);
  });
});
