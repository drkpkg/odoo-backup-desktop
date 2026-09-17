import { useQuery, useQueryClient } from "@tanstack/react-query";
import { LockKeyhole, Puzzle, RefreshCw } from "lucide-react";
import { useEffect } from "react";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { PluginFrame } from "./PluginFrame";
import { usePlugins, usePluginsChangedListener } from "./usePlugins";

const WINDOW_CONTEXT_KEY = ["plugin-window-context"] as const;

/** Shell mínimo de una ventana de plugin (`plugin--<id>--<windowId>`): sin barra lateral. */
export function PluginWindowApp() {
  const queryClient = useQueryClient();
  const toast = useToast();
  usePluginsChangedListener();

  const status = useQuery({ queryKey: queryKeys.appStatus, queryFn: () => ipc.getAppStatus(), staleTime: Infinity });
  const context = useQuery({ queryKey: WINDOW_CONTEXT_KEY, queryFn: () => ipc.getPluginWindowContext(), staleTime: Infinity });
  const plugins = usePlugins();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onVaultLocked(() => {
        queryClient.setQueryData<AppStatus>(queryKeys.appStatus, (prev) =>
          prev ? { ...prev, vault: { ...prev.vault, unlocked: false } } : prev,
        );
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    // Al volver a la ventana se revisa si la bóveda ya se desbloqueó en la principal.
    const onFocus = () => void queryClient.invalidateQueries({ queryKey: queryKeys.appStatus });
    window.addEventListener("focus", onFocus);
    return () => {
      cancelled = true;
      unlisten?.();
      window.removeEventListener("focus", onFocus);
    };
  }, [queryClient]);

  const plugin = context.data ? plugins.data?.find((p) => p.id === context.data.pluginId) : undefined;
  const pluginWindow = context.data ? plugin?.windows.find((w) => w.id === context.data.windowId) : undefined;

  useEffect(() => {
    if (plugin && pluginWindow) document.title = `${pluginWindow.title} · ${plugin.name}`;
  }, [plugin, pluginWindow]);

  if (status.isPending || context.isPending || plugins.isPending) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={20} label="Cargando…" />
      </div>
    );
  }

  const failure = status.error ?? context.error ?? plugins.error;
  if (failure) {
    return (
      <div className="p-6">
        <Alert tone="danger" title="No se pudo abrir la ventana del plugin">
          {errorMessage(failure)}
        </Alert>
      </div>
    );
  }

  if (!status.data?.vault.unlocked) {
    return (
      <EmptyState
        icon={<LockKeyhole size={22} />}
        title="La bóveda está bloqueada"
        description="Desbloquea la bóveda en la ventana principal para usar este plugin."
        action={
          <Button icon={<RefreshCw size={14} />} onClick={() => void status.refetch()}>
            Reintentar
          </Button>
        }
      />
    );
  }

  if (!context.data || !plugin || plugin.status !== "enabled" || !pluginWindow) {
    return (
      <EmptyState
        icon={<Puzzle size={22} />}
        title="Ventana no disponible"
        description="El plugin fue desactivado, eliminado o ya no declara esta ventana. Puedes cerrarla."
      />
    );
  }

  return (
    <div className="h-full">
      <PluginFrame
        plugin={plugin}
        path={pluginWindow.path}
        title={`${pluginWindow.title} · ${plugin.name}`}
        surface="window"
        params={context.data.params ?? {}}
        appVersion={status.data.appVersion}
        onOpenSettings={async () => {
          try {
            await ipc.requestOpenPluginSettings(plugin.id);
            toast.info("Ajustes abiertos en la ventana principal");
          } catch (err) {
            toast.error("No se pudieron abrir los ajustes", errorMessage(err));
          }
        }}
      />
    </div>
  );
}
