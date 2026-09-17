import { useQuery } from "@tanstack/react-query";
import { ChevronDown, ChevronUp, CircleAlert, CircleCheck, CircleSlash, LoaderCircle, X } from "lucide-react";
import { useEffect, useId, useState } from "react";

import { Button, IconButton } from "../../components/Button";
import { ProgressBar } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatBytes, formatClock } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { TRANSPORT_LABELS } from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import { useBackupJobs } from "./BackupJobsProvider";
import { describeJob, jobsSummary, stageAnnouncement, type JobState } from "./jobs";

const HIGHLIGHT_MS = 1600;

export const jobCardId = (jobId: string) => `backup-job-${jobId}`;

/**
 * Panel flotante: el único lugar con el progreso detallado de los respaldos de esta sesión.
 * Minimizado muestra solo el resumen; las filas de instancias enlazan aquí con "Ver progreso".
 */
export function BackupJobsPanel() {
  const { state, panelExpanded, setPanelExpanded, highlight } = useBackupJobs();
  const jobs = state.order.map((id) => state.jobs[id]).filter((job): job is JobState => Boolean(job));
  const instances = useQuery({ queryKey: queryKeys.instances, queryFn: () => ipc.listInstances() });
  const listId = useId();
  const [highlighted, setHighlighted] = useState<string | null>(null);

  // "Ver progreso": llevar la tarjeta a la vista, enfocarla y resaltarla un momento.
  useEffect(() => {
    if (!highlight) return;
    const card = document.getElementById(jobCardId(highlight.jobId));
    if (!card) return;
    card.scrollIntoView({ block: "nearest" });
    card.focus({ preventScroll: true });
    setHighlighted(highlight.jobId);
    const timer = setTimeout(() => setHighlighted(null), HIGHLIGHT_MS);
    return () => clearTimeout(timer);
  }, [highlight]);

  if (jobs.length === 0) return null;

  const nameFor = (instanceId: string) => instances.data?.find((i) => i.id === instanceId)?.name ?? "Instancia";
  const running = jobs.some((job) => job.status === "running");
  const failed = jobs.some((job) => job.status === "failed");
  const summary = jobsSummary(jobs);

  return (
    <section
      aria-label="Respaldos"
      className="fixed right-4 bottom-4 z-40 flex max-h-[min(70vh,32rem)] w-[min(24rem,calc(100vw-2rem))] flex-col overflow-hidden rounded-overlay border border-border bg-surface shadow-overlay"
    >
      <button
        type="button"
        aria-expanded={panelExpanded}
        aria-controls={listId}
        onClick={() => setPanelExpanded(!panelExpanded)}
        title={panelExpanded ? "Minimizar" : "Mostrar el progreso"}
        className="flex w-full shrink-0 items-center gap-2.5 px-3 py-2.5 text-left hover:bg-surface-2"
      >
        {running ? (
          <LoaderCircle size={15} className="shrink-0 animate-spin text-accent motion-reduce:animate-none" aria-hidden="true" />
        ) : failed ? (
          <CircleAlert size={15} className="shrink-0 text-danger" aria-hidden="true" />
        ) : (
          <CircleCheck size={15} className="shrink-0 text-success" aria-hidden="true" />
        )}
        <span className="min-w-0 flex-1 truncate text-[13px] font-semibold tabular">{summary}</span>
        {panelExpanded ? (
          <ChevronDown size={16} className="shrink-0 text-muted" aria-hidden="true" />
        ) : (
          <ChevronUp size={16} className="shrink-0 text-muted" aria-hidden="true" />
        )}
      </button>

      <div id={listId} hidden={!panelExpanded} className="min-h-0 divide-y divide-border overflow-y-auto border-t border-border">
        {jobs.map((job) => (
          <JobCard key={job.jobId} job={job} instanceName={nameFor(job.instanceId)} highlighted={highlighted === job.jobId} />
        ))}
      </div>

      {/* Un único anuncio por job y solo al cambiar de fase (la barra y el resumen no son live). */}
      {jobs.map((job) => (
        <p key={job.jobId} role="status" className="sr-only">
          {stageAnnouncement(job, nameFor(job.instanceId))}
        </p>
      ))}
    </section>
  );
}

function JobCard({ job, instanceName, highlighted }: { job: JobState; instanceName: string; highlighted: boolean }) {
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
    <article
      id={jobCardId(job.jobId)}
      tabIndex={-1}
      aria-label={`Respaldo de ${instanceName}`}
      className={`px-3 py-2.5 transition-colors duration-500 focus:outline-none ${highlighted ? "bg-accent-soft" : ""}`}
    >
      <header className="flex items-start justify-between gap-2">
        <div className="min-w-0">
          <p className="truncate text-sm font-medium" title={instanceName}>
            {instanceName}
          </p>
          <p className="text-xs text-subtle tabular">
            {job.transport ? TRANSPORT_LABELS[job.transport] : "Respaldo"}
            {job.status === "running" && job.live ? ` · ${formatClock(elapsed)}` : ""}
          </p>
        </div>
        {job.status === "running" ? (
          <Button size="sm" variant="ghost" onClick={doCancel} loading={cancelling} aria-label={`Cancelar el respaldo de ${instanceName}`}>
            Cancelar
          </Button>
        ) : (
          <IconButton label="Quitar del panel" icon={<X size={14} />} onClick={() => dismiss(job.jobId)} />
        )}
      </header>

      <div className="mt-1.5">
        {job.status === "running" ? <RunningBody job={job} instanceName={instanceName} /> : null}
        {job.status === "completed" ? (
          <p className="flex items-center gap-1.5 text-[13px] text-success">
            <CircleCheck size={15} aria-hidden="true" />
            {job.entry
              ? `Completado · ${formatBytes(job.entry.sizeBytes)}${job.entry.drive.status === "success" ? " · subido a Drive" : ""}`
              : "Terminado. Revisa el historial."}
          </p>
        ) : null}
        {job.status === "failed" ? (
          <div className="space-y-1">
            <p className="flex items-center gap-1.5 text-[13px] text-danger">
              <CircleAlert size={15} className="shrink-0" aria-hidden="true" />
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
            <CircleSlash size={15} aria-hidden="true" /> Cancelado
          </p>
        ) : null}
      </div>
    </article>
  );
}

function RunningBody({ job, instanceName }: { job: JobState; instanceName: string }) {
  const { label, percent } = describeJob(job);
  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between gap-2 text-[13px]">
        <span className="min-w-0 truncate tabular" title={label}>
          {label}
        </span>
        {/* Solo con total conocido: las fases sin porcentaje no muestran números. */}
        {percent !== null ? <span className="text-xs text-muted tabular">{percent}%</span> : null}
      </div>
      <ProgressBar value={percent} label={`Progreso del respaldo de ${instanceName}`} />
    </div>
  );
}
