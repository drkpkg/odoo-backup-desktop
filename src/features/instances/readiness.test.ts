import { describe, expect, it } from "vitest";

import type { ProbeReport } from "../../lib/types";
import { assessReadiness, probeGuidance, type ReadinessInput } from "./readiness";

const failed = (code: string) => ({ status: "failed" as const, code, message: code });

function probe(patch: Partial<ProbeReport> = {}): ProbeReport {
  return {
    baseUrl: "https://cliente1.nube.example.com/",
    https: true,
    version: { major: 18, minor: 0, serverVersion: "18.0-20260901", saas: false },
    supported: true,
    database: "cliente1",
    protocol: "xml_rpc",
    uid: 2,
    auth: { status: "ok" },
    module: failed("module_not_installed"),
    moduleApiVersion: null,
    dbManager: { status: "ok" },
    recommendedTransport: null,
    warnings: [],
    checkedAt: "2026-09-16T10:00:00Z",
    ...patch,
  };
}

const input = (patch: Partial<ReadinessInput> = {}): ReadinessInput => ({
  transport: "auto",
  protocol: "auto",
  secretKind: "api_key",
  hasMasterPassword: false,
  ...patch,
});

describe("assessReadiness", () => {
  it("asks for credentials or the database when authentication was skipped", () => {
    expect(assessReadiness(probe({ auth: { status: "skipped", reason: "no_credentials" } }), input())).toEqual({
      kind: "credentials_missing",
      missing: "credentials",
    });
    expect(assessReadiness(probe({ auth: { status: "skipped", reason: "no_database" } }), input())).toEqual({
      kind: "credentials_missing",
      missing: "database",
    });
  });

  it("reports failed authentication before anything else", () => {
    expect(assessReadiness(probe({ auth: failed("authentication_failed") }), input({ hasMasterPassword: true }))).toEqual({
      kind: "auth_failed",
      code: "authentication_failed",
    });
  });

  it("resolves auto like the backup runner: module with API key first, then the database manager", () => {
    expect(assessReadiness(probe({ module: { status: "ok" } }), input({ hasMasterPassword: true }))).toEqual({
      kind: "ready",
      transport: "obd_module",
    });
    expect(assessReadiness(probe(), input({ hasMasterPassword: true }))).toEqual({ kind: "ready", transport: "db_manager" });
    expect(assessReadiness(probe({ module: { status: "ok" } }), input({ secretKind: "password", hasMasterPassword: true }))).toEqual({
      kind: "ready",
      transport: "db_manager",
    });
  });

  it("points at the missing piece in auto mode", () => {
    expect(assessReadiness(probe(), input())).toEqual({ kind: "master_password_missing", moduleAlternative: true });
    expect(assessReadiness(probe({ module: { status: "ok" } }), input({ secretKind: "password" }))).toEqual({ kind: "api_key_required" });
    expect(assessReadiness(probe({ dbManager: failed("db_manager_disabled") }), input())).toEqual({
      kind: "no_method",
      moduleCode: "module_not_installed",
    });
  });

  it("checks the requirements of a forced transport", () => {
    const disabled = probe({ dbManager: failed("db_manager_disabled"), module: { status: "ok" } });
    expect(assessReadiness(disabled, input({ transport: "db_manager", hasMasterPassword: true }))).toEqual({ kind: "db_manager_disabled" });
    expect(assessReadiness(probe(), input({ transport: "db_manager" }))).toEqual({ kind: "master_password_missing", moduleAlternative: false });
    expect(assessReadiness(probe(), input({ transport: "obd_module" }))).toEqual({ kind: "module_unavailable", code: "module_not_installed" });
    expect(assessReadiness(probe({ module: { status: "ok" } }), input({ transport: "obd_module", secretKind: "password" }))).toEqual({
      kind: "api_key_required",
    });
  });
});

describe("probeGuidance", () => {
  const guidance = (report: ProbeReport, values: ReadinessInput) => probeGuidance(assessReadiness(report, values), values);

  it("confirms a ready instance without an action", () => {
    expect(guidance(probe({ module: { status: "ok" } }), input())).toMatchObject({ tone: "success", title: "Lista para respaldar", action: null });
  });

  it("offers the concrete fix as an action", () => {
    expect(guidance(probe(), input())).toMatchObject({ tone: "warning", action: "add_master_password" });
    expect(guidance(probe({ module: { status: "ok" } }), input({ secretKind: "password" }))).toMatchObject({ action: "use_api_key" });
    expect(guidance(probe({ dbManager: failed("db_manager_disabled") }), input({ transport: "db_manager" }))).toMatchObject({
      action: "use_auto_transport",
    });
    expect(guidance(probe({ auth: failed("unsupported_protocol") }), input({ protocol: "json2" }))).toMatchObject({
      tone: "danger",
      action: "use_auto_protocol",
    });
  });

  it("mentions the credential kind the user chose", () => {
    expect(guidance(probe({ auth: failed("authentication_failed") }), input({ secretKind: "password" })).detail).toContain("la contraseña");
    expect(guidance(probe({ auth: { status: "skipped", reason: "no_credentials" } }), input()).detail).toContain("la API key");
  });

  it("explains why no backup method is available", () => {
    const report = probe({ dbManager: failed("db_manager_disabled") });
    expect(guidance(report, input({ secretKind: "password" }))).toMatchObject({ title: "Falta configurar un método de respaldo", action: "use_api_key" });
    expect(guidance(probe({ dbManager: failed("db_manager_disabled"), module: failed("module_api_incompatible") }), input()).detail).toContain(
      "no es compatible",
    );
  });
});
