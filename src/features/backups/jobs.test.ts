import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { MockBackend } from "../../lib/mock-backend";
import type { BackupEvent, HistoryEntry } from "../../lib/types";
import { describeJob, initialJobsState, jobsReducer, runningJobForInstance, runningJobs, type JobsState } from "./jobs";

const entry: HistoryEntry = {
  id: "h1",
  instanceId: "i1",
  instanceName: "Cliente",
  status: "success",
  transport: "db_manager",
  startedAt: "2026-09-16T10:00:00Z",
  finishedAt: "2026-09-16T10:05:00Z",
  filePath: "/tmp/cliente/cliente_2026-09-16_10-00-00.zip",
  sizeBytes: 1024,
  sha256: "ab".repeat(32),
  odooVersion: "17.0",
  errorCode: null,
  errorMessage: null,
  drive: { status: "skipped", fileId: null, errorMessage: null },
};

function apply(events: BackupEvent[], state: JobsState = initialJobsState): JobsState {
  return events.reduce((acc, event, index) => jobsReducer(acc, { type: "event", event, instanceId: "i1", now: 1000 + index }), state);
}

describe("jobsReducer", () => {
  it("tracks a job from start to completion", () => {
    let state = apply([
      { type: "started", jobId: "j1", instanceId: "i1", transport: "db_manager" },
      { type: "progress", jobId: "j1", stage: "server_preparing", elapsedSecs: 3 },
    ]);
    expect(runningJobForInstance(state, "i1")?.stage).toBe("server_preparing");
    expect(describeJob(state.jobs.j1!).label).toBe("El servidor está preparando el backup (00:03)");

    state = apply([{ type: "progress", jobId: "j1", stage: "downloading", received: 5 * 1024 * 1024, total: null }], state);
    let job = state.jobs.j1!;
    expect(job.elapsedSecs).toBeNull();
    expect(describeJob(job)).toEqual({ label: "Descargando: 5,0 MB recibidos", percent: null });

    state = apply([{ type: "progress", jobId: "j1", stage: "downloading", received: 50, total: 200 }], state);
    expect(describeJob(state.jobs.j1!)).toEqual({ label: "Descargando 50 B de 200 B", percent: 25 });

    state = apply([{ type: "progress", jobId: "j1", stage: "uploading", sent: 150, total: 200 }], state);
    expect(describeJob(state.jobs.j1!)).toEqual({ label: "Subiendo a Google Drive 75%", percent: 75 });

    state = apply([{ type: "completed", jobId: "j1", entry }], state);
    job = state.jobs.j1!;
    expect(job.status).toBe("completed");
    expect(job.entry).toEqual(entry);
    expect(runningJobs(state)).toHaveLength(0);
  });

  it("keeps partial progress within the same stage", () => {
    const state = apply([
      { type: "progress", jobId: "j1", stage: "downloading", received: 10, total: 100 },
      { type: "progress", jobId: "j1", stage: "downloading", received: 40 },
    ]);
    expect(state.jobs.j1!.total).toBe(100);
    expect(state.jobs.j1!.received).toBe(40);
  });

  it("records failures and ignores late events", () => {
    let state = apply([
      { type: "started", jobId: "j1", instanceId: "i1", transport: "db_manager" },
      { type: "failed", jobId: "j1", code: "db_manager_disabled", message: "list_db = False" },
    ]);
    state = apply([{ type: "progress", jobId: "j1", stage: "downloading", received: 1 }], state);
    expect(state.jobs.j1!.status).toBe("failed");
    expect(state.jobs.j1!.error).toEqual({ code: "db_manager_disabled", message: "list_db = False" });
  });

  it("restores active jobs and marks vanished ones as finished", () => {
    let state = jobsReducer(initialJobsState, {
      type: "restore",
      now: 5000,
      jobs: [{ jobId: "r1", instanceId: "i9", stage: "downloading", startedAt: "2026-09-16T10:00:00Z", received: 10, total: 20 }],
    });
    const restored = state.jobs.r1!;
    expect(restored.live).toBe(false);
    expect(restored.startedAt).toBe(Date.parse("2026-09-16T10:00:00Z"));
    expect(describeJob(restored).percent).toBe(50);

    state = jobsReducer(state, { type: "vanished", jobIds: ["r1"], now: 6000 });
    expect(state.jobs.r1!.status).toBe("completed");

    state = jobsReducer(state, { type: "dismiss", jobId: "r1" });
    expect(state.order).toEqual([]);
  });

  it("does not overwrite live jobs when restoring", () => {
    let state = apply([{ type: "progress", jobId: "j1", stage: "validating" }]);
    state = jobsReducer(state, {
      type: "restore",
      now: 1,
      jobs: [{ jobId: "j1", instanceId: "i1", stage: "downloading", startedAt: "2026-09-16T10:00:00Z" }],
    });
    expect(state.jobs.j1!.stage).toBe("validating");
  });
});

describe("mock backend backup simulation", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  async function unlocked(): Promise<MockBackend> {
    const backend = new MockBackend({ vaultExists: true, unlocked: true, speed: 0 });
    return backend;
  }

  it("emits a valid event sequence that the reducer turns into a completed job", async () => {
    const backend = await unlocked();
    const instances = await backend.listInstances();
    const andina = instances.find((i) => i.name.includes("Andina"))!;

    const events: BackupEvent[] = [];
    const jobId = await backend.startBackup(andina.id, (event) => events.push(event));
    expect(await backend.listActiveJobs()).toHaveLength(1);
    await vi.runAllTimersAsync();

    expect(events[0]).toMatchObject({ type: "started", jobId, instanceId: andina.id });
    const stages = events.flatMap((e) => (e.type === "progress" ? [e.stage] : []));
    expect(stages[0]).toBe("requesting");
    expect(stages).toContain("server_preparing");
    expect(stages).toContain("downloading");
    expect(stages).toContain("validating");
    expect(events.at(-1)?.type).toBe("completed");

    const state = events.reduce<JobsState>(
      (acc, event, i) => jobsReducer(acc, { type: "event", event, instanceId: andina.id, now: i }),
      initialJobsState,
    );
    expect(state.jobs[jobId]?.status).toBe("completed");
    expect(await backend.listActiveJobs()).toHaveLength(0);

    const history = await backend.listHistory({ instanceId: andina.id });
    expect(history[0]?.status).toBe("success");
  });

  it("supports cancellation and rejects concurrent backups of the same instance", async () => {
    const backend = await unlocked();
    const [first] = await backend.listInstances();
    const events: BackupEvent[] = [];
    const jobId = await backend.startBackup(first!.id, (event) => events.push(event));
    await expect(backend.startBackup(first!.id, () => undefined)).rejects.toMatchObject({ code: "backup_in_progress" });

    await backend.cancelBackup(jobId);
    await vi.runAllTimersAsync();
    expect(events.at(-1)).toEqual({ type: "cancelled", jobId });
    expect(events.filter((e) => e.type === "cancelled")).toHaveLength(1);
  });

  it("never exposes secrets in instance views", async () => {
    const backend = await unlocked();
    const saved = await backend.saveInstance({
      name: "Nueva",
      url: "https://nueva.nube.com",
      database: "nueva",
      login: "api",
      secretKind: "api_key",
      secret: "super-secret-key",
      masterPassword: "master-secret",
      transport: "auto",
      protocol: "auto",
      includeFilestore: true,
      uploadToDrive: false,
    });
    const serialized = JSON.stringify(await backend.listInstances());
    expect(saved.hasSecret).toBe(true);
    expect(saved.hasMasterPassword).toBe(true);
    expect(serialized).not.toContain("super-secret-key");
    expect(serialized).not.toContain("master-secret");
  });
});
