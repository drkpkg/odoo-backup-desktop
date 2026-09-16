---
name: react-ui
description: Use when building or changing the Odoo Backup Desktop frontend in src/ — React 19 + TypeScript screens, typed IPC wrappers for Tauri commands, the browser mock backend, backup progress via Channels, error-code translation to Spanish, forms that handle credentials, styling with Tailwind v4.
---

# React UI conventions (Odoo Backup Desktop)

The UI is a thin client over the Rust commands documented in `docs/architecture.md`
("Contrato IPC"). It never talks to Odoo or Google directly and never stores secrets.

## Stack

React 19, TypeScript (strict), Vite, Tailwind CSS v4 (`@tailwindcss/vite`), TanStack Query for
command data, `@tauri-apps/api` (`invoke`, `Channel`, `listen`), `@tauri-apps/plugin-dialog`
(folder picker). Tests with Vitest. Package manager: **pnpm**.

## Language

- UI text in **Spanish** (users are LatAm admins). Code, identifiers, comments in **English**.
- Keep user-facing strings out of logic modules; error texts live in `src/lib/errors.ts`.

## IPC layer

| File | Rule |
|---|---|
| `src/lib/types.ts` | TS types mirroring the contract exactly (camelCase fields, snake_case enum values such as `"api_key"`, `"db_manager"`, `"xml_rpc"`) |
| `src/lib/ipc.ts` | one typed function per command (`listInstances()`, `startBackup(instanceId, onEvent)`); components never call `invoke` directly |
| `src/lib/errors.ts` | `CommandError = { code, message }` → Spanish text by `code`; unknown codes fall back to a generic message + technical detail |
| `src/lib/mock-backend.ts` | used when `window.__TAURI_INTERNALS__` is absent so `pnpm dev` works in a plain browser for visual checks; must implement the same types |

When a command changes in Rust, update `types.ts`, `ipc.ts`, the mock and the docs together.

## Data flow

- Queries: `useQuery({ queryKey: ["instances"], queryFn: ipc.listInstances })`; mutations invalidate
  the affected keys (`instances`, `history`, `settings`, `drive`, `app-status`).
- On `vault-locked` (global event via `listen`) clear the query cache and show the unlock screen.
- Backup progress: `startBackup` receives a `Channel<BackupEvent>`; keep per-job progress in a small
  store/context keyed by `jobId`; invalidate `history` + `instances` on `completed`/`failed`/`cancelled`.
  `server_preparing` has no percentage (show elapsed time); `downloading` may have `total: null`
  (show bytes received) — use a percentage only when `total` is known.

## Credentials in forms

- Secret inputs (`password`/API key/master password) are write-only: when editing, show "guardado"
  (`hasSecret`) and send `secret` only if the user typed a new one; `masterPassword: null` removes it.
- Clear secret fields from component state right after a successful submit or probe; never put them
  in query cache, URLs, `localStorage`, logs or error reports.
- `type="password"` with `autoComplete="off"`/`new-password`; allow reveal toggle only locally.

## Screens

Unlock/create vault · Instances (list + form with "Probar conexión" showing `ProbeReport`
checks and warnings) · Backup progress (cancel) · History (open folder via `reveal_backup`) ·
Settings (download folder, retention, concurrency, timeouts, auto-lock, Google Drive client +
connect/disconnect, vault password/keychain).

## Probe warnings → Spanish

`insecure_http` "La URL usa http: las credenciales viajan sin cifrar" ·
`unsupported_version` "Versión de Odoo no soportada (15.0–19.0)" ·
`deprecated_xml_rpc` "XML-RPC está obsoleto en Odoo 19; usa una API key para JSON-2" ·
`database_derived_from_host` "La base de datos se dedujo del subdominio" ·
`master_password_over_wire` "El gestor de BD envía la contraseña maestra; si el servidor aún usa
'admin', Odoo la reemplazará".

## UI quality

- Accessible: labels on inputs, keyboard navigation, focus states, `aria-live` for progress.
- Show sizes with binary units (KiB/MiB/GiB) and dates in local time (`es` locale).
- Destructive actions (delete instance, disconnect Drive) need confirmation in-app (no
  `window.confirm`, which blocks the webview).
- Commands: `pnpm dev` (browser + mock), `pnpm tauri dev`, `pnpm build` (tsc + vite), `pnpm test`.
