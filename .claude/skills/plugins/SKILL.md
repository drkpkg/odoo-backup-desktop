---
name: plugins
description: Use when creating or changing Odoo Backup Desktop plugins or the plugin system itself — plugin.json manifests, settings schemas, plugin pages/menus/windows, the obd-plugin:// protocol, the postMessage bridge and plugin SDK (plugin-sdk/), developer mode, example/template plugins, crates/obd-plugins, src-tauri/src/plugins.rs or plugin_commands.rs, and the phase B roadmap (WASM backend, destinations, hooks).
---

# Plugin system (API v1)

Contract: `docs/plugins.md` (manifest rules, schema subset, bridge methods, commands, error codes).
Always keep that doc, `src/lib/types.ts`, `plugin-sdk/obd-plugin.d.ts` and the Rust views in sync.

## Where things live

| Piece | Path |
|---|---|
| Manifest/schema validation, discovery, assets, stores | `crates/obd-plugins` |
| Manager, `obd-plugin` protocol, dev watcher | `src-tauri/src/plugins.rs` |
| Commands | `src-tauri/src/plugin_commands.rs` (+ `build.rs`, `permissions/app-commands.toml`) |
| Plugin window capability | `src-tauri/capabilities/plugin-windows.json` (`plugin--*`, `allow-plugin-window-commands`) |
| UI: manager, frame + bridge, schema form, window shell | `src/features/plugins/` |
| SDK served at `/_sdk/` (embedded with `include_str!`) | `plugin-sdk/obd-plugin.{js,css,d.ts}` |
| Example / template / generator | `examples/plugins/hello-obd`, `templates/plugin`, `scripts/new-plugin.sh` |
| Tests | `crates/obd-plugins` unit tests, `src-tauri/tests/plugins.rs` (mock runtime + real capabilities), `src/features/plugins/*.test.ts`, `dev/e2e/run-plugins-e2e.sh` (real WebKitGTK via WebDriver) |

## Security model (keep it)

- Plugin pages run in `<iframe sandbox="allow-scripts allow-forms allow-popups">` → opaque origin
  (`null`). Tauri injects IPC + invoke key only into the **main frame**, so iframes cannot call commands.
  Never add `allow-same-origin`, never inject scripts into all frames, never pass `__TAURI_INTERNALS__`.
- The host decides `pluginId` from `event.source === iframe.contentWindow`; never trust ids in messages.
- Plugin windows load the app itself (label `plugin--<id>--<window>`) with a restricted capability;
  add commands to `allow-plugin-window-commands` only when the bridge needs them.
- Custom protocols count as "local" origins for Tauri ACL — the protection is the missing invoke key
  and the sandbox, verified by `dev/e2e` ("iframe has no Tauri IPC").
- Secret settings (`secret: true` / `format: password`) → vault `pluginSecrets[<id>]`, never in
  `plugin-settings.json`, never returned (only `secretsSet`).
- `resolve_asset` must reject `..`, hidden segments, backslashes, `:` and symlinks leaving the folder.
- Page CSP comes from `obd_plugins::page_csp` (`connect-src` = declared `permissions.network`).
- Module scripts in opaque-origin iframes need `Access-Control-Allow-Origin: *` on protocol responses.

## Writing a plugin

```bash
scripts/new-plugin.sh my-plugin "Mi plugin" ~/dev/my-plugin
# App → Plugins → Modo desarrollador → Cargar desde carpeta… (auto-reload on save)
```

- Pages import the SDK relatively: `import obd from "../../_sdk/obd-plugin.js"` (page lives at
  `<base>/<id>/ui/…`). Styles: `../../_sdk/obd-plugin.css` (`.obd-btn`, `.obd-card`, `.obd-table`…).
- No `localStorage` (opaque origin): use `obd.storage.get/set` (1 MiB per plugin).
- Params (e.g. `instanceId` from `instance_actions`) arrive in `await obd.context()`.
- Icons: kebab-case lucide names from the curated map in `src/features/plugins/icons.ts`, else `puzzle`.

## Adding a bridge method

1. Row in `docs/plugins.md` (method, params, result) and error codes.
2. `handleBridgeMessage` in `src/features/plugins/bridge.ts` + tests.
3. SDK function in `plugin-sdk/obd-plugin.js` + `.d.ts` + test.
4. If it needs a new command: Rust command, `build.rs`, permission sets, `ipc.ts`, mock backend,
   `tests/plugins.rs` (and the plugin-window ACL test if windows use it).

## Gotchas

- `cargo test`/`cargo build` write a **dev** binary (devUrl) to `target/debug`; the e2e runner builds
  the embedded-frontend binary into `target/e2e` with `pnpm tauri build --debug --no-bundle`.
- WebKitWebDriver frame switching: use `{"id": 0}` (element references fail).
- In scripts, don't `pkill -f odoo-backup-desktop`: it matches the calling shell.
- `serde_json` has no `preserve_order`: the UI must use `schema.propertyOrder`.

## Phase B (planned)

WASM backend (`backend.wasm`, wasmtime component model) implementing `destinations` (StorageAdapter)
and `hooks`; host-side streaming for large uploads; Google Drive migrated to a built-in plugin;
instances get a list of destinations instead of `uploadToDrive`. Manifest fields already parse and
raise `backend_not_supported` until then.
