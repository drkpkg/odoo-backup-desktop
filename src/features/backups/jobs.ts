import { formatBytes, formatClock, percent } from "../../lib/format";
import { STAGE_LABELS } from "../../lib/labels";
import type { ActiveJob, BackupEvent, BackupStage, CommandError, HistoryEntry, TransportKind } from "../../lib/types";

export type JobStatus = "running" | "completed" | "failed" | "cancelled";

export type JobState = {
  jobId: string;
  instanceId: string;
  transport: TransportKind | null;
  status: JobStatus;
  stage: BackupStage | null;
  elapsedSecs: number | null;
  received: number | null;
  total: number | null;
  sent: number | null;
  startedAt: number;
  finishedAt: number | null;
  error: CommandError | null;
  entry: HistoryEntry | null;
  /** Tiene un Channel activo (eventos en vivo). Los restaurados se consultan por polling. */
  live: boolean;
};

export type JobsState = {
  jobs: Record<string, JobState>;
  /** Orden de llegada (más nuevo al final). */
  order: string[];
};

export type JobsAction =
  | { type: "event"; event: BackupEvent; instanceId: string; now: number }
  | { type: "restore"; jobs: ActiveJob[]; now: number }
  | { type: "vanished"; jobIds: string[]; now: number }
  | { type: "dismiss"; jobId: string }
  | { type: "reset" };

export const initialJobsState: JobsState = { jobs: {}, order: [] };

function blankJob(jobId: string, instanceId: string, now: number, live: boolean): JobState {
  return {
    jobId,
    instanceId,
    transport: null,
    status: "running",
    stage: null,
    elapsedSecs: null,
    received: null,
    total: null,
    sent: null,
    startedAt: now,
    finishedAt: null,
    error: null,
    entry: null,
    live,
  };
}

function upsert(state: JobsState, job: JobState): JobsState {
  const exists = job.jobId in state.jobs;
  return {
    jobs: { ...state.jobs, [job.jobId]: job },
    order: exists ? state.order : [...state.order, job.jobId],
  };
}

function applyEvent(state: JobsState, event: BackupEvent, instanceId: string, now: number): JobsState {
  const current = state.jobs[event.jobId] ?? blankJob(event.jobId, instanceId, now, true);
  // Un job terminado ignora eventos tardíos.
  if (current.status !== "running") return state;

  switch (event.type) {
    case "started":
      return upsert(state, {
        ...current,
        instanceId: event.instanceId,
        transport: event.transport,
        live: true,
      });
    case "progress": {
      const stageChanged = current.stage !== event.stage;
      const base = stageChanged
        ? { ...current, elapsedSecs: null, received: null, total: null, sent: null }
        : current;
      return upsert(state, {
        ...base,
        stage: event.stage,
        elapsedSecs: event.elapsedSecs ?? base.elapsedSecs,
        received: event.received ?? base.received,
        total: event.total !== undefined ? event.total : base.total,
        sent: event.sent ?? base.sent,
        live: true,
      });
    }
    case "completed":
      return upsert(state, { ...current, status: "completed", entry: event.entry, finishedAt: now });
    case "failed":
      return upsert(state, {
        ...current,
        status: "failed",
        error: { code: event.code, message: event.message },
        finishedAt: now,
      });
    case "cancelled":
      return upsert(state, { ...current, status: "cancelled", finishedAt: now });
  }
}

export function jobsReducer(state: JobsState, action: JobsAction): JobsState {
  switch (action.type) {
    case "event":
      return applyEvent(state, action.event, action.instanceId, action.now);
    case "restore": {
      let next = state;
      for (const active of action.jobs) {
        const existing = next.jobs[active.jobId];
        if (existing?.live) continue;
        const started = Date.parse(active.startedAt);
        const base = existing ?? blankJob(active.jobId, active.instanceId, Number.isNaN(started) ? action.now : started, false);
        next = upsert(next, {
          ...base,
          status: "running",
          stage: active.stage,
          received: active.received ?? null,
          total: active.total ?? null,
          sent: active.sent ?? null,
        });
      }
      return next;
    }
    case "vanished": {
      let next = state;
      for (const jobId of action.jobIds) {
        const job = next.jobs[jobId];
        if (!job || job.live || job.status !== "running") continue;
        // El job terminó mientras no teníamos canal: el resultado está en el historial.
        next = upsert(next, { ...job, status: "completed", finishedAt: action.now });
      }
      return next;
    }
    case "dismiss": {
      if (!(action.jobId in state.jobs)) return state;
      const jobs = { ...state.jobs };
      delete jobs[action.jobId];
      return { jobs, order: state.order.filter((id) => id !== action.jobId) };
    }
    case "reset":
      return initialJobsState;
  }
}

export function runningJobs(state: JobsState): JobState[] {
  return state.order.map((id) => state.jobs[id]).filter((job): job is JobState => job?.status === "running");
}

export function runningJobForInstance(state: JobsState, instanceId: string): JobState | undefined {
  return runningJobs(state).find((job) => job.instanceId === instanceId);
}

export type JobDescription = {
  label: string;
  /** 0..100, o null para barra indeterminada. */
  percent: number | null;
};

/** Texto de progreso en español para un job en curso. */
export function describeJob(job: JobState): JobDescription {
  switch (job.stage) {
    case null:
    case "requesting":
      return { label: STAGE_LABELS.requesting, percent: null };
    case "server_preparing":
      return {
        label:
          job.elapsedSecs !== null
            ? `${STAGE_LABELS.server_preparing} (${formatClock(job.elapsedSecs)})`
            : STAGE_LABELS.server_preparing,
        percent: null,
      };
    case "downloading": {
      const received = job.received ?? 0;
      if (job.total) {
        return {
          label: `${STAGE_LABELS.downloading} ${formatBytes(received)} de ${formatBytes(job.total)}`,
          percent: percent(received, job.total),
        };
      }
      return { label: `${STAGE_LABELS.downloading}: ${formatBytes(received)} recibidos`, percent: null };
    }
    case "validating":
      return { label: STAGE_LABELS.validating, percent: null };
    case "uploading": {
      const pct = percent(job.sent ?? 0, job.total);
      return {
        label: pct === null ? STAGE_LABELS.uploading : `${STAGE_LABELS.uploading} ${pct}%`,
        percent: pct,
      };
    }
    case "retention":
      return { label: STAGE_LABELS.retention, percent: null };
  }
}
