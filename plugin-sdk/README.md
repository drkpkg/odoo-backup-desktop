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
`plugin_storage_limit`, `unsupported_surface`).

## Estilos (`obd-plugin.css`)

La hoja replica la paleta, la densidad y las formas de la app (una prueba de la app verifica que
sigan iguales), así que un plugin que la usa se ve integrado y no "parecido".

**Tokens** (variables CSS, cambian solas con el tema):

| Grupo | Variables |
|---|---|
| Color | `--obd-bg`, `--obd-surface`, `--obd-surface-2`, `--obd-border(-strong)`, `--obd-text`, `--obd-muted`, `--obd-subtle`, `--obd-accent(-hover/-fg/-soft)`, `--obd-success/-warning/-danger/-info(-soft)`, `--obd-focus` |
| Espaciado | `--obd-space-1` … `--obd-space-6` (4 px a 24 px), `--obd-space-page`, `--obd-space-section` |
| Alturas | `--obd-control-height` (36 px), `--obd-control-height-sm` (28 px) |
| Formas | `--obd-radius-sm` (controles), `--obd-radius-card`, `--obd-radius-overlay` (`--obd-radius` = tarjeta) |
| Sombras | `--obd-shadow`, `--obd-shadow-overlay` |
| Capas | `--obd-z-sticky`, `--obd-z-dropdown`, `--obd-z-overlay`, `--obd-z-toast` |

Todos los colores de texto cumplen WCAG AA (4.5:1) sobre los fondos de la paleta.

**Base:** `.obd-page`, `.obd-card`, `.obd-row`, `.obd-between`, `.obd-stack`, `.obd-grid`, `.obd-muted`,
`.obd-btn` (+ `-primary`, `-danger`, `-sm`), `.obd-field`, `.obd-input`, `.obd-select`, `.obd-textarea`,
`.obd-table`, `.obd-badge-*`, `.obd-dl`, `.obd-pre`, `.obd-sr-only`.

**Patrones** (ver `examples/plugins/hello-obd/ui/index.html`):

```html
<!-- Encabezado de página -->
<header class="obd-page-header">
  <div><h1>Título</h1><p>Descripción breve.</p></div>
  <div class="obd-page-header-actions"><button class="obd-btn obd-btn-primary">Acción</button></div>
</header>

<!-- Estado vacío -->
<div class="obd-empty">
  <div class="obd-empty-icon" aria-hidden="true">…svg…</div>
  <h2>No hay datos</h2>
  <p>Qué hacer para que aparezcan.</p>
</div>

<!-- Aviso; -neutral para una capacidad que no está disponible en esta versión -->
<div class="obd-banner obd-banner-neutral" role="note">
  <div><p class="obd-banner-title">Título</p><p>Detalle.</p></div>
</div>

<!-- Tabla que se apila en tarjetas si el contenedor mide menos de 520 px (cada celda con data-label) -->
<div class="obd-table-wrap">
  <table class="obd-table obd-table-stack">…<td data-label="Versión">17.0</td>…</table>
</div>

<!-- Carga -->
<p class="obd-loading" role="status"><span class="obd-spinner" aria-hidden="true"></span>Cargando…</p>
<div class="obd-skeleton" style="height: 16px"></div>
```

Las páginas viven en iframes: usa foco visible (ya incluido con `:focus-visible`), etiquetas en los
controles y `role="status"` para cargas, igual que la app.

Contrato completo: [`docs/plugins.md`](../docs/plugins.md).
