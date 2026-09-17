import { describe, expect, it } from "vitest";

import { AppError } from "../../lib/errors";
import type { SettingsSchema } from "../../lib/types";
import {
  buildSubmission,
  coerceNumber,
  fieldKind,
  FORM_ERROR_KEY,
  initialDraft,
  orderedKeys,
  parseServerErrors,
  validateDraft,
  validateSubmissionLikeBackend,
} from "./schemaForm";

const schema: SettingsSchema = {
  type: "object",
  title: "Ajustes",
  properties: {
    endpoint: { type: "string", title: "URL", format: "url", default: "https://api.example.com" },
    token: { type: "string", title: "Token", secret: true, minLength: 10 },
    retries: { type: "integer", minimum: 0, maximum: 10, default: 3 },
    ratio: { type: "number", minimum: 0, maximum: 1 },
    mode: { type: "string", enum: ["rapido", "seguro"], enumLabels: ["Rápido", "Seguro"] },
    level: { type: "integer", enum: [1, 2, 3] },
    notify: { type: "boolean", default: true },
    notes: { type: "string", format: "multiline", maxLength: 5 },
    contact: { type: "string", format: "email" },
    tag: { type: "string", pattern: "^[a-z]+$" },
    pin: { type: "string", format: "password" },
  },
  propertyOrder: ["endpoint", "token", "retries", "ratio", "mode", "level", "notify", "notes", "contact", "tag"],
  required: ["endpoint", "token", "mode"],
};

describe("field kinds and order", () => {
  it("maps properties to controls", () => {
    expect(fieldKind(schema.properties.endpoint!)).toBe("url");
    expect(fieldKind(schema.properties.token!)).toBe("secret");
    expect(fieldKind(schema.properties.pin!)).toBe("secret");
    expect(fieldKind(schema.properties.retries!)).toBe("integer");
    expect(fieldKind(schema.properties.ratio!)).toBe("number");
    expect(fieldKind(schema.properties.mode!)).toBe("enum");
    expect(fieldKind(schema.properties.level!)).toBe("enum");
    expect(fieldKind(schema.properties.notify!)).toBe("boolean");
    expect(fieldKind(schema.properties.notes!)).toBe("multiline");
    expect(fieldKind(schema.properties.contact!)).toBe("email");
    expect(fieldKind(schema.properties.tag!)).toBe("text");
  });

  it("keeps manifest order and appends properties missing from propertyOrder", () => {
    expect(orderedKeys(schema)).toEqual([...schema.propertyOrder, "pin"]);
  });
});

describe("initialDraft / buildSubmission", () => {
  it("uses stored values, then defaults, and never pre-fills secrets", () => {
    const draft = initialDraft(schema, { values: { retries: 7, mode: "seguro", level: 2, notify: false } });
    expect(draft).toMatchObject({
      endpoint: "https://api.example.com",
      token: "",
      retries: "7",
      ratio: "",
      mode: "1",
      level: "1",
      notify: false,
      notes: "",
      pin: "",
    });
  });

  it("coerces values back to their types", () => {
    const draft = { ...initialDraft(schema, { values: {} }), retries: "4", ratio: "0.25", mode: "0", level: "2", notes: "", token: "" };
    const values = buildSubmission(schema, draft, []);
    expect(values).toMatchObject({ endpoint: "https://api.example.com", retries: 4, ratio: 0.25, mode: "rapido", level: 3, notify: true, notes: null });
    expect("token" in values).toBe(false);
  });

  it("sends secrets only when written or removed", () => {
    const draft = { ...initialDraft(schema, { values: {} }), token: "abcdefghijk" };
    expect(buildSubmission(schema, draft, []).token).toBe("abcdefghijk");
    expect(buildSubmission(schema, { ...draft, token: "" }, ["token"]).token).toBeNull();
  });

  it("parses numbers strictly", () => {
    expect(coerceNumber(" 3 ", true)).toBe(3);
    expect(coerceNumber("3.5", true)).toBeNull();
    expect(coerceNumber("3.5", false)).toBe(3.5);
    expect(coerceNumber("abc", false)).toBeNull();
    expect(coerceNumber("", false)).toBeNull();
  });
});

describe("validateDraft", () => {
  const valid = () => ({ ...initialDraft(schema, { values: {} }), token: "0123456789", mode: "0" });

  it("accepts a valid draft", () => {
    expect(validateDraft(schema, valid(), [], [])).toEqual({});
  });

  it("reports required, type, range, length, pattern and format errors in Spanish", () => {
    const errors = validateDraft(
      schema,
      { ...valid(), endpoint: "", token: "short", retries: "11", ratio: "x", mode: "", notes: "123456", contact: "no-mail", tag: "ABC" },
      [],
      [],
    );
    expect(errors).toEqual({
      endpoint: "Campo obligatorio.",
      token: "Debe tener al menos 10 caracteres.",
      retries: "Debe ser menor o igual a 10.",
      ratio: "Debe ser un número.",
      mode: "Campo obligatorio.",
      notes: "Debe tener como máximo 5 caracteres.",
      contact: "Debe ser un correo electrónico válido.",
      tag: "No tiene el formato esperado.",
    });
    expect(validateDraft(schema, { ...valid(), endpoint: "ftp://x" }, [], []).endpoint).toBe("Debe ser una URL http:// o https://.");
    expect(validateDraft(schema, { ...valid(), retries: "2.5" }, [], []).retries).toBe("Debe ser un número entero.");
  });

  it("treats a stored secret as satisfying required unless it is being removed", () => {
    const draft = { ...valid(), token: "" };
    expect(validateDraft(schema, draft, ["token"], []).token).toBeUndefined();
    expect(validateDraft(schema, draft, ["token"], ["token"]).token).toBe("Campo obligatorio.");
    expect(validateDraft(schema, draft, [], []).token).toBe("Campo obligatorio.");
  });
});

describe("parseServerErrors", () => {
  it("maps plugin_settings_invalid JSON to field messages", () => {
    const error = new AppError(
      "plugin_settings_invalid",
      JSON.stringify([
        { field: "retries", code: "maximum", message: "must be <= 10" },
        { field: "ghost", code: "unknown_field", message: "unknown" },
      ]),
    );
    expect(parseServerErrors(error, schema)).toEqual({
      retries: "Debe ser menor o igual a 10.",
      [FORM_ERROR_KEY]: "ghost: Campo desconocido para este plugin.",
    });
  });

  it("handles malformed payloads and other errors", () => {
    expect(parseServerErrors(new AppError("plugin_settings_invalid", "not json"), schema)).toHaveProperty(FORM_ERROR_KEY);
    expect(parseServerErrors({ code: "vault_locked", message: "x" }, schema)).toBeNull();
  });
});

describe("validateSubmissionLikeBackend (mock)", () => {
  it("applies absent/null/secret rules", () => {
    const ok = validateSubmissionLikeBackend(schema, { endpoint: null, mode: "seguro", token: "0123456789abc", retries: 5 }, {}, []);
    expect(ok).toEqual({
      ok: true,
      values: { endpoint: "https://api.example.com", retries: 5, mode: "seguro", notify: true },
      setSecrets: { token: "0123456789abc" },
      removeSecrets: [],
    });

    const bad = validateSubmissionLikeBackend(schema, { mode: "x", token: null, extra: 1 }, {}, ["token"]);
    expect(bad.ok).toBe(false);
    if (!bad.ok) {
      expect(bad.errors.map((e) => `${e.field}:${e.code}`).sort()).toEqual(["extra:unknown_field", "mode:enum", "token:required"]);
    }
  });
});
