# Odoo Backup Desktop — notes for Claude Code

Desktop app (Tauri 2 + React/TS + Rust workspace) that backs up Odoo 15–19 instances.
Read `docs/architecture.md` first: it holds the IPC contract, vault format and security rules.

## Layout
- `crates/obd-vault` encrypted vault · `crates/obd-odoo` Odoo client + backup transports ·
  `crates/obd-storage` storage adapters (local, Google Drive) + retention ·
  `crates/obd-plugins` plugin manifest/schema validation, discovery, assets, plugin stores
- `src-tauri/` app state, commands (`commands.rs`, `plugin_commands.rs`), backup runner (`backup.rs`),
  plugin manager + `obd-plugin://` protocol (`plugins.rs`), history (SQLite)
- `src/` React UI; `src/lib/ipc.ts` + `types.ts` mirror the IPC contract; mock backend for `pnpm dev`
- `plugin-sdk/` JS/CSS/d.ts served to plugins at `/_sdk/`; `examples/plugins/`, `templates/plugin/`,
  `scripts/new-plugin.sh`. Plugin contract: `docs/plugins.md`.
- Project skills in `.claude/skills/` (tauri2-desktop, odoo-remote-backup, secure-vault, gdrive-storage,
  desktop-release, react-ui, plugins) — load the relevant one before working in that area.

## Rules
- Secrets never cross IPC or reach logs/URLs. Commands accept secrets as input and return metadata only.
- Keychain and Argon2id calls are blocking: always `spawn_blocking`.
- New commands must be added to `src-tauri/build.rs`, `src-tauri/permissions/app-commands.toml`,
  `generate_handler!` in `src-tauri/src/lib.rs`, the contract in `docs/architecture.md` (or
  `docs/plugins.md`) and `src/lib/ipc.ts`. Only add them to `allow-plugin-window-commands` if plugin
  windows really need them.
- Plugins never get Tauri IPC: their pages run in sandboxed iframes and talk through the bridge.
- Code, identifiers and comments in English; UI strings in Spanish. In Spanish copy say **respaldo /
  respaldar / respaldos** (never "backup"); the brand name "Odoo Backup Desktop" stays as is.
- Commits and PRs carry no AI attribution.

## Commands
- `cargo test --workspace` · `pnpm test` · `pnpm typecheck` · `pnpm build`
- Lint/format on this machine (rustup shims lack components): `/usr/bin/cargo-clippy clippy --workspace --all-targets -- -D warnings`
  (with `CARGO=/usr/bin/cargo`) and `/usr/bin/rustfmt --edition 2024 --config-path rustfmt.toml <files>`
- Odoo integration tests (Docker): `dev/odoo/run-integration.sh`
