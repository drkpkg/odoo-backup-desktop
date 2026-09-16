---
name: odoo-remote-backup
description: Use when working on crates/appex-odoo or anything that talks to a remote Odoo 15.0–19.0 server — version detection, XML-RPC vs JSON-2 selection, authentication, the /web/database/backup (DbManager) transport, the appex_backup module transport, probe diagnostics, backup zip validation, or the Docker integration tests in dev/odoo.
---

# Remote Odoo backups (Odoo 15.0–19.0)

Facts below were verified against odoo/odoo source (branches 15.0–19.0 and master) on 2026-09-16
unless marked *unverified*. Module contract: `docs/appex-backup-module-api.md`.

## Version detection (no credentials)

1. `GET /web/version` → `{"version_info": [...], "version": "19.0"}`. Exists **only in ≥ 19**
   (route in the auto-installed `rpc` addon, also `/json/version`).
2. Fallback: XML-RPC `POST /xmlrpc/2/common` → `version()` → `server_version`, `server_version_info`.
   Also `/web/webclient/version_info` (JSON-RPC, `auth="none"`) exists 15–19.
3. Parse `saas~17.2` as major 17, minor 2. Supported range: 15..=19.

## Protocol selection (`select_protocol`)

| Server | Auto choice | Notes |
|---|---|---|
| 15–18 | XML-RPC | `execute_kw(db, uid, password_or_api_key, model, method, args, kwargs)` |
| 19 | JSON-2 if the secret is an API key, else XML-RPC | XML-RPC works but logs a deprecation error |
| 20–21 | JSON-2 | `db` RPC service removed in 20 |
| 22+ | JSON-2 only | `/xmlrpc`, `/xmlrpc/2`, `/jsonrpc` removed ("scheduled for removal in Odoo 22") |

JSON-2: `POST /json/2/<model>/<method>`, headers `Authorization: bearer <api key>`,
`X-Odoo-Database: <db>` (optional when dbfilter resolves one DB), JSON body `{ids, context, ...kwargs}`.
Errors are real HTTP statuses (401 bad key, 404 unknown model/method, 422, 403 access). The key must
have global scope (`res.users.apikeys._check_credentials(scope='rpc', ...)`). JSON-2 has **no**
database-management calls. `auth='bearer'` exists from 18.0.

XML-RPC: Odoo marshals `None` as `<nil/>` → the parser must accept it. Faults carry `faultCode`
(string) + `faultString`. API keys are accepted in place of passwords since Odoo 14.

## Database name / subdomains

Instances live in our cloud, one subdomain per DB (`dbfilter = ^%d$`). `%d` = first host label,
leading `www.` stripped. Always connect with the real hostname, never the IP (Host header drives
dbfilter). `database_from_host()` derives the default DB name.

## DbManager transport — `POST /web/database/backup`

- Form (`application/x-www-form-urlencoded`), `auth="none"`, `csrf=False`: `master_pwd`, `name`,
  `backup_format=zip`; **`filestore` (true/false) only on ≥ 19** (older versions ignore unknown
  args with a warning — don't send it there).
- **Blocked when `list_db = False`** on every version: 15–19 `dump_db` is decorated with
  `@check_db_management_enabled` (raises `AccessDenied`); master uses `verify_access()` →
  `verify_db_management_enabled()`. XML-RPC `db.dump` is gated the same way (and returns base64 in
  memory — never use it).
- On 15–19 errors come back as **HTTP 200 with the HTML manager page** (`Database backup error: ...`).
  Success = `Content-Type: application/octet-stream` and body starting with `PK\x03\x04`.
- The server runs pg_dump + copies/zips the filestore **before sending the first byte**; no
  `Content-Length`. Use a long read timeout (`serverPrepareTimeoutMinutes`) and show bytes received.
- **Side effect:** if the server master password is still the default `admin`, Odoo replaces it with
  the `master_pwd` you send (`insecure and master_pwd → change_admin_password`). Surface the
  `master_password_over_wire` warning; require https.
- `name` must be in `http.db_list()` (dbfilter applies). `.dump` format doesn't check pg_dump's exit
  code → truncated 200; only use `zip`.
- Server-side limits that kill big backups: `limit_time_real` (default 120 s, SIGKILL in prefork),
  nginx `proxy_read_timeout`, temp disk space. Report them as a diagnostic, the client can't fix them.
- `list_db` status: `/web/database/list` (JSON-RPC) fails with AccessDenied when disabled.

## AppexModule transport (works with `list_db = False`)

Module `appex_backup` (developed later) exposes `appex.backup.api`: `get_info`, `request_backup`,
`get_job`, `discard_job` + `GET /appex_backup/download/<job_id>` (bearer API key, `Range`).
Client flow: check `api_version == 1` → request → poll `get_job` → resumable download (≤5 retries,
backoff) → verify size + sha256 → validate zip → rename → discard. Needs an **API key**.
Server-side notes for the future module: `Stream`/vendored `send_file` supports Range from 16
(`make_conditional(accept_ranges=True)`); 15's `http.send_file` needs Range enabled manually;
`limit_time_real_cron` defaults to `limit_time_real`.

## Download + validation rules

- Write `<dest>/<stem>.zip.part`, hash SHA-256 while streaming, never buffer the file in memory.
- Validate (blocking → `spawn_blocking`): every entry CRC, `dump.sql` present and ending with
  `-- PostgreSQL database dump complete` (*footer is standard pg_dump behaviour, not Odoo code*),
  `manifest.json` (`odoo_dump`, `db_name`, `version`, `major_version`, `pg_version`, `modules`),
  `db_name` matches. `filestore/` entries are `xx/<sha1>`; `checklist/` markers may be empty.
- Rename `.part` → `.zip` only after validation; delete `.part` on error/cancel.
- Map every failure to `OdooError::code()` (UI translates codes to Spanish).

## Integration tests (Docker)

`dev/odoo/` holds a compose file + script that start Odoo 15/17/19 with `list_db = True` and a known
`admin_passwd`, create a DB with `base`, generate an API key (19) via `odoo shell`, and run the
`#[ignore]` tests. Use high ports (1801x) — 8000 and 5432–5434 are taken on the dev machine. Never
touch unrelated containers; `docker compose down -v` when done.

## Not supported

Odoo Online (only manual "Download Backup" in odoo.com/my/databases) and Odoo.sh (Backups tab;
SSH + pg_dump *unverified*). Instances with `list_db = False` need the module.
