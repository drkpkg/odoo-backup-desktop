import type { PluginIssue, PluginView } from "../../lib/types";

/** Aviso de fase B: no es un error del plugin, se muestra como capacidad no disponible. */
export const BACKEND_ISSUE = "backend_not_supported";

const ISSUE_TEXT: Record<string, string> = {
  manifest_missing: "Falta plugin.json en la carpeta del plugin.",
  manifest_unreadable: "No se pudo leer la carpeta o su plugin.json.",
  manifest_invalid: "El plugin.json no es válido.",
  unsupported_api_version: "El plugin usa una versión de API que esta app no admite.",
  invalid_id: "El id del plugin no es válido.",
  invalid_name: "El nombre del plugin no es válido.",
  invalid_version: "La versión no es válida (usa semver, p. ej. 1.0.0).",
  invalid_label: "Una etiqueta de página, menú o ventana no es válida.",
  invalid_contribution_id: "Un id de página, menú o ventana no es válido.",
  duplicate_contribution_id: "Hay ids de páginas, menús o ventanas repetidos.",
  invalid_icon: "Un menú usa un icono que no existe.",
  invalid_menu_target: "Un menú debe abrir una página o una ventana.",
  unknown_page: "Un menú apunta a una página que no existe.",
  unknown_window: "Un menú apunta a una ventana que no existe.",
  invalid_network_host: "Un host de red declarado no es válido.",
  invalid_path: "Una ruta de página o ventana no es válida.",
  path_not_found: "No existe el archivo de una página o ventana.",
  path_outside_plugin: "Una ruta sale de la carpeta del plugin.",
  invalid_window_size: "El tamaño de una ventana no es válido.",
  settings_schema_invalid: "El esquema de ajustes no es válido.",
  [BACKEND_ISSUE]: "Los destinos, hooks y backends WebAssembly no están disponibles en esta versión.",
};

/** Texto en español para un problema; los desconocidos usan el mensaje técnico. */
export function issueText(issue: PluginIssue): string {
  return ISSUE_TEXT[issue.code] ?? issue.message;
}

export type PluginHealth = { tone: "danger" | "warning"; title: string; detail: string };

/** La causa visible de un plugin que no funciona del todo; null si está bien. */
export function pluginHealth(plugin: PluginView): PluginHealth | null {
  const errors = plugin.issues.filter((issue) => issue.severity === "error");
  const warnings = plugin.issues.filter((issue) => issue.severity === "warning" && issue.code !== BACKEND_ISSUE);
  const more = (count: number) => (count > 1 ? ` Hay ${count - 1} ${count === 2 ? "problema más" : "problemas más"} en los detalles técnicos.` : "");

  if (plugin.status === "error") {
    const [first] = errors;
    return {
      tone: "danger",
      title: "No se pudo cargar",
      detail: `${first ? issueText(first) : "El plugin tiene errores."}${more(errors.length)} Corrígelo y pulsa «Recargar».`,
    };
  }
  if (plugin.status === "shadowed") {
    return { tone: "warning", title: "Reemplazado", detail: "Otro plugin con el mismo id tiene prioridad. Quita uno de los dos para usar este." };
  }
  const [warning] = warnings;
  if (warning) return { tone: "warning", title: "Funciona con avisos", detail: `${issueText(warning)}${more(warnings.length)}` };
  return null;
}

/** El plugin declara destinos, hooks o un backend (fase B, aún no disponibles). */
export function needsBackend(plugin: PluginView): boolean {
  return plugin.hasBackend || plugin.destinations.length > 0 || plugin.hooks.length > 0;
}
