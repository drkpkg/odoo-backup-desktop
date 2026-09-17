import { useQuery } from "@tanstack/react-query";
import { CircleAlert, CircleCheck, CircleSlash, X } from "lucide-react";
import { useState } from "react";

import { Button, IconButton } from "../../components/Button";
import { ProgressBar } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatBytes, formatClock } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { TRANSPORT_LABELS } from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import { useBackupJobs } from "./BackupJobsProvider";
import { describeJob, type JobState } from "./jobs";

/** Panel flotante con el progreso de los backups de esta sesión. */
export function BackupJobsPanel() {
  const { state } = useBackupJobs();
  const jobs = state.order.map((id) => state.jobs[id]).filter((job): job is JobState => Boolean(job));
  const instances = useQuery({ queryKey: queryKeys.instances, queryFn: () => ipc.listInstances() });

  if (jobs.length === 0) return null;

  const nameFor = (instanceId: string) => instances.data?.find((i) => i.id === instanceId)?.name ?? "Instancia";

  return (
    <section
      aria-label="Respaldos en curso"
      className="fixed right-4 bottom-4 z-40 flex max-h-[60vh] w-96 flex-col gap-2 overflow-y-auto"
    >
      {jobs.map((job) => (
        <JobCard key={job.jobId} job={job} instanceName={nameFor(job.instanceId)} />
      ))}
    </section>
  );
}

function JobCard({ job, instanceName }: { job: JobState; instanceName: string }) {
  const { cancel, dismiss } = useBackupJobs();
  const toast = useToast();
  const [cancelling, setCancelling] = useState(false);
  const elapsed = Math.max(0, ((job.finishedAt ?? Date.now()) - job.startedAt) / 1000);

  const doCancel = async () => {
    setCancelling(true);
    try {
      await cancel(job.jobId);
    } catch (err) {
      toast.error("No se pudo cancelar el respaldo", errorMessage(err));
      setCancelling(false);
    }
  };

  return (
    <article className="rounded-lg border border-border bg-surface p-3 shadow-overlay">
      <header className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-sm font-semibold">{instanceName}</p>
          <p className="text-xs text-subtle">
            {job.transport ? TRANSPORT_LABELS[job.transport] : "Respaldo"}
            {job.status === "running" && job.live ? ` · ${formatClock(elapsed)}` : ""}
          </p>
        </div>
        {job.status !== "running" ? (
          <IconButton label="Cerrar" icon={<X size={14} />} onClick={() => dismiss(job.jobId)} />
        ) : null}
      </header>

      <div className="mt-2">
        {job.status === "running" ? <RunningBody job={job} /> : null}
        {job.status === "completed" ? (
          <p className="flex items-center gap-1.5 text-[13px] text-success">
            <CircleCheck size={15} />
            {job.entry
              ? `Completado · ${formatBytes(job.entry.sizeBytes)}${job.entry.drive.status === "success" ? " · subido a Drive" : ""}`
              : "Terminado. Revisa el historial."}
          </p>
        ) : null}
        {job.status === "failed" ? (
          <div className="space-y-1">
            <p className="flex items-center gap-1.5 text-[13px] text-danger">
              <CircleAlert size={15} className="shrink-0" />
              {messageForCode(job.error?.code ?? "internal")}
            </p>
            {job.error?.message ? (
              <p className="line-clamp-2 font-mono text-[11px] break-all text-subtle" title={job.error.message}>
                {job.error.message}
              </p>
            ) : null}
          </div>
        ) : null}
        {job.status === "cancelled" ? (
          <p className="flex items-center gap-1.5 text-[13px] text-muted">
            <CircleSlash size={15} /> Cancelado
          </p>
        ) : null}
      </div>

      {job.status === "running" ? (
        <div className="mt-2.5 flex justify-end">
          <Button size="sm" variant="ghost" onClick={doCancel} loading={cancelling}>
            Cancelar
          </Button>
        </div>
      ) : null}
    </article>
  );
}

function RunningBody({ job }: { job: JobState }) {
  const { label, percent } = describeJob(job);
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2 text-[13px]">
        <span className="min-w-0 truncate tabular" title={label}>
          {label}
        </span>
        {percent !== null ? <span className="text-xs text-muted tabular">{percent}%</span> : null}
      </div>
      <ProgressBar value={percent} label={label} />
    </div>
  );
}
