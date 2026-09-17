# __PLUGIN_NAME__

Plugin de Odoo Backup Desktop (API v1). Contrato: `docs/plugins.md` del repositorio.

- `plugin.json`: manifiesto (páginas, menús, ventanas, ajustes, permisos).
- `settings.schema.json`: ajustes; la app genera el formulario en *Ajustes → Plugins*.
- `ui/`: páginas HTML que usan el SDK en `../../_sdk/obd-plugin.js`.

Para probarlo: *Plugins → Modo desarrollador → Cargar desde carpeta…*. Con el modo desarrollador
activo, la app recarga el plugin al guardar cambios.
