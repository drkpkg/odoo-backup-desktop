---
name: gdrive-storage
description: Use when working on crates/obd-storage or backup destinations in Odoo Backup Desktop — the StorageAdapter trait, local folder adapter, retention rules, Google Drive OAuth (loopback + PKCE), Drive resumable uploads, folder tagging, shared drives, OAuth client/verification setup, or adding a new destination (S3, SFTP, OneDrive, Dropbox, WebDAV).
---

# Storage adapters and Google Drive

## Adapter contract (`crates/obd-storage/src/adapter.rs`)

`StorageAdapter`: `provider_id`, `capabilities`, `ensure_target(TargetSpec) → TargetId`,
`upload(target, file, meta, progress, cancel) → RemoteObject` (streaming, never whole file in RAM),
`list_backups(target)` (newest first, only files created by Odoo Backup Desktop), `delete(object)`.

- Retention lives in `retention.rs` (`select_expired` + `apply_retention`), **not** in adapters.
  Rules: `keep_last`, `max_age_days`; the newest backup is never deleted.
- Errors map to `StorageError` (`AuthExpired`, `QuotaExceeded`, `RateLimited{retry_after}`,
  `NotFound`, `Transient`, `Fatal`, `Timeout`, `Cancelled`); retry only `is_retryable()` with
  exponential backoff + jitter. UI translates `.code()`.
- Local layout: `<download_dir>/<slug(instance_name)>/<db>_<YYYY-MM-DD_HH-MM-SS>.zip`.

## Google OAuth for a desktop app

- Flow: **system browser** + loopback redirect `http://127.0.0.1:<random port>` + PKCE S256 + `state`.
  Google blocks embedded webviews (`disallowed_useragent`) → never open consent inside Tauri.
  OOB copy/paste flow is dead (blocked for all clients since 2023-01-31). Desktop client type keeps loopback.
  https://developers.google.com/identity/protocols/oauth2/native-app
- Auth URL params: `response_type=code`, `client_id`, `redirect_uri`, `scope`, `code_challenge`,
  `code_challenge_method=S256`, `state`, `access_type=offline`, `prompt=consent` (to get a refresh token).
- Client secret: for installed apps Google says it "is obviously not treated as a secret", but the
  token endpoint still demands it for Desktop clients → send it when configured. Store the refresh
  token only in the vault.
- Refresh token failures (`invalid_grant`) → `AuthExpired` → UI asks to reconnect. Tokens die after
  6 months unused; max 100 refresh tokens per account per client (oldest silently revoked).
- Since June 2025 client secrets are only visible at creation; clients unused for 6 months are deleted.

## Scopes and verification

- Use **only** `https://www.googleapis.com/auth/drive.file` (non-sensitive): no CASA, no 100-user
  cap, no unverified-app screen. `drive`/`drive.readonly` are **restricted** (≈6 weeks review +
  yearly CASA) — don't request them.
- With `drive.file` the app sees only files/folders it created (enough for list + retention).
- Publishing status **Testing** → refresh tokens expire after **7 days** and 100 test users max.
  Set the Google Cloud project to **In production** (brand verification only needed for name/logo).
- No company OAuth client exists yet: the app lets the user configure `clientId`/`clientSecret`
  (`set_drive_client`). Default client via build-time env is a later decision.

## Drive API rules

- Folder: `mimeType = application/vnd.google-apps.folder`, tagged with
  `appProperties: {obdBackup: "root"}` (root) and `{obdBackup: "instance", instanceId}`.
  Find with `q = "appProperties has { key='obdBackup' and value='root' } and trashed = false"`.
- Files: `appProperties: {obdBackup: "file", instanceId, sha256}`.
- **Resumable upload** (`uploadType=resumable`) for everything > 5 MB:
  1. `POST {upload_api}files?uploadType=resumable` with JSON metadata, `X-Upload-Content-Type`,
     `X-Upload-Content-Length` → session URI in `Location`.
  2. `PUT` chunks with `Content-Range: bytes start-end/total`; chunk size multiple of **256 KiB**
     (default 8 MiB) except the last; `308 Resume Incomplete` + `Range` header = bytes stored.
  3. On 5xx/429/network error: `PUT` empty body with `Content-Range: bytes */total` → resume from
     the returned `Range`. `404`/`410` = session expired (sessions live ~1 week) → restart.
  4. Final `200/201` returns the file; compare `md5Checksum` with the local MD5 computed while sending.
- Shared drives: `supportsAllDrives=true` on every call; listing also `includeItemsFromAllDrives=true`,
  `corpora=drive`, `driveId`. Permanent delete on a shared drive needs `organizer`; trashing needs `writer`.
- Delete: `files.delete` (permanent) or `PATCH trashed=true` (30-day trash) per `permanentDelete`.
- Limits: 5 TB/file, 750 GB uploaded per user per day; `files.list` costs quota → page with `pageToken`.
- Service accounts have **no storage quota** in My Drive (`storageQuotaExceeded`); only useful with
  Workspace shared drives.

## Why native and not a library

- `google-drive3` 7.0 is generated and awkward; `yup-oauth2` installed flow has no PKCE/state.
- Apache OpenDAL 0.59 `gdrive`/`dropbox`/`onedrive`/`webdav` buffer the whole file and have no
  resumable upload → unusable for multi-GB zips (OK later for **S3** and **SFTP**).
- rclone sidecar (MIT, ~70 backends) is an optional future "generic" adapter; its shared Google
  client stops working in 2026, extra binary to sign.

## Testing

Unit tests use `wiremock` with `GoogleEndpoints` overrides: token refresh, `invalid_grant`, folder
search/create, multi-chunk upload with 308/Range, resume after 503, expired session restart, md5
mismatch, 429 mapping, list/delete. The loopback redirect is tested by calling the local listener.
Real Google calls are *unverified* until an OAuth client is configured.
