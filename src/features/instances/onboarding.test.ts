import { describe, expect, it } from "vitest";

import type { HistoryEntry, InstanceView, ProbeReport } from "../../lib/types";
import { onboardingState } from "./onboarding";

const readyProbe: ProbeReport = {
  baseUrl: "https://cliente1.nube.example.com/",
  https: true,
  version: { major: 17, minor: 0, serverVersion: "17.0", saas: false },
  supported: true,
  database: "cliente1",
  protocol: "xml_rpc",
  uid: 2,
  auth: { status: "ok" },
  module: { status: "failed", code: "module_not_installed", message: "x" },
  moduleApiVersion: null,
  dbManager: { status: "ok" },
  recommendedTransport: "db_manager",
  warnings: [],
  checkedAt: "2026-09-16T10:00:00Z",
};

const success = { status: "success" } as HistoryEntry;
const failed = { status: "failed" } as HistoryEntry;

function instance(id: string, patch: Partial<InstanceView> = {}): InstanceView {
  return {
    id,
    name: `Cliente ${id}`,
    url: "https://cliente1.nube.example.com/",
    database: "cliente1",
    login: "admin",
    secretKind: "password",
    hasSecret: true,
    hasMasterPassword: true,
    transport: "auto",
    protocol: "auto",
    includeFilestore: true,
    uploadToDrive: false,
    lastProbe: null,
    lastBackup: null,
    createdAt: "2026-09-16T10:00:00Z",
    updatedAt: "2026-09-16T10:00:00Z",
    ...patch,
  };
}

describe("onboardingState", () => {
  it("starts by adding an instance; the folder step is already done", () => {
    const state = onboardingState([]);
    expect(state.next).toEqual({ kind: "add_instance" });
    expect(state.done).toEqual({ add_instance: false, probe: false, folder: true, first_backup: false });
    expect(state.completedCount).toBe(1);
    expect(state.finished).toBe(false);
  });

  it("asks to probe an untested instance", () => {
    const untested = instance("a");
    expect(onboardingState([untested]).next).toEqual({ kind: "probe", instance: untested });
  });

  it("asks to fix an instance that needs attention", () => {
    const broken = instance("a", { hasMasterPassword: false, lastProbe: { ...readyProbe, recommendedTransport: null } });
    const state = onboardingState([broken]);
    expect(state.next).toMatchObject({ kind: "fix", instance: broken });
    expect(state.next && "reason" in state.next ? state.next.reason : "").toContain("contraseña maestra");
  });

  it("prefers backing up a ready instance, even after a failed attempt", () => {
    const untested = instance("a");
    const ready = instance("b", { lastProbe: readyProbe, lastBackup: failed });
    const state = onboardingState([untested, ready]);
    expect(state.next).toEqual({ kind: "backup", instance: ready });
    expect(state.completedCount).toBe(3);
  });

  it("points at the running backup", () => {
    const ready = instance("b", { lastProbe: readyProbe });
    expect(onboardingState([ready], new Set(["b"])).next).toEqual({ kind: "in_progress", instance: ready });
  });

  it("finishes after the first completed backup", () => {
    const state = onboardingState([instance("a", { lastProbe: readyProbe, lastBackup: success })]);
    expect(state).toMatchObject({ finished: true, next: null, completedCount: 4 });
  });
});
