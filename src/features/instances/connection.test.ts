import { describe, expect, it } from "vitest";

import type { InstanceView, ProbeReport } from "../../lib/types";
import { connectionState } from "./connection";

function probe(patch: Partial<ProbeReport> = {}): ProbeReport {
  return {
    baseUrl: "https://cliente1.nube.example.com/",
    https: true,
    version: { major: 17, minor: 0, serverVersion: "17.0-20260810", saas: false },
    supported: true,
    database: "cliente1",
    protocol: "xml_rpc",
    uid: 2,
    auth: { status: "ok" },
    module: { status: "failed", code: "module_not_installed", message: "not installed" },
    moduleApiVersion: null,
    dbManager: { status: "ok" },
    recommendedTransport: "db_manager",
    warnings: [],
    checkedAt: "2026-09-16T10:00:00Z",
    ...patch,
  };
}

function instance(patch: Partial<InstanceView> = {}): InstanceView {
  return {
    id: "inst-1",
    name: "Cliente Uno",
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
    lastProbe: probe(),
    lastBackup: null,
    createdAt: "2026-09-16T10:00:00Z",
    updatedAt: "2026-09-16T10:00:00Z",
    ...patch,
  };
}

describe("connectionState", () => {
  it("is 'Sin probar' without a probe", () => {
    const state = connectionState(instance({ lastProbe: null }));
    expect(state).toMatchObject({ label: "Sin probar", tone: "neutral", version: null, technical: null });
  });

  it("is ready with a working database manager and master password", () => {
    const state = connectionState(instance());
    expect(state).toMatchObject({ label: "Lista", tone: "success", version: "17.0", technical: "XML-RPC · Gestor de BD" });
  });

  it("is ready with the obd_backup module and an API key", () => {
    const state = connectionState(
      instance({
        secretKind: "api_key",
        hasMasterPassword: false,
        lastProbe: probe({ protocol: "json2", module: { status: "ok" }, recommendedTransport: "obd_module" }),
      }),
    );
    expect(state).toMatchObject({ label: "Lista", tone: "success", technical: "JSON-2 · Módulo obd_backup" });
  });

  it("flags unsupported versions", () => {
    const state = connectionState(instance({ lastProbe: probe({ supported: false }) }));
    expect(state).toMatchObject({ label: "Versión no soportada", tone: "warning" });
  });

  it("flags rejected credentials", () => {
    const state = connectionState(
      instance({ lastProbe: probe({ auth: { status: "failed", code: "authentication_failed", message: "x" } }) }),
    );
    expect(state).toMatchObject({ label: "Requiere atención", tone: "danger" });
    expect(state.reason).toContain("credenciales");
  });

  it("needs attention when no transport is available", () => {
    const state = connectionState(
      instance({
        lastProbe: probe({ dbManager: { status: "failed", code: "db_manager_disabled", message: "x" }, recommendedTransport: null }),
      }),
    );
    expect(state).toMatchObject({ label: "Requiere atención", tone: "warning" });
    expect(state.technical).toBe("XML-RPC");
  });

  it("uses the current master password, not only the probe recommendation", () => {
    // Probed before the master password was saved: the database manager is usable now.
    const state = connectionState(instance({ lastProbe: probe({ recommendedTransport: null }) }));
    expect(state).toMatchObject({ label: "Lista", technical: "XML-RPC · Gestor de BD" });
  });

  it("checks the forced transport requirements", () => {
    expect(connectionState(instance({ transport: "db_manager", hasMasterPassword: false })).reason).toContain("contraseña maestra");
    expect(
      connectionState(instance({ transport: "db_manager", lastProbe: probe({ dbManager: { status: "failed", code: "db_manager_disabled", message: "x" } }) }))
        .label,
    ).toBe("Requiere atención");
    expect(connectionState(instance({ transport: "obd_module" })).reason).toContain("obd_backup");
    expect(
      connectionState(instance({ transport: "obd_module", secretKind: "password", lastProbe: probe({ module: { status: "ok" } }) })).reason,
    ).toContain("API key");
  });
});
