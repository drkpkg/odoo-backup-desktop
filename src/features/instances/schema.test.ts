import { describe, expect, it } from "vitest";

import type { InstanceView } from "../../lib/types";
import {
  buildInstanceInput,
  buildProbeRequest,
  databaseFromUrl,
  defaultFormValues,
  isHttpUrl,
  makeInstanceFormSchema,
  normalizeUrl,
  probeRequestForInstance,
  type InstanceFormValues,
} from "./schema";

function values(patch: Partial<InstanceFormValues> = {}): InstanceFormValues {
  return {
    ...defaultFormValues(null),
    name: "Cliente Uno",
    url: "https://cliente1.nube-appex.lat/",
    database: "cliente1",
    login: "backup@cliente1.com",
    secretKind: "api_key",
    secret: "key-123",
    ...patch,
  };
}

function existing(patch: Partial<InstanceView> = {}): InstanceView {
  return {
    id: "inst-1",
    name: "Cliente Uno",
    url: "https://cliente1.nube-appex.lat",
    database: "cliente1",
    login: "backup@cliente1.com",
    secretKind: "api_key",
    hasSecret: true,
    hasMasterPassword: true,
    transport: "auto",
    protocol: "auto",
    includeFilestore: true,
    uploadToDrive: false,
    lastProbe: null,
    lastBackup: null,
    createdAt: "2026-09-01T00:00:00Z",
    updatedAt: "2026-09-01T00:00:00Z",
    ...patch,
  };
}

function issuesFor(schemaValues: InstanceFormValues, ctx = { hasSecret: false, hasMasterPassword: false }) {
  const result = makeInstanceFormSchema(ctx).safeParse(schemaValues);
  if (result.success) return {};
  return Object.fromEntries(result.error.issues.map((issue) => [issue.path.join("."), issue.message]));
}

describe("URL helpers", () => {
  it("validates http(s) URLs", () => {
    expect(isHttpUrl("https://cliente.nube.com")).toBe(true);
    expect(isHttpUrl("http://10.0.0.5:8069")).toBe(true);
    expect(isHttpUrl("ftp://cliente.nube.com")).toBe(false);
    expect(isHttpUrl("cliente.nube.com")).toBe(false);
    expect(isHttpUrl("")).toBe(false);
  });

  it("normalizes trailing slashes and spaces", () => {
    expect(normalizeUrl("  https://cliente.nube.com///  ")).toBe("https://cliente.nube.com");
  });

  it("derives the database from the subdomain like database_from_host", () => {
    expect(databaseFromUrl("https://cliente1.nube-appex.lat")).toBe("cliente1");
    expect(databaseFromUrl("https://www.cliente1.nube.com/web")).toBe("cliente1");
    expect(databaseFromUrl("https://CLIENTE2.Nube.com")).toBe("cliente2");
    expect(databaseFromUrl("https://miempresa.com")).toBe("miempresa");
    expect(databaseFromUrl("http://localhost:8069")).toBeNull();
    expect(databaseFromUrl("http://192.168.1.10:8069")).toBeNull();
    expect(databaseFromUrl("http://[::1]:8069")).toBeNull();
    expect(databaseFromUrl("not a url")).toBeNull();
  });
});

describe("instance form schema", () => {
  it("accepts a complete new instance", () => {
    expect(issuesFor(values())).toEqual({});
  });

  it("requires fields and a valid URL/database", () => {
    const issues = issuesFor(values({ name: " ", url: "cliente.com", database: "bad db", login: "" }));
    expect(Object.keys(issues).sort()).toEqual(["database", "login", "name", "url"]);
  });

  it("requires the secret only when none is stored", () => {
    expect(issuesFor(values({ secret: "" }))).toHaveProperty("secret", "Ingresa la API key.");
    expect(issuesFor(values({ secret: "", secretKind: "password" }))).toHaveProperty("secret", "Ingresa la contraseña.");
    expect(issuesFor(values({ secret: "" }), { hasSecret: true, hasMasterPassword: false })).toEqual({});
  });

  it("requires an API key for JSON-2 and the appex_backup module", () => {
    const issues = issuesFor(values({ secretKind: "password", protocol: "json2", transport: "appex_module" }));
    expect(issues).toHaveProperty("protocol");
    expect(issues).toHaveProperty("transport");
  });

  it("requires a master password for the database manager transport", () => {
    expect(issuesFor(values({ transport: "db_manager" }))).toHaveProperty("masterPassword");
    expect(issuesFor(values({ transport: "db_manager", masterPassword: "super" }))).toEqual({});
    const ctx = { hasSecret: true, hasMasterPassword: true };
    expect(issuesFor(values({ transport: "db_manager" }), ctx)).toEqual({});
    expect(issuesFor(values({ transport: "db_manager", removeMasterPassword: true }), ctx)).toHaveProperty("masterPassword");
  });
});

describe("buildInstanceInput", () => {
  it("creates a new instance with trimmed fields and secrets", () => {
    const input = buildInstanceInput(values({ name: "  Cliente Uno ", masterPassword: "master" }));
    expect(input).toEqual({
      name: "Cliente Uno",
      url: "https://cliente1.nube-appex.lat",
      database: "cliente1",
      login: "backup@cliente1.com",
      secretKind: "api_key",
      secret: "key-123",
      masterPassword: "master",
      transport: "auto",
      protocol: "auto",
      includeFilestore: true,
      uploadToDrive: false,
    });
    expect(input).not.toHaveProperty("id");
  });

  it("omits empty secrets when editing (keep stored values)", () => {
    const input = buildInstanceInput(values({ secret: "", masterPassword: "" }), existing());
    expect(input.id).toBe("inst-1");
    expect(input).not.toHaveProperty("secret");
    expect(input).not.toHaveProperty("masterPassword");
  });

  it("sends null to remove the stored master password", () => {
    const input = buildInstanceInput(values({ removeMasterPassword: true, masterPassword: "ignored" }), existing());
    expect(input.masterPassword).toBeNull();
  });

  it("does not keep passwords with surrounding spaces trimmed", () => {
    expect(buildInstanceInput(values({ secret: " pass with spaces " })).secret).toBe(" pass with spaces ");
  });
});

describe("probe requests", () => {
  it("uses typed values and stored secrets when editing", () => {
    const request = buildProbeRequest(values({ secret: "", database: "", login: " " }), existing());
    expect(request).toEqual({
      instanceId: "inst-1",
      url: "https://cliente1.nube-appex.lat",
      secretKind: "api_key",
      protocol: "auto",
    });
  });

  it("includes secrets typed in the form", () => {
    const request = buildProbeRequest(values({ masterPassword: "m" }));
    expect(request.secret).toBe("key-123");
    expect(request.masterPassword).toBe("m");
    expect(request.database).toBe("cliente1");
  });

  it("builds a request for a saved instance without secrets", () => {
    const request = probeRequestForInstance(existing());
    expect(request).toEqual({
      instanceId: "inst-1",
      url: "https://cliente1.nube-appex.lat",
      database: "cliente1",
      login: "backup@cliente1.com",
      secretKind: "api_key",
      protocol: "auto",
    });
  });
});
