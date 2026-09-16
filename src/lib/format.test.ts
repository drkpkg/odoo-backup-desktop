import { describe, expect, it } from "vitest";

import {
  formatBytes,
  formatClock,
  formatDateTime,
  formatDuration,
  formatOdooVersion,
  formatRelative,
  percent,
  secondsBetween,
  shortHash,
} from "./format";

describe("formatBytes", () => {
  it("formats bytes with base 1024 and Spanish decimals", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(1023)).toBe("1023 B");
    expect(formatBytes(1536)).toBe("1,5 KB");
    expect(formatBytes(5 * 1024 * 1024)).toBe("5,0 MB");
    expect(formatBytes(250 * 1024 * 1024)).toBe("250 MB");
    expect(formatBytes(3.25 * 1024 ** 3)).toBe("3,3 GB");
  });

  it("returns a dash for missing or invalid values", () => {
    expect(formatBytes(null)).toBe("—");
    expect(formatBytes(undefined)).toBe("—");
    expect(formatBytes(-1)).toBe("—");
    expect(formatBytes(Number.NaN)).toBe("—");
  });
});

describe("durations", () => {
  it("formats a clock", () => {
    expect(formatClock(0)).toBe("00:00");
    expect(formatClock(75)).toBe("01:15");
    expect(formatClock(3725)).toBe("1:02:05");
    expect(formatClock(-5)).toBe("00:00");
  });

  it("formats human durations", () => {
    expect(formatDuration(45)).toBe("45 s");
    expect(formatDuration(120)).toBe("2 min");
    expect(formatDuration(125)).toBe("2 min 5 s");
    expect(formatDuration(3600)).toBe("1 h");
    expect(formatDuration(3725)).toBe("1 h 2 min");
    expect(formatDuration(null)).toBe("—");
  });

  it("computes seconds between ISO dates", () => {
    expect(secondsBetween("2026-09-16T10:00:00Z", "2026-09-16T10:01:30Z")).toBe(90);
    expect(secondsBetween("2026-09-16T10:00:00Z", null)).toBeNull();
    expect(secondsBetween("nope", "2026-09-16T10:00:00Z")).toBeNull();
  });
});

describe("dates", () => {
  it("formats date-time or dash", () => {
    expect(formatDateTime(null)).toBe("—");
    expect(formatDateTime("invalid")).toBe("—");
    expect(formatDateTime("2026-09-16T10:30:00Z")).toMatch(/2026/);
  });

  it("formats relative times in Spanish", () => {
    const now = Date.parse("2026-09-16T12:00:00Z");
    expect(formatRelative("2026-09-16T11:55:00Z", now)).toBe("hace 5 minutos");
    expect(formatRelative("2026-09-15T12:00:00Z", now)).toBe("ayer");
    expect(formatRelative(null, now)).toBe("—");
  });
});

describe("misc", () => {
  it("computes bounded percentages", () => {
    expect(percent(50, 200)).toBe(25);
    expect(percent(300, 200)).toBe(100);
    expect(percent(10, null)).toBeNull();
    expect(percent(10, 0)).toBeNull();
    expect(percent(null, 10)).toBeNull();
  });

  it("labels Odoo versions", () => {
    expect(formatOdooVersion({ major: 17, minor: 0, saas: false, serverVersion: "17.0" })).toBe("17.0");
    expect(formatOdooVersion({ major: 17, minor: 2, saas: true, serverVersion: "saas~17.2" })).toBe("saas~17.2");
    expect(formatOdooVersion(null)).toBe("—");
  });

  it("shortens hashes", () => {
    const hash = "a".repeat(20) + "b".repeat(40) + "cdef";
    expect(shortHash(hash)).toBe("aaaaaa…cdef");
    expect(shortHash("abc")).toBe("abc");
    expect(shortHash(null)).toBe("—");
  });
});
