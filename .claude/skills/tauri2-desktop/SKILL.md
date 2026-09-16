---
name: tauri2-desktop
description: Use when adding or changing anything in src-tauri/ of Appex Backup — Tauri 2 commands, the command allowlist (build.rs AppManifest, permissions/app-commands.toml, capabilities), CSP, managed state, progress Channels, plugins (dialog, opener, notification, single-instance), blocking work (keychain, Argon2id), or Linux WebKitGTK rendering issues.
---

# Tauri 2 desktop shell (Appex Backup)

Versions in use (checked 2026-09-16): `tauri` 2.11.5, `tauri-build` 2.6.3, `@tauri-apps/cli` 2.11.x,
`tauri-plugin-dialog` 2.7, `-opener` 2.5, `-notification` 2.4, `-single-instance` 2.4.
**Do not** move to Tauri 3 (first alpha tagged 2026-09-15). **Do not** use `tauri-plugin-stronghold`
(maintainers: "will be deprecated and therefore removed in v3") nor community keyring plugins that
expose get/set to JavaScript.

Contract for every command and event: `docs/architecture.md` → "Contrato IPC".

## Layout

| Path | Role |
|---|---|
| `src-tauri/tauri.conf.json` | productName `Appex Backup`, identifier `lat.appex.backup`, `mainBinaryName` `appex-backup`, CSP, bundle targets |
| `src-tauri/build.rs` | `COMMANDS` list → `AppManifest::commands` (generates `allow-<cmd>` permissions) |
| `src-tauri/permissions/app-commands.toml` | permission set `allow-app-commands` listing every command |
| `src-tauri/capabilities/default.json` | grants `core:app/event/window:default`, `dialog:allow-open`, `allow-app-commands` to window `main` |
| `src-tauri/src/` | state, commands, backup runner, history (SQLite), settings |
| `crates/appex-*` | business logic; the Tauri crate only orchestrates |

## Adding a command (checklist)

1. Implement `#[tauri::command] async fn my_cmd(...) -> Result<T, CommandError>` in `src-tauri/src/commands/`.
   `CommandError` serializes as `{ code, message }`; map crate errors with their `.code()`.
2. Register it in `tauri::generate_handler![...]`.
3. Add the name to `COMMANDS` in `build.rs` **and** to `permissions/app-commands.toml`.
   A command missing from the allowlist fails at runtime with a permission error, not at compile time.
4. Add the TS type + wrapper in `src/lib/ipc.ts` / `src/lib/types.ts` and document it in `docs/architecture.md`.
5. Return structs with `#[serde(rename_all = "camelCase")]`; JS passes camelCase args (Tauri maps them to snake_case params).

## Security rules

- **Secrets never cross IPC outward.** Commands accept secrets (write-only) and return metadata
  (`hasSecret`, `hasMasterPassword`). Never return `SecretField`/`SecretString` contents, never emit
  them in events, never log them (`#[instrument(skip(...))]`, redacted `Debug`).
- The webview is untrusted: validate every argument in Rust (URLs, paths, ranges).
- Keep CSP strict (`default-src 'self'`, `connect-src ipc: http://ipc.localhost`, no remote scripts/CDNs).
  All network access (Odoo, Google) happens in Rust, never from JS.
- Grant plugin permissions narrowly (`dialog:allow-open` only). Opening URLs/folders is done from Rust
  with `tauri_plugin_opener::OpenerExt` so JS doesn't need `opener:*` permissions.
- `freezePrototype: true` is set; don't disable it.

## State and concurrency

- Managed state (`AppState` in `src-tauri/src/state.rs`) holds: `VaultManager` (unlocked vault +
  decrypted data behind a `std::sync::Mutex`, only touched inside `spawn_blocking`), settings
  (`RwLock`), `JobRegistry` (`CancellationToken` per job), `History` (SQLite), shared `reqwest::Client`
  (no global timeout: the DB manager answers only when the zip is ready).
- Commands that take `AppHandle` are generic over `R: Runtime` so `src-tauri/tests/ipc.rs` can run them
  on `tauri::test::mock_builder()` with the real `tauri.conf.json` and capabilities.
- **Blocking calls go through `tokio::task::spawn_blocking`:** OS keychain (D-Bus / Credential
  Manager), Argon2id (≈64 MiB, hundreds of ms), vault file I/O, zip validation, SQLite.
- Backups are limited by a semaphore (`maxConcurrentBackups`). Never hold a lock across an `.await`
  on network I/O.
- Auto-lock: drop decrypted data + DEK and emit `vault-locked` (`{ reason: "manual" | "idle" }`).

## Progress: Channels, not global events

Long operations take `on_event: tauri::ipc::Channel<BackupEvent>` (JS: `new Channel<BackupEvent>()`).
Channels are ordered and scoped to the caller. Throttle `progress` messages (≈4/s) to avoid flooding
the webview. Use global `app.emit` only for app-wide state (`vault-locked`).

## Plugins

| Plugin | Use |
|---|---|
| `dialog` | JS `open({ directory: true })` to choose the download folder |
| `opener` | Rust: open Google consent URL in the **system browser** (embedded webviews are blocked by Google, `disallowed_useragent`); reveal a backup in the file manager |
| `notification` | Rust: notify when a backup finishes/fails |
| `single-instance` | Must be registered **first**; prevents two processes writing the vault/history. Uses D-Bus on Linux |

Not used yet: `updater` (needs signing keys, see `desktop-release` skill), `autostart`/tray (belong to the future daemon phase).

## Paths

Use `app.path().app_data_dir()` (Linux `~/.local/share/lat.appex.backup`, Windows
`%APPDATA%\lat.appex.backup`) for `vault.bin`, `settings.json`, `history.sqlite3`;
`app.path().app_log_dir()` for logs. Create dirs with `0700` on Unix.

## Linux WebKitGTK quirks

- Blank/white window on NVIDIA or some Wayland setups: set `WEBKIT_DISABLE_DMABUF_RENDERER=1`
  only when needed (don't force it globally). Reference: https://v2.tauri.app/develop/debug/linux-graphics/
- AppImage bundles libwayland/libxkbcommon/libxcb, which break on Mesa 25+ Wayland (Ubuntu 24.04+,
  Arch). Tauri issues #15665 and #15976 are open → see `packaging/appimage/strip-wayland-libs.sh`.
- Build Linux bundles on **ubuntu-22.04** (oldest with WebKitGTK 4.1 → lowest glibc).
- Tray icons (future) need the AppIndicator extension on GNOME; left-click events don't fire on Linux.
- **Never enable zbus's `tokio` feature** (e.g. `zbus-secret-service-keyring-store/rt-tokio-*`). Cargo
  unifies it into every zbus user, and `notify-rust` then calls `zbus::block_on` inside a tokio worker
  → panic ("Cannot start a runtime from within a runtime") and no desktop notifications. Keep the
  keyring store on `rt-async-io-crypto-rust`. Check with
  `cargo tree -p appex-backup -e features -i zbus | grep 'zbus feature "tokio"'` (must print nothing).

## Dev commands

```bash
pnpm install
pnpm tauri dev                 # app with hot reload
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
pnpm tauri build               # bundles into target/release/bundle/ (workspace root target)
```
