// Backend en memoria para ejecutar la UI en un navegador (`pnpm dev`) sin Tauri.
// Simula la bóveda, instancias, pruebas de conexión, backups con eventos de progreso,
// historial, ajustes y Google Drive. Parámetros de URL:
//   ?mock=fresh        sin bóveda creada
//   ?mock=nokeychain   sin llavero del sistema (requiere contraseña maestra)
//   ?mock=unlocked     bóveda ya desbloqueada

import type { Backend } from "./backend";
import { AppError } from "./errors";
import type {
  ActiveJob,
  AppStatus,
  BackupEvent,
  BackupStage,
  DriveStatus,
  HistoryEntry,
  InstanceInput,
  InstanceView,
  OdooVersion,
  ProbeReport,
  ProbeRequest,
  ProbeWarning,
  Settings,
  TransportKind,
  VaultLockedPayload,
} from "./types";

export type MockOptions = {
  vaultExists?: boolean;
  unlocked?: boolean;
  keychainAvailable?: boolean;
  /** Multiplicador de tiempos (0 = inmediato en pruebas). */
  speed?: number;
};

const MB = 1024 * 1024;
const MOCK_PASSWORD = "obd-demo";

function isoAgo(minutes: number): string {
  return new Date(Date.now() - minutes * 60_000).toISOString();
}

function uuid(): string {
  if (typeof crypto !== "undefined" && "randomUUID" in crypto) return crypto.randomUUID();
  return `id-${Math.random().toString(36).slice(2)}-${Date.now().toString(36)}`;
}

function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

type StoredInstance = InstanceView & {
  secret: string | null;
  masterPassword: string | null;
};

type SimulatedJob = {
  jobId: string;
  instanceId: string;
  historyId: string;
  stage: BackupStage;
  startedAt: string;
  received?: number;
  total?: number | null;
  sent?: number;
  timers: ReturnType<typeof setTimeout>[];
  emit: (event: BackupEvent) => void;
};

function versionFor(major: number, minor = 0, saas = false): OdooVersion {
  return {
    major,
    minor,
    saas,
    serverVersion: saas ? `saas~${major}.${minor}` : `${major}.${minor}-20260901`,
  };
}

function hostOf(url: string): string {
  try {
    return new URL(url).hostname;
  } catch {
    return "";
  }
}

function fakeSha256(seed: string): string {
  let h1 = 0x811c9dc5;
  let out = "";
  for (let round = 0; round < 8; round += 1) {
    for (let i = 0; i < seed.length; i += 1) {
      h1 ^= seed.charCodeAt(i) + round;
      h1 = Math.imul(h1, 0x01000193) >>> 0;
    }
    out += h1.toString(16).padStart(8, "0");
  }
  return out.slice(0, 64);
}

function stamp(date: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}_${pad(date.getHours())}-${pad(date.getMinutes())}-${pad(date.getSeconds())}`;
}

function slug(name: string): string {
  return (
    name
      .normalize("NFD")
      .replace(/\p{Diacritic}/gu, "")
      .toLowerCase()
      .replace(/[^a-z0-9_]+/g, "-")
      .replace(/^-+|-+$/g, "") || "instance"
  );
}

function parseMockOptions(): MockOptions {
  if (typeof window === "undefined") return {};
  const flags = new URLSearchParams(window.location.search).getAll("mock").flatMap((v) => v.split(","));
  return {
    vaultExists: !flags.includes("fresh"),
    unlocked: flags.includes("unlocked"),
    keychainAvailable: !flags.includes("nokeychain"),
  };
}

export class MockBackend implements Backend {
  readonly kind = "mock" as const;

  private speed: number;
  private vault: AppStatus["vault"];
  private instances: StoredInstance[] = [];
  private history: HistoryEntry[] = [];
  private jobs = new Map<string, SimulatedJob>();
  private lockHandlers = new Set<(payload: VaultLockedPayload) => void>();
  private settings: Settings = {
    downloadDir: "/home/usuario/Backups/Odoo",
    keepLastLocal: 10,
    maxConcurrentBackups: 2,
    serverPrepareTimeoutMinutes: 60,
    autoLockMinutes: 15,
    drive: { rootFolderName: "Odoo Backup Desktop", keepLast: 30, permanentDelete: false, sharedDriveId: null },
  };
  private drive: DriveStatus = {
    configured: true,
    clientId: "123456789012-abcdefg.apps.googleusercontent.com",
    hasClientSecret: true,
    connected: true,
    email: "backups@example.com",
    displayName: "Backups Odoo",
  };
  private driveConnect: { timer: ReturnType<typeof setTimeout>; reject: (e: unknown) => void } | null = null;

  constructor(options: MockOptions = {}) {
    this.speed = options.speed ?? 1;
    const keychainAvailable = options.keychainAvailable ?? true;
    const exists = options.vaultExists ?? true;
    this.vault = {
      exists,
      unlocked: exists && (options.unlocked ?? false),
      keychainAvailable,
      keychainEnabled: exists && keychainAvailable,
      passwordEnabled: exists,
    };
    if (exists) this.seed();
  }

  // --- utilidades ---------------------------------------------------------

  private delay(ms: number): Promise<void> {
    const scaled = ms * this.speed;
    return scaled <= 0 ? Promise.resolve() : new Promise((resolve) => setTimeout(resolve, scaled));
  }

  private status(): AppStatus {
    return { appVersion: "0.1.0-mock", vault: { ...this.vault } };
  }

  private requireUnlocked(): void {
    if (!this.vault.unlocked) throw new AppError("vault_locked", "vault is locked");
  }

  private view(instance: StoredInstance): InstanceView {
    // Nunca exponer secretos: solo metadatos.
    const { secret, masterPassword, ...rest } = instance;
    return {
      ...clone(rest),
      hasSecret: secret !== null && secret !== "",
      hasMasterPassword: masterPassword !== null && masterPassword !== "",
      lastBackup: this.history.find((h) => h.instanceId === instance.id) ?? null,
    };
  }

  private seed(): void {
    const andina = this.makeInstance({
      name: "Distribuidora Andina",
      url: "https://andina.nube.example.com",
      database: "andina",
      login: "backup@andina.com",
      secretKind: "password",
      transport: "auto",
      protocol: "auto",
      uploadToDrive: true,
      masterPassword: "********",
      createdAt: isoAgo(60 * 24 * 40),
    });
    const sanRafael = this.makeInstance({
      name: "Clínica San Rafael",
      url: "https://sanrafael.nube.example.com",
      database: "sanrafael",
      login: "api-backup",
      secretKind: "api_key",
      transport: "obd_module",
      protocol: "json2",
      uploadToDrive: true,
      masterPassword: null,
      createdAt: isoAgo(60 * 24 * 20),
    });
    const tornillo = this.makeInstance({
      name: "Ferretería El Tornillo",
      url: "http://tornillo.nube.example.com",
      database: "tornillo",
      login: "admin",
      secretKind: "password",
      transport: "db_manager",
      protocol: "xml_rpc",
      uploadToDrive: false,
      masterPassword: "********",
      createdAt: isoAgo(60 * 24 * 10),
    });
    const pinos = this.makeInstance({
      name: "Colegio Los Pinos",
      url: "https://lospinos.nube.example.com",
      database: "lospinos",
      login: "backup@lospinos.edu",
      secretKind: "api_key",
      transport: "auto",
      protocol: "auto",
      uploadToDrive: false,
      masterPassword: null,
      createdAt: isoAgo(60 * 3),
    });
    andina.lastProbe = this.buildProbe(andina.url, andina.database, andina.protocol, andina.secretKind, true, isoAgo(180));
    sanRafael.lastProbe = this.buildProbe(sanRafael.url, sanRafael.database, sanRafael.protocol, sanRafael.secretKind, true, isoAgo(60 * 26));
    tornillo.lastProbe = this.buildProbe(tornillo.url, tornillo.database, tornillo.protocol, tornillo.secretKind, true, isoAgo(60 * 5));
    this.instances = [andina, sanRafael, tornillo, pinos];

    const entries: HistoryEntry[] = [];
    const add = (inst: StoredInstance, minutesAgo: number, durationSecs: number, patch: Partial<HistoryEntry>) => {
      const started = new Date(Date.now() - minutesAgo * 60_000);
      const finished = new Date(started.getTime() + durationSecs * 1000);
      const base: HistoryEntry = {
        id: uuid(),
        instanceId: inst.id,
        instanceName: inst.name,
        status: "success",
        transport: inst.lastProbe?.recommendedTransport ?? "db_manager",
        startedAt: started.toISOString(),
        finishedAt: finished.toISOString(),
        filePath: `${this.settings.downloadDir}/${slug(inst.name)}/${inst.database}_${stamp(started)}.zip`,
        sizeBytes: Math.round((180 + Math.random() * 900) * MB),
        sha256: fakeSha256(`${inst.id}${minutesAgo}`),
        odooVersion: inst.lastProbe ? `${inst.lastProbe.version.major}.${inst.lastProbe.version.minor}` : null,
        errorCode: null,
        errorMessage: null,
        drive: inst.uploadToDrive
          ? { status: "success", fileId: `1${fakeSha256(inst.id).slice(0, 27)}`, errorMessage: null }
          : { status: "skipped", fileId: null, errorMessage: null },
      };
      entries.push({ ...base, ...patch });
    };
    add(andina, 125, 412, {});
    add(sanRafael, 60 * 26, 238, {});
    add(tornillo, 60 * 4, 31, {
      status: "failed",
      filePath: null,
      sizeBytes: null,
      sha256: null,
      errorCode: "db_manager_disabled",
      errorMessage: "database management is disabled on the server (list_db = False)",
      drive: { status: "skipped", fileId: null, errorMessage: null },
    });
    add(andina, 60 * 24 + 125, 398, {
      drive: { status: "failed", fileId: null, errorMessage: "rate limited" },
    });
    add(sanRafael, 60 * 24 * 2 + 30, 251, {});
    add(andina, 60 * 24 * 2 + 125, 17, {
      status: "cancelled",
      filePath: null,
      sizeBytes: null,
      sha256: null,
      drive: { status: "skipped", fileId: null, errorMessage: null },
    });
    this.history = entries.sort((a, b) => b.startedAt.localeCompare(a.startedAt));
  }

  private makeInstance(data: {
    name: string;
    url: string;
    database: string;
    login: string;
    secretKind: InstanceView["secretKind"];
    transport: InstanceView["transport"];
    protocol: InstanceView["protocol"];
    uploadToDrive: boolean;
    masterPassword: string | null;
    createdAt: string;
  }): StoredInstance {
    return {
      id: uuid(),
      name: data.name,
      url: data.url,
      database: data.database,
      login: data.login,
      secretKind: data.secretKind,
      hasSecret: true,
      hasMasterPassword: data.masterPassword !== null,
      transport: data.transport,
      protocol: data.protocol,
      includeFilestore: true,
      uploadToDrive: data.uploadToDrive,
      lastProbe: null,
      lastBackup: null,
      createdAt: data.createdAt,
      updatedAt: data.createdAt,
      secret: "********",
      masterPassword: data.masterPassword,
    };
  }

  private buildProbe(
    url: string,
    database: string | undefined,
    protocolPref: ProbeRequest["protocol"],
    secretKind: ProbeRequest["secretKind"],
    hasSecret: boolean,
    checkedAt: string = new Date().toISOString(),
    hasMasterPassword = true,
  ): ProbeReport {
    const host = hostOf(url);
    const https = url.startsWith("https://");
    const label = host.split(".")[0] ?? "";
    const major = host.includes("sanrafael") ? 19 : host.includes("tornillo") ? 15 : host.includes("andina") ? 17 : host.includes("legacy") ? 14 : 18;
    const version = versionFor(major);
    const supported = major >= 15 && major <= 19;
    const warnings: ProbeWarning[] = [];
    if (!https) warnings.push("insecure_http");
    if (!supported) warnings.push("unsupported_version");
    const resolvedDb = database && database.length > 0 ? database : label || null;
    if (!database && resolvedDb) warnings.push("database_derived_from_host");

    let protocol: ProbeReport["protocol"] =
      protocolPref === "auto" ? (major >= 19 && secretKind === "api_key" ? "json2" : "xml_rpc") : protocolPref;
    if (protocol === "json2" && (major < 19 || secretKind !== "api_key")) protocol = null;
    if (protocol === "xml_rpc" && major >= 19) warnings.push("deprecated_xml_rpc");

    const auth: ProbeReport["auth"] = !hasSecret
      ? { status: "skipped", reason: "sin credenciales" }
      : protocol === null
        ? { status: "failed", code: "unsupported_protocol", message: "JSON-2 requires Odoo >= 19 and an API key" }
        : host.includes("badauth")
          ? { status: "failed", code: "authentication_failed", message: "authentication failed" }
          : { status: "ok" };

    const moduleInstalled = host.includes("sanrafael");
    const module: ProbeReport["module"] =
      auth.status !== "ok"
        ? { status: "skipped", reason: "requiere autenticación" }
        : moduleInstalled
          ? { status: "ok" }
          : { status: "failed", code: "module_not_installed", message: "model obd.backup.api does not exist" };

    const listDbDisabled = host.includes("tornillo") || host.includes("sanrafael");
    const dbManager: ProbeReport["dbManager"] = listDbDisabled
      ? { status: "failed", code: "db_manager_disabled", message: "list_db = False" }
      : { status: "ok" };

    let recommendedTransport: TransportKind | null = null;
    if (module.status === "ok") recommendedTransport = "obd_module";
    else if (dbManager.status === "ok" && hasMasterPassword) recommendedTransport = "db_manager";
    if (recommendedTransport === "db_manager") warnings.push("master_password_over_wire");

    return {
      baseUrl: url.replace(/\/+$/, ""),
      https,
      version,
      supported,
      database: resolvedDb,
      protocol,
      uid: auth.status === "ok" ? 7 : null,
      auth,
      module,
      moduleApiVersion: module.status === "ok" ? 1 : null,
      dbManager,
      recommendedTransport,
      warnings,
      checkedAt,
    };
  }

  // --- Estado y bóveda ----------------------------------------------------

  async getAppStatus(): Promise<AppStatus> {
    await this.delay(120);
    return this.status();
  }

  async createVault(args: { useKeychain: boolean; masterPassword?: string }): Promise<AppStatus> {
    await this.delay(700);
    if (this.vault.exists) throw new AppError("vault_exists", "vault already exists");
    if (args.useKeychain && !this.vault.keychainAvailable) throw new AppError("keychain_unavailable", "no secret service");
    if (!args.useKeychain && !args.masterPassword) {
      throw new AppError("vault_invalid_options", "keychain or master password required");
    }
    this.vault = {
      ...this.vault,
      exists: true,
      unlocked: true,
      keychainEnabled: args.useKeychain,
      passwordEnabled: Boolean(args.masterPassword),
    };
    return this.status();
  }

  async unlockVault(args: { masterPassword?: string }): Promise<AppStatus> {
    await this.delay(args.masterPassword ? 900 : 400);
    if (!this.vault.exists) throw new AppError("vault_not_found", "vault not found");
    if (args.masterPassword === undefined) {
      if (!this.vault.keychainAvailable) throw new AppError("keychain_unavailable", "no secret service");
      if (!this.vault.keychainEnabled) throw new AppError("keychain_key_missing", "no key in keychain");
    } else {
      if (!this.vault.passwordEnabled) throw new AppError("vault_password_not_enabled", "no password");
      if (args.masterPassword !== MOCK_PASSWORD) throw new AppError("vault_wrong_password", "wrong password");
    }
    this.vault.unlocked = true;
    return this.status();
  }

  async lockVault(): Promise<AppStatus> {
    await this.delay(80);
    this.vault.unlocked = false;
    for (const handler of this.lockHandlers) handler({ reason: "manual" });
    return this.status();
  }

  async setMasterPassword(args: { newPassword?: string }): Promise<AppStatus> {
    this.requireUnlocked();
    await this.delay(800);
    if (!args.newPassword) {
      if (!this.vault.keychainEnabled) {
        throw new AppError("vault_invalid_options", "cannot remove the password without keychain");
      }
      this.vault.passwordEnabled = false;
    } else {
      this.vault.passwordEnabled = true;
    }
    return this.status();
  }

  async setKeychainEnabled(args: { enabled: boolean }): Promise<AppStatus> {
    this.requireUnlocked();
    await this.delay(300);
    if (args.enabled && !this.vault.keychainAvailable) throw new AppError("keychain_unavailable", "no secret service");
    if (!args.enabled && !this.vault.passwordEnabled) {
      throw new AppError("vault_invalid_options", "cannot disable keychain without password");
    }
    this.vault.keychainEnabled = args.enabled;
    return this.status();
  }

  async onVaultLocked(handler: (payload: VaultLockedPayload) => void): Promise<() => void> {
    this.lockHandlers.add(handler);
    return () => {
      this.lockHandlers.delete(handler);
    };
  }

  // --- Instancias ---------------------------------------------------------

  async listInstances(): Promise<InstanceView[]> {
    this.requireUnlocked();
    await this.delay(150);
    return this.instances.map((i) => this.view(i));
  }

  async saveInstance(input: InstanceInput): Promise<InstanceView> {
    this.requireUnlocked();
    await this.delay(350);
    const now = new Date().toISOString();
    if (input.id) {
      const current = this.instances.find((i) => i.id === input.id);
      if (!current) throw new AppError("not_found", "instance not found");
      Object.assign(current, {
        name: input.name,
        url: input.url,
        database: input.database,
        login: input.login,
        secretKind: input.secretKind,
        transport: input.transport,
        protocol: input.protocol,
        includeFilestore: input.includeFilestore,
        uploadToDrive: input.uploadToDrive,
        updatedAt: now,
      });
      if (input.secret !== undefined) current.secret = input.secret;
      if (input.masterPassword !== undefined) current.masterPassword = input.masterPassword;
      return this.view(current);
    }
    if (!input.secret) throw new AppError("missing_secret", "secret required");
    const created: StoredInstance = {
      id: uuid(),
      name: input.name,
      url: input.url,
      database: input.database,
      login: input.login,
      secretKind: input.secretKind,
      hasSecret: true,
      hasMasterPassword: Boolean(input.masterPassword),
      transport: input.transport,
      protocol: input.protocol,
      includeFilestore: input.includeFilestore,
      uploadToDrive: input.uploadToDrive,
      lastProbe: null,
      lastBackup: null,
      createdAt: now,
      updatedAt: now,
      secret: input.secret,
      masterPassword: input.masterPassword ?? null,
    };
    this.instances.push(created);
    return this.view(created);
  }

  async deleteInstance(id: string): Promise<void> {
    this.requireUnlocked();
    await this.delay(250);
    const index = this.instances.findIndex((i) => i.id === id);
    if (index < 0) throw new AppError("not_found", "instance not found");
    this.instances.splice(index, 1);
  }

  async probeInstance(input: ProbeRequest): Promise<ProbeReport> {
    this.requireUnlocked();
    await this.delay(1400);
    if (!/^https?:\/\/[^/\s]+/.test(input.url)) throw new AppError("invalid_url", `invalid url: ${input.url}`);
    const host = hostOf(input.url);
    if (host.includes("offline") || host.endsWith(".invalid")) {
      throw new AppError("connection", `could not connect to ${host}`);
    }
    const stored = input.instanceId ? this.instances.find((i) => i.id === input.instanceId) : undefined;
    const hasSecret = Boolean(input.secret) || Boolean(stored?.secret);
    const hasMaster = Boolean(input.masterPassword) || Boolean(stored?.masterPassword);
    const report = this.buildProbe(
      input.url,
      input.database,
      input.protocol,
      input.secretKind,
      hasSecret,
      new Date().toISOString(),
      hasMaster,
    );
    if (stored) stored.lastProbe = report;
    return clone(report);
  }

  // --- Backups ------------------------------------------------------------

  async startBackup(instanceId: string, onEvent: (event: BackupEvent) => void): Promise<string> {
    this.requireUnlocked();
    const instance = this.instances.find((i) => i.id === instanceId);
    if (!instance) throw new AppError("not_found", "instance not found");
    if ([...this.jobs.values()].some((j) => j.instanceId === instanceId)) {
      throw new AppError("backup_in_progress", "a backup is already running for this instance");
    }
    await this.delay(80);

    const jobId = uuid();
    const transport: TransportKind =
      instance.transport === "auto" ? (instance.lastProbe?.recommendedTransport ?? "db_manager") : instance.transport;
    const startedAt = new Date().toISOString();
    const job: SimulatedJob = {
      jobId,
      instanceId,
      historyId: uuid(),
      stage: "requesting",
      startedAt,
      timers: [],
      emit: onEvent,
    };
    this.jobs.set(jobId, job);

    const failing = instance.transport === "db_manager" && hostOf(instance.url).includes("tornillo");
    const total = Math.round((240 + Math.random() * 500) * MB);
    const knownTotal = transport === "obd_module";
    const uploads = instance.uploadToDrive && this.drive.connected;

    type Step = { at: number; run: () => void };
    const steps: Step[] = [];
    let t = 0;
    const push = (gap: number, run: () => void) => {
      t += gap;
      steps.push({ at: t, run });
    };
    const progress = (stage: BackupStage, extra: Partial<Extract<BackupEvent, { type: "progress" }>> = {}) => {
      job.stage = stage;
      if (extra.received !== undefined) job.received = extra.received;
      if (extra.total !== undefined) job.total = extra.total;
      if (extra.sent !== undefined) job.sent = extra.sent;
      onEvent({ type: "progress", jobId, stage, ...extra });
    };

    push(0, () => onEvent({ type: "started", jobId, instanceId, transport }));
    push(300, () => progress("requesting"));
    const prepareSecs = failing ? 3 : 5;
    for (let s = 0; s <= prepareSecs; s += 1) {
      push(s === 0 ? 400 : 1000, () => progress("server_preparing", { elapsedSecs: s }));
    }
    if (failing) {
      push(600, () =>
        this.finishJob(job, instance, "failed", {
          errorCode: "db_manager_disabled",
          errorMessage: "database management is disabled on the server (list_db = False)",
        }),
      );
    } else {
      const chunks = 12;
      for (let c = 1; c <= chunks; c += 1) {
        push(350, () => progress("downloading", { received: Math.round((total * c) / chunks), total: knownTotal ? total : null }));
      }
      push(400, () => progress("validating"));
      if (uploads) {
        const parts = 10;
        for (let p = 0; p <= parts; p += 1) {
          push(p === 0 ? 900 : 300, () => progress("uploading", { sent: Math.round((total * p) / parts), total }));
        }
      }
      push(600, () => progress("retention"));
      push(500, () =>
        this.finishJob(job, instance, "success", {
          sizeBytes: total,
          transport,
          drive: uploads
            ? { status: "success", fileId: `1${fakeSha256(jobId).slice(0, 27)}`, errorMessage: null }
            : { status: "skipped", fileId: null, errorMessage: null },
        }),
      );
    }

    for (const step of steps) {
      job.timers.push(setTimeout(step.run, step.at * this.speed));
    }
    return jobId;
  }

  private finishJob(
    job: SimulatedJob,
    instance: StoredInstance,
    status: "success" | "failed" | "cancelled",
    patch: Partial<HistoryEntry>,
  ): void {
    for (const timer of job.timers) clearTimeout(timer);
    this.jobs.delete(job.jobId);
    const started = new Date(job.startedAt);
    const entry: HistoryEntry = {
      id: job.historyId,
      instanceId: instance.id,
      instanceName: instance.name,
      status,
      transport: patch.transport ?? null,
      startedAt: job.startedAt,
      finishedAt: new Date().toISOString(),
      filePath: status === "success" ? `${this.settings.downloadDir}/${slug(instance.name)}/${instance.database}_${stamp(started)}.zip` : null,
      sizeBytes: null,
      sha256: status === "success" ? fakeSha256(job.jobId) : null,
      odooVersion: instance.lastProbe ? `${instance.lastProbe.version.major}.${instance.lastProbe.version.minor}` : null,
      errorCode: null,
      errorMessage: null,
      drive: { status: "skipped", fileId: null, errorMessage: null },
      ...patch,
    };
    this.history.unshift(entry);
    if (status === "success") job.emit({ type: "completed", jobId: job.jobId, entry: clone(entry) });
    else if (status === "failed") {
      job.emit({ type: "failed", jobId: job.jobId, code: entry.errorCode ?? "internal", message: entry.errorMessage ?? "" });
    } else job.emit({ type: "cancelled", jobId: job.jobId });
  }

  async cancelBackup(jobId: string): Promise<void> {
    await this.delay(100);
    const job = this.jobs.get(jobId);
    if (!job) throw new AppError("job_not_found", "job not found");
    const instance = this.instances.find((i) => i.id === job.instanceId);
    if (!instance) return;
    this.finishJob(job, instance, "cancelled", { errorCode: "cancelled", errorMessage: "operation cancelled" });
  }

  async listActiveJobs(): Promise<ActiveJob[]> {
    await this.delay(60);
    return [...this.jobs.values()].map((j) => ({
      jobId: j.jobId,
      instanceId: j.instanceId,
      stage: j.stage,
      startedAt: j.startedAt,
      received: j.received,
      total: j.total,
      sent: j.sent,
    }));
  }

  async listHistory(args?: { instanceId?: string; limit?: number }): Promise<HistoryEntry[]> {
    this.requireUnlocked();
    await this.delay(150);
    const filtered = args?.instanceId ? this.history.filter((h) => h.instanceId === args.instanceId) : this.history;
    return clone(filtered.slice(0, args?.limit ?? 200));
  }

  async revealBackup(historyId: string): Promise<void> {
    await this.delay(100);
    const entry = this.history.find((h) => h.id === historyId);
    if (!entry?.filePath) throw new AppError("not_found", "backup file not found");
    console.info("[mock] reveal", entry.filePath);
  }

  // --- Ajustes y Drive ----------------------------------------------------

  async getSettings(): Promise<Settings> {
    this.requireUnlocked();
    await this.delay(100);
    return clone(this.settings);
  }

  async updateSettings(settings: Settings): Promise<Settings> {
    this.requireUnlocked();
    await this.delay(250);
    if (!settings.downloadDir.trim()) throw new AppError("download_dir_invalid", "download dir required");
    this.settings = clone(settings);
    return clone(this.settings);
  }

  async getDriveStatus(): Promise<DriveStatus> {
    this.requireUnlocked();
    await this.delay(100);
    return { ...this.drive };
  }

  async setDriveClient(args: { clientId: string; clientSecret?: string | null }): Promise<DriveStatus> {
    this.requireUnlocked();
    await this.delay(250);
    const clientId = args.clientId.trim();
    const changed = clientId !== this.drive.clientId;
    this.drive = {
      ...this.drive,
      clientId: clientId || null,
      configured: clientId.length > 0,
      hasClientSecret: args.clientSecret === undefined ? this.drive.hasClientSecret : Boolean(args.clientSecret),
      ...(changed ? { connected: false, email: null, displayName: null } : {}),
    };
    return { ...this.drive };
  }

  connectDrive(): Promise<DriveStatus> {
    this.requireUnlocked();
    if (!this.drive.configured) return Promise.reject(new AppError("drive_not_configured", "no client id"));
    if (this.driveConnect) return Promise.reject(new AppError("drive_connect_in_progress", "already connecting"));
    return new Promise<DriveStatus>((resolve, reject) => {
      const timer = setTimeout(() => {
        this.driveConnect = null;
        this.drive = { ...this.drive, connected: true, email: "backups@example.com", displayName: "Backups Odoo" };
        resolve({ ...this.drive });
      }, 4000 * this.speed);
      this.driveConnect = { timer, reject };
    });
  }

  async cancelDriveConnect(): Promise<void> {
    if (!this.driveConnect) return;
    clearTimeout(this.driveConnect.timer);
    this.driveConnect.reject(new AppError("cancelled", "authorization cancelled"));
    this.driveConnect = null;
  }

  async disconnectDrive(): Promise<DriveStatus> {
    this.requireUnlocked();
    await this.delay(300);
    this.drive = { ...this.drive, connected: false, email: null, displayName: null };
    return { ...this.drive };
  }

  async pickDirectory(): Promise<string | null> {
    await this.delay(200);
    return "/home/usuario/Documentos/Backups Odoo";
  }
}

export function createMockBackend(options?: MockOptions): Backend {
  return new MockBackend({ ...parseMockOptions(), ...options });
}
