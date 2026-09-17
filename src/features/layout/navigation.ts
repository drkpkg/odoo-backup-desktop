// Modelo de navegación de la ventana principal: páginas propias + páginas de plugins.

import type { PluginMenu, PluginView } from "../../lib/types";

export type CorePage = "instances" | "history" | "plugins" | "settings";

export type Route =
  | { kind: "core"; page: CorePage }
  | { kind: "plugin"; pluginId: string; pageId: string; params: Record<string, unknown> };

export const coreRoute = (page: CorePage): Route => ({ kind: "core", page });

export function pluginRoute(pluginId: string, pageId: string, params: Record<string, unknown> = {}): Route {
  return { kind: "plugin", pluginId, pageId, params };
}

export function sameRoute(a: Route, b: Route): boolean {
  if (a.kind === "core" && b.kind === "core") return a.page === b.page;
  if (a.kind === "plugin" && b.kind === "plugin") return a.pluginId === b.pluginId && a.pageId === b.pageId;
  return false;
}

export type PluginMenuEntry = { plugin: PluginView; menu: PluginMenu };

/** Menús de plugins activos para una ubicación, en orden de plugin y declaración. */
export function pluginMenus(plugins: PluginView[] | undefined, location: PluginMenu["location"]): PluginMenuEntry[] {
  if (!plugins) return [];
  return plugins
    .filter((plugin) => plugin.status === "enabled")
    .flatMap((plugin) =>
      plugin.menus
        .filter((menu) => menu.location === location)
        .filter((menu) =>
          menu.page ? plugin.pages.some((page) => page.id === menu.page) : plugin.windows.some((w) => w.id === menu.window),
        )
        .map((menu) => ({ plugin, menu })),
    );
}

/** Prefijo de las etiquetas de ventanas de plugin (`plugin--<id>--<windowId>`). */
export const PLUGIN_WINDOW_LABEL_PREFIX = "plugin--";

export function isPluginWindowLabel(label: string | null | undefined): boolean {
  return typeof label === "string" && label.startsWith(PLUGIN_WINDOW_LABEL_PREFIX) && label.length > PLUGIN_WINDOW_LABEL_PREFIX.length;
}
