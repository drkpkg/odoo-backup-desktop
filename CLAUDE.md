# Appex Backup — notes for Claude Code

Desktop app (Tauri 2 + React/TS + Rust workspace) that backs up Odoo 15–19 instances.
Read `docs/architecture.md` first: it holds the IPC contract, vault format and security rules.

## Layout
- `crates/appex-vault` encrypted vault · `crates/appex-odoo` Odoo client + backup transports ·
  `crates/appex-storage` storage adapters (local, Google Drive) + retention
- `src-tauri/` app state, commands (`commands.rs`), backup runner (`backup.rs`), history (SQLite)
- `src/` React UI; `src/lib/ipc.ts` + `types.ts` mirror the IPC contract; mock backend for `pnpm dev`
- Project skills in `.claude/skills/` (tauri2-desktop, odoo-remote-backup, secure-vault, gdrive-storage,
  desktop-release, react-ui) — load the relevant one before working in that area.

## Rules
- Secrets never cross IPC or reach logs/URLs. Commands accept secrets as input and return metadata only.
- Keychain and Argon2id calls are blocking: always `spawn_blocking`.
- New commands must be added to `src-tauri/build.rs`, `src-tauri/permissions/app-commands.toml`,
  `generate_handler!` in `src-tauri/src/lib.rs`, the contract in `docs/architecture.md` and `src/lib/ipc.ts`.
- Code, identifiers and comments in English; UI strings in Spanish.
- Commits and PRs carry no AI attribution.

## Commands
- `cargo test --workspace` · `pnpm test` · `pnpm typecheck` · `pnpm build`
- Lint/format on this machine (rustup shims lack components): `/usr/bin/cargo-clippy clippy --workspace --all-targets -- -D warnings`
  (with `CARGO=/usr/bin/cargo`) and `/usr/bin/rustfmt --edition 2024 --config-path rustfmt.toml <files>`
- Odoo integration tests (Docker): `dev/odoo/run-integration.sh`
