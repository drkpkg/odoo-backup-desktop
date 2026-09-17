import { describe, expect, it } from "vitest";

import type { PluginView } from "../../lib/types";
import { FALLBACK_PLUGIN_ICON, pluginIcon, PLUGIN_ICONS, resolvePluginIconName } from "../plugins/icons";
import { coreRoute, isPluginWindowLabel, pluginMenus, pluginRoute, sameRoute } from "./navigation";

function plugin(id: string, overrides: Partial<PluginView> = {}): PluginView {
  return {
    id,
    name: id,
    version: "1.0.0",
    description: null,
    author: null,
    homepage: null,
    source: "user",
    path: `/plugins/${id}`,
    status: "enabled",
    issues: [],
    baseUrl: `obd-plugin://localhost/${id}/`,
    revision: 1,
    pages: [{ id: "main", title: "Main", path: "ui/index.html" }],
    menus: [
      { id: "side", location: "sidebar", label: "Side", icon: "cloud", page: "main", window: null },
      { id: "act", location: "instance_actions", label: "Act", icon: null, page: null, window: "win" },
      { id: "broken", location: "sidebar", label: "Broken", icon: null, page: "missing", window: null },
    ],
    windows: [{ id: "win", title: "Win", path: "ui/win.html", width: null, height: null }],
    hasSettings: false,
    destinations: [],
    hooks: [],
    permissions: { network: [] },
    hasBackend: false,
    ...overrides,
  };
}

describe("routes", () => {
  it("compares core and plugin routes", () => {
    expect(sameRoute(coreRoute("plugins"), coreRoute("plugins"))).toBe(true);
    expect(sameRoute(coreRoute("plugins"), coreRoute("settings"))).toBe(false);
    expect(sameRoute(pluginRoute("a", "main", { x: 1 }), pluginRoute("a", "main"))).toBe(true);
    expect(sameRoute(pluginRoute("a", "main"), pluginRoute("b", "main"))).toBe(false);
    expect(sameRoute(coreRoute("plugins"), pluginRoute("a", "main"))).toBe(false);
  });
});

describe("pluginMenus", () => {
  it("returns menus of enabled plugins whose targets exist", () => {
    const list = [plugin("a"), plugin("b", { status: "disabled" }), plugin("c", { status: "error" })];
    expect(pluginMenus(list, "sidebar").map((e) => `${e.plugin.id}:${e.menu.id}`)).toEqual(["a:side"]);
    expect(pluginMenus(list, "instance_actions").map((e) => `${e.plugin.id}:${e.menu.id}`)).toEqual(["a:act"]);
    expect(pluginMenus(undefined, "sidebar")).toEqual([]);
  });
});

describe("plugin window labels", () => {
  it("detects plugin windows", () => {
    expect(isPluginWindowLabel("plugin--hello-obd--detail")).toBe(true);
    expect(isPluginWindowLabel("main")).toBe(false);
    expect(isPluginWindowLabel("plugin--")).toBe(false);
    expect(isPluginWindowLabel("plugin-hello")).toBe(false);
    expect(isPluginWindowLabel(null)).toBe(false);
  });
});

describe("plugin icons", () => {
  it("falls back to puzzle for unknown or missing names", () => {
    expect(resolvePluginIconName("cloud")).toBe("cloud");
    expect(resolvePluginIconName("does-not-exist")).toBe(FALLBACK_PLUGIN_ICON);
    expect(resolvePluginIconName(null)).toBe(FALLBACK_PLUGIN_ICON);
    expect(resolvePluginIconName("constructor")).toBe(FALLBACK_PLUGIN_ICON);
    expect(resolvePluginIconName("__proto__")).toBe(FALLBACK_PLUGIN_ICON);
    expect(pluginIcon("nope")).toBe(PLUGIN_ICONS.puzzle);
    expect(Object.keys(PLUGIN_ICONS).length).toBeGreaterThanOrEqual(40);
  });
});
