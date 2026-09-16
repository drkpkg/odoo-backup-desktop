import { describe, expect, it } from "vitest";

import { AppError, ERROR_MESSAGES, errorMessage, FALLBACK_ERROR_MESSAGE, messageForCode, toAppError } from "./errors";

describe("toAppError", () => {
  it("keeps command errors from invoke", () => {
    const error = toAppError({ code: "vault_wrong_password", message: "wrong master password" });
    expect(error).toBeInstanceOf(AppError);
    expect(error.code).toBe("vault_wrong_password");
    expect(error.message).toBe("wrong master password");
  });

  it("parses JSON strings and wraps plain strings/errors", () => {
    expect(toAppError('{"code":"timeout","message":"request timed out"}').code).toBe("timeout");
    const plain = toAppError("boom");
    expect(plain.code).toBe("internal");
    expect(plain.message).toBe("boom");
    expect(toAppError(new Error("js failure")).code).toBe("internal");
    expect(toAppError(42).message).toBe("42");
  });

  it("returns AppError instances unchanged", () => {
    const original = new AppError("cancelled", "operation cancelled");
    expect(toAppError(original)).toBe(original);
  });
});

describe("messages", () => {
  it("maps known codes to Spanish", () => {
    expect(messageForCode("db_manager_disabled")).toContain("list_db");
    expect(messageForCode("module_not_installed")).toContain("obd_backup");
    expect(messageForCode("storage_auth_expired")).toContain("Google Drive");
    expect(errorMessage({ code: "authentication_failed", message: "x" })).toBe(ERROR_MESSAGES.authentication_failed);
  });

  it("falls back for unknown codes", () => {
    expect(messageForCode("something_new")).toBe(FALLBACK_ERROR_MESSAGE);
  });

  it("covers every error code declared by the Rust crates", () => {
    const rustCodes = [
      // obd-vault
      "vault_not_found", "vault_exists", "vault_wrong_password", "vault_password_not_enabled", "keychain_unavailable",
      "keychain_key_missing", "keychain_key_mismatch", "vault_corrupted", "vault_unsupported_version",
      "vault_invalid_options", "keychain_error", "vault_serde", "io",
      // obd-odoo
      "invalid_url", "connection", "timeout", "http_status", "version_detection", "unsupported_version",
      "unsupported_protocol", "authentication_failed", "access_denied", "db_manager_disabled", "module_not_installed",
      "module_api_incompatible", "rpc", "server_backup_error", "prepare_timeout", "invalid_backup", "protocol", "cancelled",
      // obd-storage
      "storage_not_configured", "storage_auth_expired", "storage_authorization_denied", "storage_quota_exceeded",
      "storage_rate_limited", "storage_not_found", "storage_transient", "storage_fatal",
    ];
    const missing = rustCodes.filter((code) => !(code in ERROR_MESSAGES));
    expect(missing).toEqual([]);
  });
});
