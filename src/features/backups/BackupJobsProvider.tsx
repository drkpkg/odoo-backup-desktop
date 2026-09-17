import { useQueryClient } from "@tanstack/react-query";
import { createContext, useCallback, useContext, useEffect, useMemo, useReducer, useRef, type ReactNode } from "react";

import { useToast } from "../../components/Toast";
import { messageForCode, toAppError } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { BackupEvent, InstanceView } from "../../lib/types";
import { initialJobsState, jobsReducer, runningJobs, type JobState, type JobsState } from "./jobs";

type BackupJobsApi = {
  state: JobsState;
  running: JobState[];
  /** Inicia un backup; devuelve el jobId o lanza `AppError`. */
  start: (instanceId: string) => Promise<string>;
  cancel: (jobId: string) => Promise<void>;
  dismiss: (jobId: string) => void;
  runningFor: (instanceId: string) => JobState | undefined;
};

const BackupJobsContext = createContext<BackupJobsApi | null>(null);

const RESTORE_POLL_MS = 2000;

export function BackupJobsProvider({ children }: { children: ReactNode }) {
  const [state, dispatch] = useReducer(jobsReducer, initialJobsState);
  const queryClient = useQueryClient();
  const toast = useToast();
  const stateRef = useRef(state);
  stateRef.current = state;

  const instanceName = useCallback(
    (instanceId: string) =>
      queryClient.getQueryData<InstanceView[]>(queryKeys.instances)?.find((i) => i.id === instanceId)?.name ??
      "la instancia",
    [queryClient],
  );

  const refreshData = useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: queryKeys.instances });
    void queryClient.invalidateQueries({ queryKey: queryKeys.historyAll });
  }, [queryClient]);

  const handleEvent = useCallback(
    (instanceId: string, event: BackupEvent) => {
      dispatch({ type: "event", event, instanceId, now: Date.now() });
      switch (event.type) {
        case "completed":
          refreshData();
          toast.success(
            `Respaldo completado: ${event.entry.instanceName}`,
            event.entry.drive.status === "failed" ? "El archivo se guardó, pero falló la subida a Google Drive." : undefined,
          );
          setTimeout(() => dispatch({ type: "dismiss", jobId: event.jobId }), 10_000);
          break;
        case "failed":
          refreshData();
          toast.error(`Falló el respaldo de ${instanceName(instanceId)}`, messageForCode(event.code));
          break;
        case "cancelled":
          refreshData();
          toast.info(`Respaldo cancelado: ${instanceName(instanceId)}`);
          setTimeout(() => dispatch({ type: "dismiss", jobId: event.jobId }), 4_000);
          break;
        default:
          break;
      }
    },
    [instanceName, refreshData, toast],
  );

  const start = useCallback(
    async (instanceId: string) => {
      try {
        const jobId = await ipc.startBackup(instanceId, (event) => handleEvent(instanceId, event));
        void queryClient.invalidateQueries({ queryKey: queryKeys.historyAll });
        return jobId;
      } catch (error) {
        throw toAppError(error);
      }
    },
    [handleEvent, queryClient],
  );

  const cancel = useCallback(async (jobId: string) => {
    try {
      await ipc.cancelBackup(jobId);
    } catch (error) {
      const appError = toAppError(error);
      // Ya terminó: nada que cancelar.
      if (appError.code !== "job_not_found") throw appError;
    }
  }, []);

  const dismiss = useCallback((jobId: string) => dispatch({ type: "dismiss", jobId }), []);

  // Restaurar jobs en curso (p. ej. tras recargar la ventana) y seguirlos por polling.
  useEffect(() => {
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;

    const poll = async (first: boolean) => {
      try {
        const active = await ipc.listActiveJobs();
        if (stopped) return;
        const now = Date.now();
        dispatch({ type: "restore", jobs: active, now });
        const activeIds = new Set(active.map((j) => j.jobId));
        const vanished = runningJobs(stateRef.current)
          .filter((job) => !job.live && !activeIds.has(job.jobId))
          .map((job) => job.jobId);
        if (vanished.length > 0) {
          dispatch({ type: "vanished", jobIds: vanished, now });
          refreshData();
          for (const jobId of vanished) setTimeout(() => dispatch({ type: "dismiss", jobId }), 6_000);
        }
        const needsPolling = first ? active.length > 0 : runningJobs(stateRef.current).some((job) => !job.live);
        if (needsPolling || active.some((j) => !stateRef.current.jobs[j.jobId]?.live)) {
          timer = setTimeout(() => void poll(false), RESTORE_POLL_MS);
        }
      } catch {
        // El listado de jobs es informativo; se reintenta en la próxima carga.
      }
    };

    void poll(true);
    return () => {
      stopped = true;
      if (timer) clearTimeout(timer);
    };
  }, [refreshData]);

  const api = useMemo<BackupJobsApi>(() => {
    const running = runningJobs(state);
    return {
      state,
      running,
      start,
      cancel,
      dismiss,
      runningFor: (instanceId) => running.find((job) => job.instanceId === instanceId),
    };
  }, [state, start, cancel, dismiss]);

  return <BackupJobsContext.Provider value={api}>{children}</BackupJobsContext.Provider>;
}

export function useBackupJobs(): BackupJobsApi {
  const api = useContext(BackupJobsContext);
  if (!api) throw new Error("useBackupJobs must be used inside BackupJobsProvider");
  return api;
}
