import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useState } from "react";

import { Spinner } from "./components/Spinner";
import { useToast } from "./components/Toast";
import { AppLayout } from "./features/layout/AppLayout";
import { coreRoute, type Route } from "./features/layout/navigation";
import { BackupJobsProvider } from "./features/backups/BackupJobsProvider";
import { HistoryPage } from "./features/history/HistoryPage";
import { InstancesPage } from "./features/instances/InstancesPage";
import { MockPluginWindowDialog } from "./features/plugins/MockPluginWindowDialog";
import { PluginPageView } from "./features/plugins/PluginPageView";
import { PluginsPage } from "./features/plugins/PluginsPage";
import { usePluginsChangedListener } from "./features/plugins/usePlugins";
import { SettingsPage } from "./features/settings/SettingsPage";
import { VaultGate } from "./features/vault/VaultGate";
import { errorMessage } from "./lib/errors";
import { ipc } from "./lib/ipc";
import { queryKeys } from "./lib/query";
import type { AppStatus } from "./lib/types";

export default function App() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [route, setRoute] = useState<Route>(coreRoute("instances"));
  const [historyInstanceId, setHistoryInstanceId] = useState<string | null>(null);
  const [settingsPluginId, setSettingsPluginId] = useState<string | null>(null);

  const status = useQuery({ queryKey: queryKeys.appStatus, queryFn: () => ipc.getAppStatus(), staleTime: Infinity });
  usePluginsChangedListener();

  const navigate = useCallback((next: Route) => {
    if (!(next.kind === "core" && next.page === "history")) setHistoryInstanceId(null);
    if (!(next.kind === "core" && next.page === "settings")) setSettingsPluginId(null);
    setRoute(next);
  }, []);

  const openPluginSettings = useCallback((pluginId: string) => {
    setSettingsPluginId(pluginId);
    setRoute(coreRoute("settings"));
  }, []);

  // Bloqueo desde el backend (manual o por inactividad): descartar todos los datos en caché.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onVaultLocked((payload) => {
        queryClient.setQueryData<AppStatus>(queryKeys.appStatus, (prev) =>
          prev ? { ...prev, vault: { ...prev.vault, unlocked: false } } : prev,
        );
        queryClient.removeQueries({
          predicate: (query) => query.queryKey[0] !== queryKeys.appStatus[0] && query.queryKey[0] !== queryKeys.plugins[0],
        });
        void queryClient.invalidateQueries({ queryKey: queryKeys.appStatus });
        if (payload.reason === "idle") toast.info("Bóveda bloqueada por inactividad");
      })
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      });
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient, toast]);

  // Ventanas de plugin que piden abrir los ajustes de su plugin aquí.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onOpenPluginSettings(openPluginSettings)
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [openPluginSettings]);

  if (status.isPending) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner size={20} label="Cargando…" />
      </div>
    );
  }

  if (status.isError) {
    return (
      <div className="flex h-full flex-col items-center justify-center gap-2 p-6 text-center">
        <p className="font-semibold">No se pudo iniciar Odoo Backup Desktop</p>
        <p className="text-sm text-muted">{errorMessage(status.error)}</p>
      </div>
    );
  }

  return (
    <VaultGate status={status.data}>
      <BackupJobsProvider>
        <AppLayout route={route} onNavigate={navigate} status={status.data}>
          {route.kind === "core" && route.page === "instances" ? (
            <InstancesPage
              onNavigate={navigate}
              onShowHistory={(instanceId) => {
                setHistoryInstanceId(instanceId);
                setRoute(coreRoute("history"));
              }}
            />
          ) : null}
          {route.kind === "core" && route.page === "history" ? (
            <HistoryPage instanceId={historyInstanceId} onInstanceChange={setHistoryInstanceId} />
          ) : null}
          {route.kind === "core" && route.page === "plugins" ? (
            <PluginsPage onNavigate={navigate} onOpenSettings={openPluginSettings} />
          ) : null}
          {route.kind === "core" && route.page === "settings" ? (
            <SettingsPage status={status.data} focusPluginId={settingsPluginId} />
          ) : null}
          {route.kind === "plugin" ? (
            <PluginPageView
              key={`${route.pluginId}:${route.pageId}:${JSON.stringify(route.params)}`}
              pluginId={route.pluginId}
              pageId={route.pageId}
              params={route.params}
              appVersion={status.data.appVersion}
              onNavigate={navigate}
              onOpenSettings={openPluginSettings}
            />
          ) : null}
        </AppLayout>
        {ipc.kind === "mock" ? (
          <MockPluginWindowDialog appVersion={status.data.appVersion} onOpenSettings={openPluginSettings} />
        ) : null}
      </BackupJobsProvider>
    </VaultGate>
  );
}
