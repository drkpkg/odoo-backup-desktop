# Odoo Backup Desktop — Arquitectura (desktop)

Aplicación de escritorio (Linux y Windows) para hacer backups `.zip` de instancias Odoo 15.0–19.0,
guardar credenciales cifradas y subir los backups a destinos externos (Google Drive primero).

> Alcance actual: **solo desktop** (backups manuales desde la UI). El daemon (`obd-daemon`,
> programación y detección de cambios por hash) y el módulo Odoo `obd_backup` se desarrollan
> más adelante. El cliente del módulo ya está implementado contra el contrato de
> [`obd-backup-module-api.md`](obd-backup-module-api.md).

## Stack

| Capa | Tecnología |
|---|---|
| Shell desktop | Tauri 2.11 (`src-tauri/`, crate `odoo-backup-desktop`) |
| UI | React 19 + TypeScript + Vite (`src/`) |
| Núcleo | Rust 2024, crates en `crates/` |
| Empaquetado | `.deb`, `.rpm`, AppImage, NSIS `.exe`, PKGBUILD (AUR) |

Identificador: `io.github.drkpkg.odoo-backup-desktop` · Producto: `Odoo Backup Desktop` · Binario: `odoo-backup-desktop`.
El prefijo `obd` (Odoo Backup Desktop) nombra los crates internos y el módulo Odoo `obd_backup`.

> "Odoo" es marca registrada de Odoo S.A. Este proyecto es independiente y no está afiliado a Odoo S.A.

## Estructura

```
crates/obd-vault     Bóveda cifrada (XChaCha20-Poly1305 + Argon2id + llavero del SO)
crates/obd-odoo      Detección de versión, XML-RPC / JSON-2, transportes de backup, validación del zip
crates/obd-storage   StorageAdapter (carpeta local, Google Drive), retención
src-tauri/             Estado de la app, comandos IPC, ejecución de backups, historial (SQLite)
src/                   UI React (sin secretos: solo metadatos)
docs/                  Arquitectura y contratos
packaging/             PKGBUILD (AUR), scripts de AppImage
.github/workflows/     CI y releases
dev/odoo/              Docker compose para pruebas de integración (Odoo 15–19)
```

## Reglas de seguridad (no negociables)

1. **Los secretos nunca llegan al webview.** Los comandos aceptan secretos como entrada
   (solo escritura) y devuelven únicamente metadatos (`hasSecret: true`).
2. En Rust, los secretos viven en `secrecy::SecretString` o `obd_vault::SecretField`
   (Debug redactado, zeroize al soltar). Nunca se registran en logs ni se ponen en URLs.
3. Todo lo relativo a instancias (URL, BD, usuario, API key, contraseña maestra) y el refresh
   token de Google se guarda **dentro de la bóveda cifrada**. El historial en SQLite guarda solo
   `instance_id`, rutas y estados.
4. Las llamadas al llavero y Argon2id son bloqueantes → `tokio::task::spawn_blocking`.
5. CSP estricta, capabilities mínimas y lista explícita de comandos (`build.rs` → `AppManifest`).

## Bóveda (`obd-vault`)

- DEK aleatoria de 32 bytes. Formato del archivo `vault.bin` (versión 1):

```
magic "OBDVAULT" (8) | version u8 = 1 | flags u8 (bit0 keychain, bit1 password)
m_cost_kib u32 LE | t_cost u32 LE | parallelism u32 LE | salt [16]
dek_nonce [24] | wrapped_dek [48]   (ceros si no hay contraseña)
payload_nonce [24]
ciphertext (XChaCha20-Poly1305, AAD = todos los bytes anteriores)
```

- `wrapped_dek` = XChaCha20-Poly1305(key = Argon2id(password, salt), AAD = `"OBDVAULT-dek-v1"`).
- El llavero guarda la DEK en base64 (≈44 caracteres; cabe en el límite de 2560 bytes de Windows).
  Servicio `io.github.drkpkg.odoo-backup-desktop`, cuenta `vault-dek`.
- Escritura atómica (tmp + fsync + rename), permisos `0600` en Unix.
- Desbloqueo: intenta llavero → si no está o no coincide, pide contraseña maestra.

## Odoo (`obd-odoo`)

### Detección de versión y protocolo

1. `GET /web/version` → Odoo ≥ 19. Si falla, XML-RPC `common.version()`.
2. Protocolo (`select_protocol`): `Auto` → JSON-2 si `major ≥ 19` y el secreto es API key;
   si no, XML-RPC. XML-RPC acepta contraseña o API key; JSON-2 solo API key.
3. Base de datos: si no se indica, se deriva del subdominio (`dbfilter = ^%d$`).

### Transportes de backup

| Transporte | Requisitos | Notas |
|---|---|---|
| `DbManager` | `list_db = True` + contraseña maestra | `POST /web/database/backup` (`master_pwd`, `name`, `backup_format=zip`, `filestore` solo en ≥19). Errores llegan como HTTP 200 con HTML → validar `Content-Type` y firma `PK\x03\x04`. Sin `Content-Length`. |
| `ObdModule` | módulo `obd_backup` instalado + API key | Funciona con `list_db = False`. Generación asíncrona, descarga reanudable con `Range`, `sha256` verificado. |

Selección `Auto` (en la app): módulo disponible → `ObdModule`; si no, `DbManager` si
`list_db` está activo y hay contraseña maestra; si no, error guiado.

### Validación del zip

CRC de todas las entradas, `dump.sql` presente y terminado en
`-- PostgreSQL database dump complete`, `manifest.json` válido y `db_name` esperado.

## Almacenamiento (`obd-storage`)

- `StorageAdapter`: `ensure_target`, `upload` (streaming, cancelable, progreso),
  `list_backups`, `delete`. Retención común (`keep_last`, `max_age_days`), nunca borra el más nuevo.
- Carpeta local: `<download_dir>/<slug-instancia>/<db>_<YYYY-MM-DD_HH-MM-SS>.zip`.
- Google Drive: OAuth loopback + PKCE, scope `drive.file`, subida reanudable (chunks de 8 MiB,
  múltiplos de 256 KiB), carpetas etiquetadas con `appProperties`, verificación `md5Checksum`.

## Flujo de un backup (app)

```
startBackup(instanceId, channel)
 ├─ probe cacheado (versión, protocolo, transporte)            → phase: requesting
 ├─ transporte.run() → .zip.part → sha256 → validación → .zip  → phase: server_preparing / downloading / validating
 ├─ retención local
 ├─ si uploadToDrive: ensure_target → upload → retención remota → phase: uploading / retention
 └─ historial (SQLite) + notificación del sistema               → completed | failed | cancelled
```

Concurrencia limitada por `maxConcurrentBackups` (semáforo). Cancelación con `CancellationToken`.

## Contrato IPC (Tauri commands)

Convenciones: nombres de comandos en `snake_case` en Rust; en TypeScript se invocan con el mismo
nombre (`invoke("list_instances")`). Los argumentos se pasan en camelCase (Tauri los convierte).
Las respuestas usan camelCase (`#[serde(rename_all = "camelCase")]`).

Errores: todo comando falla con `{ code: string, message: string }`. La UI traduce `code` a
español (`src/lib/errors.ts`); `message` es detalle técnico en inglés.

### Estado y bóveda

| Comando | Args | Retorno |
|---|---|---|
| `get_app_status` | — | `AppStatus` |
| `create_vault` | `{ useKeychain: boolean, masterPassword?: string }` | `AppStatus` |
| `unlock_vault` | `{ masterPassword?: string }` (sin contraseña → llavero) | `AppStatus` |
| `lock_vault` | — | `AppStatus` |
| `set_master_password` | `{ newPassword?: string }` (vacío = quitar, requiere llavero) | `AppStatus` |
| `set_keychain_enabled` | `{ enabled: boolean }` | `AppStatus` |

```ts
type AppStatus = {
  appVersion: string;
  vault: {
    exists: boolean; unlocked: boolean;
    keychainAvailable: boolean; keychainEnabled: boolean; passwordEnabled: boolean;
  };
};
```

Evento global: `vault-locked` (payload `{ reason: "manual" | "idle" }`) cuando la bóveda se bloquea.

### Instancias

| Comando | Args | Retorno |
|---|---|---|
| `list_instances` | — | `InstanceView[]` |
| `save_instance` | `{ input: InstanceInput }` | `InstanceView` |
| `delete_instance` | `{ id: string }` | `void` |
| `probe_instance` | `{ input: ProbeRequest }` | `ProbeReport` |

```ts
type SecretKind = "password" | "api_key";
type TransportPreference = "auto" | "db_manager" | "obd_module";
type ProtocolPreference = "auto" | "xml_rpc" | "json2";

type InstanceView = {
  id: string; name: string; url: string; database: string; login: string;
  secretKind: SecretKind; hasSecret: boolean; hasMasterPassword: boolean;
  transport: TransportPreference; protocol: ProtocolPreference;
  includeFilestore: boolean; uploadToDrive: boolean;
  lastProbe: ProbeReport | null;        // última prueba guardada
  lastBackup: HistoryEntry | null;
  createdAt: string; updatedAt: string; // ISO 8601
};

type InstanceInput = {
  id?: string;                          // ausente = crear
  name: string; url: string; database: string; login: string;
  secretKind: SecretKind;
  secret?: string;                      // ausente = conservar
  masterPassword?: string | null;       // ausente = conservar, null = borrar
  transport: TransportPreference; protocol: ProtocolPreference;
  includeFilestore: boolean; uploadToDrive: boolean;
};

type ProbeRequest = {
  instanceId?: string;                  // usa secretos guardados si los campos van vacíos
  url: string; database?: string; login?: string;
  secretKind: SecretKind; secret?: string; masterPassword?: string;
  protocol: ProtocolPreference;
};

type CheckStatus =
  | { status: "ok" }
  | { status: "failed"; code: string; message: string }
  | { status: "skipped"; reason: string };

type ProbeReport = {
  baseUrl: string; https: boolean;
  version: { major: number; minor: number; serverVersion: string; saas: boolean };
  supported: boolean; database: string | null;
  protocol: "xml_rpc" | "json2" | null; uid: number | null;
  auth: CheckStatus; module: CheckStatus; moduleApiVersion: number | null;
  dbManager: CheckStatus;
  recommendedTransport: "db_manager" | "obd_module" | null;
  warnings: ("insecure_http" | "unsupported_version" | "deprecated_xml_rpc"
    | "database_derived_from_host" | "master_password_over_wire")[];
  checkedAt: string;
};
```

### Backups e historial

| Comando | Args | Retorno |
|---|---|---|
| `start_backup` | `{ instanceId: string, onEvent: Channel<BackupEvent> }` | `string` (jobId) |
| `cancel_backup` | `{ jobId: string }` | `void` |
| `list_active_jobs` | — | `ActiveJob[]` |
| `list_history` | `{ instanceId?: string, limit?: number }` | `HistoryEntry[]` |
| `reveal_backup` | `{ historyId: string }` | `void` (abre el gestor de archivos) |

```ts
type BackupStage = "requesting" | "server_preparing" | "downloading" | "validating"
  | "uploading" | "retention";

type BackupEvent =
  | { type: "started"; jobId: string; instanceId: string; transport: "db_manager" | "obd_module" }
  | { type: "progress"; jobId: string; stage: BackupStage;
      elapsedSecs?: number; received?: number; total?: number | null; sent?: number }
  | { type: "completed"; jobId: string; entry: HistoryEntry }
  | { type: "failed"; jobId: string; code: string; message: string }
  | { type: "cancelled"; jobId: string };

type ActiveJob = { jobId: string; instanceId: string; stage: BackupStage; startedAt: string;
  received?: number; total?: number | null; sent?: number };

type HistoryEntry = {
  id: string; instanceId: string; instanceName: string;
  status: "running" | "success" | "failed" | "cancelled";
  transport: "db_manager" | "obd_module" | null;
  startedAt: string; finishedAt: string | null;
  filePath: string | null; sizeBytes: number | null; sha256: string | null;
  odooVersion: string | null;
  errorCode: string | null; errorMessage: string | null;
  drive: { status: "skipped" | "success" | "failed"; fileId: string | null; errorMessage: string | null };
};
```

### Ajustes y Google Drive

| Comando | Args | Retorno |
|---|---|---|
| `get_settings` | — | `Settings` |
| `update_settings` | `{ settings: Settings }` | `Settings` |
| `get_drive_status` | — | `DriveStatus` |
| `set_drive_client` | `{ clientId: string, clientSecret?: string \| null }` | `DriveStatus` |
| `connect_drive` | — (abre el navegador; espera hasta 5 min) | `DriveStatus` |
| `cancel_drive_connect` | — | `void` |
| `disconnect_drive` | — | `DriveStatus` |

```ts
type Settings = {
  downloadDir: string;
  keepLastLocal: number | null;
  maxConcurrentBackups: number;          // 1..4
  serverPrepareTimeoutMinutes: number;   // 5..720
  autoLockMinutes: number | null;
  drive: { rootFolderName: string; keepLast: number | null; permanentDelete: boolean; sharedDriveId: string | null };
};

type DriveStatus = {
  configured: boolean;                   // hay client id
  clientId: string | null; hasClientSecret: boolean;
  connected: boolean; email: string | null; displayName: string | null;
};
```

La carpeta de descarga se elige en la UI con `@tauri-apps/plugin-dialog` (`open({ directory: true })`)
y se guarda con `update_settings`.

### Códigos de error de la capa de la app

Además de los códigos de cada crate (`VaultError::code`, `OdooError::code`, `StorageError::code`):
`vault_locked`, `not_found`, `invalid_input`, `invalid_url`, `password_required`, `password_too_short`,
`duplicate_name`, `backup_in_progress`, `job_not_found`, `missing_master_password`, `api_key_required`,
`no_transport_available`, `download_dir_invalid`, `drive_not_configured`, `drive_not_connected`,
`browser_open_failed`, `file_missing`, `history_db`, `internal`. En el historial, `interrupted` marca
backups que quedaron a medias porque la app se cerró.

## Pruebas

| Suite | Comando | Qué cubre |
|---|---|---|
| Unitarias Rust | `cargo test --workspace` | Crates + capa de la app |
| Contrato IPC | `cargo test -p odoo-backup-desktop --test ipc` | Comandos reales vía runtime simulado de Tauri, `tauri.conf.json` y capabilities reales, secretos nunca expuestos |
| Odoo real (Docker) | `dev/odoo/run-integration.sh` | Cliente Odoo contra 15/17/19 (con y sin `list_db`) |
| App ↔ Odoo real | `IT_COMMAND='cargo test -p odoo-backup-desktop --test ipc -- --ignored' dev/odoo/run-integration.sh` | Backup completo por comandos IPC, retención local |
| Frontend | `pnpm test`, `pnpm typecheck`, `pnpm build` | Utilidades, mapeo de errores, formularios, mock |

## Archivos de la app

| Archivo | Ubicación (Linux / Windows) | Contenido |
|---|---|---|
| `vault.bin` | `~/.local/share/io.github.drkpkg.odoo-backup-desktop/` · `%APPDATA%\io.github.drkpkg.odoo-backup-desktop\` | Bóveda cifrada |
| `settings.json` | igual | Ajustes no secretos |
| `history.sqlite3` | igual | Historial de backups |
| `logs/` | `app_log_dir` | Logs rotados (sin secretos) |
