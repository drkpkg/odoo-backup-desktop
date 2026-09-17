import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect } from "react";

import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { InstanceView } from "../../lib/types";
import { pluginRoute, type PluginMenuEntry, type Route } from "../layout/navigation";

export function usePlugins() {
  return useQuery({ queryKey: queryKeys.plugins, queryFn: () => ipc.listPlugins(), staleTime: Infinity });
}

/** Mantiene la caché de plugins al día con el evento `plugins-changed`. */
export function usePluginsChangedListener() {
  const queryClient = useQueryClient();
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onPluginsChanged(() => {
        void queryClient.invalidateQueries({ queryKey: queryKeys.plugins });
        void queryClient.invalidateQueries({ queryKey: queryKeys.pluginConfig });
        void queryClient.invalidateQueries({ queryKey: queryKeys.pluginSettingsAll });
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);
}

/** Abre el destino de un menú de plugin: página (navegación) o ventana (comando). */
export function useOpenPluginMenu(navigate: (route: Route) => void) {
  const toast = useToast();
  return useCallback(
    async (entry: PluginMenuEntry, instance?: InstanceView) => {
      const params: Record<string, unknown> = instance ? { instanceId: instance.id } : {};
      const { plugin, menu } = entry;
      if (menu.page) {
        navigate(pluginRoute(plugin.id, menu.page, params));
        return;
      }
      if (menu.window) {
        try {
          await ipc.openPluginWindow(plugin.id, menu.window, params);
        } catch (err) {
          toast.error(`No se pudo abrir la ventana de ${plugin.name}`, errorMessage(err));
        }
      }
    },
    [navigate, toast],
  );
}
