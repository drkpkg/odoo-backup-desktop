import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";

import { Spinner } from "./components/Spinner";
import { useToast } from "./components/Toast";
import { AppLayout, type Page } from "./features/layout/AppLayout";
import { BackupJobsProvider } from "./features/backups/BackupJobsProvider";
import { HistoryPage } from "./features/history/HistoryPage";
import { InstancesPage } from "./features/instances/InstancesPage";
import { SettingsPage } from "./features/settings/SettingsPage";
import { VaultGate } from "./features/vault/VaultGate";
import { errorMessage } from "./lib/errors";
import { ipc } from "./lib/ipc";
import { queryKeys } from "./lib/query";
import type { AppStatus } from "./lib/types";

export default function App() {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [page, setPage] = useState<Page>("instances");
  const [historyInstanceId, setHistoryInstanceId] = useState<string | null>(null);

  const status = useQuery({ queryKey: queryKeys.appStatus, queryFn: () => ipc.getAppStatus(), staleTime: Infinity });

  // Bloqueo desde el backend (manual o por inactividad): descartar todos los datos en caché.
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onVaultLocked((payload) => {
        queryClient.setQueryData<AppStatus>(queryKeys.appStatus, (prev) =>
          prev ? { ...prev, vault: { ...prev.vault, unlocked: false } } : prev,
        );
        queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== queryKeys.appStatus[0] });
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
        <p className="font-semibold">No se pudo iniciar Appex Backup</p>
        <p className="text-sm text-muted">{errorMessage(status.error)}</p>
      </div>
    );
  }

  return (
    <VaultGate status={status.data}>
      <BackupJobsProvider>
        <AppLayout
          page={page}
          onNavigate={(next) => {
            if (next !== "history") setHistoryInstanceId(null);
            setPage(next);
          }}
          status={status.data}
        >
          {page === "instances" ? (
            <InstancesPage
              onShowHistory={(instanceId) => {
                setHistoryInstanceId(instanceId);
                setPage("history");
              }}
            />
          ) : null}
          {page === "history" ? (
            <HistoryPage instanceId={historyInstanceId} onInstanceChange={setHistoryInstanceId} />
          ) : null}
          {page === "settings" ? <SettingsPage status={status.data} /> : null}
        </AppLayout>
      </BackupJobsProvider>
    </VaultGate>
  );
}
