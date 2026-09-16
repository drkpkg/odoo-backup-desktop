import type { OdooVersion } from "./types";

const LOCALE = "es";

const UNITS = ["B", "KB", "MB", "GB", "TB"] as const;

/** 1536 → "1,5 KB" (base 1024). */
export function formatBytes(bytes: number | null | undefined): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) return "—";
  if (bytes < 1024) return `${bytes} B`;
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = value >= 100 ? 0 : 1;
  return `${new Intl.NumberFormat(LOCALE, { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(value)} ${UNITS[unit]}`;
}

/** 75 → "01:15"; 3725 → "1:02:05". */
export function formatClock(totalSeconds: number): string {
  const secs = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** 45 → "45 s"; 125 → "2 min 5 s"; 3725 → "1 h 2 min". */
export function formatDuration(totalSeconds: number | null | undefined): string {
  if (totalSeconds === null || totalSeconds === undefined || !Number.isFinite(totalSeconds) || totalSeconds < 0) {
    return "—";
  }
  const secs = Math.round(totalSeconds);
  if (secs < 60) return `${secs} s`;
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h > 0) return m > 0 ? `${h} h ${m} min` : `${h} h`;
  return s > 0 ? `${m} min ${s} s` : `${m} min`;
}

/** Duración entre dos fechas ISO en segundos (null si falta alguna). */
export function secondsBetween(startIso: string | null | undefined, endIso: string | null | undefined): number | null {
  if (!startIso || !endIso) return null;
  const start = Date.parse(startIso);
  const end = Date.parse(endIso);
  if (Number.isNaN(start) || Number.isNaN(end)) return null;
  return Math.max(0, (end - start) / 1000);
}

/** "16 sept 2026, 10:30". */
export function formatDateTime(iso: string | null | undefined): string {
  if (!iso) return "—";
  const date = new Date(iso);
  if (Number.isNaN(date.getTime())) return "—";
  return new Intl.DateTimeFormat(LOCALE, { dateStyle: "medium", timeStyle: "short" }).format(date);
}

const RELATIVE_STEPS: [Intl.RelativeTimeFormatUnit, number][] = [
  ["second", 60],
  ["minute", 60],
  ["hour", 24],
  ["day", 30],
  ["month", 12],
  ["year", Number.POSITIVE_INFINITY],
];

/** "hace 5 minutos". `now` inyectable para pruebas. */
export function formatRelative(iso: string | null | undefined, now: number = Date.now()): string {
  if (!iso) return "—";
  const time = Date.parse(iso);
  if (Number.isNaN(time)) return "—";
  let value = (time - now) / 1000;
  const rtf = new Intl.RelativeTimeFormat(LOCALE, { numeric: "auto" });
  for (const [unit, size] of RELATIVE_STEPS) {
    if (Math.abs(value) < size) return rtf.format(Math.round(value), unit);
    value /= size;
  }
  return formatDateTime(iso);
}

/** Porcentaje entero 0..100, o null si no hay total. */
export function percent(done: number | null | undefined, total: number | null | undefined): number | null {
  if (done === null || done === undefined || !total || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.floor((done / total) * 100)));
}

/** { major: 17, minor: 2, saas: true } → "saas~17.2". */
export function formatOdooVersion(version: OdooVersion | null | undefined): string {
  if (!version) return "—";
  return version.saas ? `saas~${version.major}.${version.minor}` : `${version.major}.${version.minor}`;
}

/** Acorta un hash para tablas: "3fa9c1…9e2d". */
export function shortHash(hash: string | null | undefined, size = 6): string {
  if (!hash) return "—";
  return hash.length <= size * 2 + 1 ? hash : `${hash.slice(0, size)}…${hash.slice(-4)}`;
}
