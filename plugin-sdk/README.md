# SDK de plugins (API v1)

La app sirve estos archivos en `/_sdk/` para todos los plugins; no hay que copiarlos ni instalar nada.

| Archivo | Uso |
|---|---|
| `obd-plugin.js` | Cliente del puente (ES module, sin dependencias) |
| `obd-plugin.d.ts` | Tipos para editores y TypeScript |
| `obd-plugin.css` | Estilos base con la paleta de la app (claro/oscuro) |

```html
<link rel="stylesheet" href="../../_sdk/obd-plugin.css" />
<script type="module">
  import obd from "../../_sdk/obd-plugin.js";

  const ctx = await obd.context(); // { pluginId, theme, surface, params, … }
  const instances = await obd.instances.list();
  await obd.storage.set("ultimaVisita", new Date().toISOString());
  await obd.ui.toast({ kind: "success", title: `Hola, ${instances.length} instancias` });
</script>
```

La ruta relativa `../../_sdk/` funciona para páginas en `ui/` (el plugin vive en `<base>/<id>/`).

## API

- `obd.context()`
- `obd.settings.get()`, `obd.settings.save(values)` (secretos: string = reemplazar, `null` = borrar)
- `obd.storage.get(key)`, `obd.storage.set(key, value)` (`null` borra; 1 MiB por plugin)
- `obd.instances.list()`, `obd.history.list({ instanceId?, limit? })` (solo lectura, sin secretos)
- `obd.ui.toast({ kind, title, description })`, `obd.ui.openWindow(id, params)`,
  `obd.ui.navigate(pageId, params)` (solo páginas), `obd.ui.openSettings()`
- `obd.on("theme" | "plugins-changed", callback)` → devuelve la función para dejar de escuchar

Los errores rechazan con `ObdError` (`error.code`, p. ej. `bridge_timeout` a los 30 s,
`plugin_storage_limit`, `unsupported_surface`). Las clases CSS útiles: `.obd-page`, `.obd-card`,
`.obd-row`, `.obd-grid`, `.obd-btn`, `.obd-btn-primary`, `.obd-input`, `.obd-table`, `.obd-badge-*`.

Contrato completo: [`docs/plugins.md`](../docs/plugins.md).
