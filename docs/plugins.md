# Plugins (API v1)

Los plugins agregan funciones a Odoo Backup Desktop sin recompilar la app: se copian a una carpeta y
la app los detecta. Esta versión (fase A) cubre **páginas, menús, ventanas, ajustes y almacenamiento
propio**. El backend de plugins en WebAssembly (destinos de backup y hooks) llega en la fase B; sus
campos del manifiesto ya se aceptan y se muestran como "requiere backend".

## Dónde se buscan

| Origen | Carpeta | Uso |
|---|---|---|
| `builtin` | `<recursos de la app>/plugins/<id>/` | Plugins incluidos en el instalador |
| `user` | `<datos de la app>/plugins/<id>/` | Plugins instalados copiando la carpeta |
| `dev` | Cualquier carpeta agregada en *Plugins → Modo desarrollador* | Desarrollo ("cargar desde carpeta") |

`<datos de la app>`: Linux `~/.local/share/io.github.drkpkg.odoo-backup-desktop`, Windows
`%APPDATA%\io.github.drkpkg.odoo-backup-desktop`.

Si dos plugins tienen el mismo `id`, gana `dev` > `user` > `builtin`; el otro queda como `shadowed`.
En modo desarrollador la app vigila las carpetas `user` y `dev` y recarga al detectar cambios.

## Estructura de un plugin

```
hello-obd/
  plugin.json          manifiesto (obligatorio)
  settings.schema.json esquema de ajustes (opcional)
  ui/index.html        páginas y ventanas (HTML/JS/CSS; cualquier framework que compile a estático)
  backend.wasm         fase B
```

## Manifiesto `plugin.json`

```json
{
  "id": "hello-obd",
  "name": "Hola OBD",
  "version": "0.1.0",
  "apiVersion": 1,
  "description": "Plugin de ejemplo",
  "author": "Felix Daniel Coca Calvimontes",
  "homepage": "https://github.com/drkpkg/odoo-backup-desktop",
  "contributes": {
    "pages": [{ "id": "main", "title": "Hola", "path": "ui/index.html" }],
    "menus": [
      { "id": "sidebar", "location": "sidebar", "label": "Hola", "icon": "sparkles", "page": "main" },
      { "id": "instance", "location": "instance_actions", "label": "Ver con Hola", "icon": "eye", "window": "detail" }
    ],
    "windows": [{ "id": "detail", "title": "Detalle", "path": "ui/detail.html", "width": 720, "height": 520 }],
    "settings": "settings.schema.json",
    "destinations": [],
    "hooks": []
  },
  "permissions": { "network": ["api.example.com", "*.example.org"] },
  "backend": null
}
```

### Reglas de validación

| Campo | Regla |
|---|---|
| `id` | `^[a-z0-9][a-z0-9-]{1,62}[a-z0-9]$` (3–64 caracteres) |
| `name` | 1–80 caracteres |
| `version` | SemVer `MAJOR.MINOR.PATCH[-pre][+build]` |
| `apiVersion` | Debe ser `1` |
| `pages[].id`, `menus[].id`, `windows[].id` | `^[a-z0-9][a-z0-9_-]{0,62}$`, únicos dentro de su lista |
| `*.path`, `settings`, `backend` | Relativos al plugin, sin `..`, sin ruta absoluta; el archivo debe existir dentro de la carpeta (también tras resolver enlaces simbólicos) |
| `menus[]` | `location`: `sidebar` \| `instance_actions`; exactamente uno de `page` o `window`, que debe existir |
| `menus[].icon` | Nombre en kebab-case (`^[a-z0-9-]{1,40}$`); si la app no lo conoce usa `puzzle` |
| `windows[].width/height` | 320–4096 (opcional) |
| `permissions.network[]` | Host (`api.example.com`) o comodín de subdominio (`*.example.com`), sin esquema ni puerto |
| `backend`, `destinations`, `hooks` | Aceptados; generan el aviso `backend_not_supported` hasta la fase B |
| Claves desconocidas | Error (`deny_unknown_fields`) para detectar errores de tipeo |

Errores → el plugin queda en estado `error` y no se carga; avisos → se carga igual.

Códigos de issue: `manifest_missing`, `manifest_unreadable`, `manifest_invalid`, `invalid_id`,
`invalid_name`, `invalid_version`, `unsupported_api_version`, `invalid_contribution_id`,
`duplicate_contribution_id`, `invalid_label`, `invalid_path`, `path_not_found`, `path_outside_plugin`,
`invalid_menu_target`, `unknown_page`, `unknown_window`, `invalid_icon`, `invalid_window_size`,
`invalid_network_host`, `settings_schema_invalid` (errores) y `backend_not_supported` (aviso).

## Esquema de ajustes (subconjunto de JSON Schema)

```json
{
  "type": "object",
  "title": "Ajustes de Hola",
  "properties": {
    "endpoint": { "type": "string", "title": "URL", "format": "url", "default": "https://api.example.com" },
    "token": { "type": "string", "title": "Token", "secret": true, "minLength": 10 },
    "retries": { "type": "integer", "title": "Reintentos", "minimum": 0, "maximum": 10, "default": 3 },
    "mode": { "type": "string", "title": "Modo", "enum": ["rapido", "seguro"], "enumLabels": ["Rápido", "Seguro"] },
    "notify": { "type": "boolean", "title": "Notificar", "default": true },
    "notes": { "type": "string", "title": "Notas", "format": "multiline", "maxLength": 2000 }
  },
  "required": ["endpoint"]
}
```

- `type` de propiedad: `string`, `number`, `integer`, `boolean`.
- Palabras clave: `title`, `description`, `default`, `enum` (+ `enumLabels`), `minimum`, `maximum`,
  `minLength`, `maxLength`, `pattern` (regex), `format` (`url`, `email`, `password`, `multiline`),
  `placeholder`, `secret`.
- `secret: true` (o `format: "password"`) → el valor se guarda **cifrado en la bóveda**, nunca se
  devuelve; la UI solo sabe si está definido. Requiere la bóveda desbloqueada.
- Valores no secretos → `<datos de la app>/plugin-settings.json`.
- La app genera el formulario en *Ajustes → Plugins* y valida en el backend.

## Páginas y ventanas

- Se sirven con el protocolo `obd-plugin`:
  Linux/macOS `obd-plugin://localhost/<id>/<ruta>`, Windows `http://obd-plugin.localhost/<id>/<ruta>`.
  La app entrega a la UI la URL base de cada plugin (`baseUrl`).
- Se muestran dentro de un `<iframe sandbox="allow-scripts allow-forms allow-popups">` (origen opaco):
  el plugin **no** tiene acceso a la IPC de Tauri ni al DOM de la app. Se comunica con el **puente**.
- Ventanas: la app abre una ventana (`plugin--<id>--<windowId>`) con su propio shell y el mismo iframe.
- Parámetros (p. ej. `instanceId` desde `instance_actions`) llegan en `context.get()`.
- `localStorage` no está disponible (origen opaco): usar `obd.storage`.
- CSP de las páginas: scripts/estilos/imágenes del propio protocolo (+ inline), `connect-src` limitado a
  `permissions.network` (https).

## SDK y puente (`/_sdk/obd-plugin.js`)

```html
<link rel="stylesheet" href="../../_sdk/obd-plugin.css" />
<script type="module">
  import obd from "../../_sdk/obd-plugin.js";
  const ctx = await obd.context();
  const instances = await obd.instances.list();
  await obd.ui.toast({ kind: "success", title: `Hola ${ctx.pluginId}` });
</script>
```

La ruta `_sdk` es reservada (ningún plugin puede usar el id `_sdk`). Rutas relativas funcionan porque
la página vive en `<base>/<id>/ui/index.html`; también se puede usar `obd-plugin://localhost/_sdk/...`.

### Mensajes

Petición (plugin → app): `{ "obd": 1, "id": "7", "method": "storage.get", "params": { "key": "x" } }`
Respuesta (app → plugin): `{ "obd": 1, "id": "7", "result": … }` o `{ "obd": 1, "id": "7", "error": { "code", "message" } }`
Evento (app → plugin): `{ "obd": 1, "event": "theme", "data": { "theme": "dark" } }`

La app solo acepta mensajes cuyo `event.source` es el iframe de ese plugin; el `pluginId` lo decide la
app, nunca el mensaje.

| Método | Parámetros | Resultado |
|---|---|---|
| `context.get` | — | `{ pluginId, pluginVersion, appVersion, theme, surface: "page" \| "window", params }` |
| `settings.get` | — | `{ values, secretsSet: string[] }` (sin secretos) |
| `settings.save` | `{ values }` (secreto: string = reemplazar, `null` = borrar, ausente = conservar) | igual que `settings.get` |
| `storage.get` | `{ key }` | valor JSON o `null` |
| `storage.set` | `{ key, value }` (`null` borra) | `null` |
| `instances.list` | — | `[{ id, name, url, database, odooVersion, lastBackup }]` (sin secretos) |
| `history.list` | `{ instanceId?, limit? }` | `HistoryEntry[]` |
| `ui.toast` | `{ kind: "info" \| "success" \| "error", title, description? }` | `null` |
| `ui.openWindow` | `{ windowId, params? }` | `null` |
| `ui.navigate` | `{ pageId, params? }` (solo desde páginas) | `null` |
| `ui.openSettings` | — | `null` |

Eventos: `theme` (`{ theme }`), `plugins-changed` (`{}`).

Límites: `storage` 1 MiB por plugin (JSON), clave `^[A-Za-z0-9._-]{1,128}$`; respuestas en 30 s.

## Comandos de la app (IPC)

| Comando | Args | Retorno |
|---|---|---|
| `list_plugins` | — | `PluginView[]` |
| `reload_plugins` | — | `PluginView[]` |
| `set_plugin_enabled` | `{ pluginId, enabled }` | `PluginView[]` |
| `get_plugin_config` | — | `PluginConfig` |
| `set_developer_mode` | `{ enabled }` | `PluginConfig` |
| `add_dev_plugin` | `{ path }` | `PluginConfig` |
| `remove_dev_plugin` | `{ path }` | `PluginConfig` |
| `open_plugins_folder` | — | `void` |
| `get_plugin_settings` | `{ pluginId }` | `PluginSettings` |
| `save_plugin_settings` | `{ pluginId, values }` | `PluginSettings` |
| `plugin_storage_get` | `{ pluginId, key }` | `JSON \| null` |
| `plugin_storage_set` | `{ pluginId, key, value }` | `void` |
| `open_plugin_window` | `{ pluginId, windowId, params? }` | `void` (si ya está abierta: la enfoca y actualiza `params`) |
| `get_plugin_window_context` | — (usa la ventana que llama) | `{ pluginId, windowId, params }` |

Las ventanas de plugin cargan la misma app (`index.html`) con label `plugin--<pluginId>--<windowId>`.
La UI detecta ese prefijo (`getCurrentWebviewWindow().label`), pide su contexto con
`get_plugin_window_context` y muestra solo el shell del plugin. Su capability (`plugin-windows.json`)
permite únicamente: `get_app_status`, `list_instances`, `list_history`, `list_plugins`,
`get_plugin_settings`, `save_plugin_settings`, `plugin_storage_get`, `plugin_storage_set`,
`open_plugin_window`, `get_plugin_window_context`, escuchar eventos y emitir
`plugin-open-settings` (`{ pluginId }`) hacia la ventana `main` (para `ui.openSettings`).

Evento global `plugins-changed` (`{ reason: "reload" | "watch" | "config" }`).

```ts
type PluginSource = "builtin" | "user" | "dev";
type PluginStatus = "enabled" | "disabled" | "error" | "shadowed";
type PluginIssue = { severity: "error" | "warning"; code: string; message: string; field: string | null };

type PluginView = {
  id: string;                 // si el manifiesto no se pudo leer: nombre de la carpeta
  name: string; version: string | null; description: string | null; author: string | null;
  homepage: string | null;
  source: PluginSource; path: string; status: PluginStatus; issues: PluginIssue[];
  baseUrl: string;            // termina en "/"
  revision: number;           // cambia en cada recarga (para refrescar iframes)
  pages: { id: string; title: string; path: string }[];
  menus: { id: string; location: "sidebar" | "instance_actions"; label: string; icon: string | null;
           page: string | null; window: string | null }[];
  windows: { id: string; title: string; path: string; width: number | null; height: number | null }[];
  hasSettings: boolean;
  destinations: { id: string; label: string }[];
  hooks: string[];
  permissions: { network: string[] };
  hasBackend: boolean;
};

type PluginConfig = { developerMode: boolean; devPluginPaths: string[]; userPluginsDir: string };

type SchemaProperty = {
  type: "string" | "number" | "integer" | "boolean";
  title?: string; description?: string; default?: unknown; enum?: (string | number)[]; enumLabels?: string[];
  minimum?: number; maximum?: number; minLength?: number; maxLength?: number; pattern?: string;
  format?: "url" | "email" | "password" | "multiline"; placeholder?: string; secret?: boolean;
};
type SettingsSchema = { type: "object"; title?: string; description?: string;
  properties: Record<string, SchemaProperty>; propertyOrder: string[]; required: string[] };

type PluginSettings = { schema: SettingsSchema; values: Record<string, unknown>; secretsSet: string[] };
```

Error de validación de ajustes: código `plugin_settings_invalid`, `message` = JSON
`[{ "field": "retries", "code": "maximum", "message": "must be <= 10" }]`.

Otros códigos: `plugin_not_found`, `plugin_disabled`, `plugin_invalid`, `plugin_no_settings`,
`plugin_window_not_found`, `plugin_storage_limit`, `plugin_storage_key_invalid`, `plugin_path_invalid`,
`plugin_store_corrupted`.

Reglas de valores: en campos requeridos `""` cuenta como vacío; un secreto con `""` se borra; un campo
no secreto en `null` vuelve a su `default`.

## Seguridad

- Los plugins son código de confianza (los escribe el dueño de la app), pero igual:
  - No acceden a la IPC ni a las credenciales de instancias; solo a lo que expone el puente.
  - Sus secretos viven en la bóveda, separados por plugin.
  - Sus archivos se sirven solo desde su carpeta (sin `..` ni enlaces que salgan de ella).
- Deshabilitar un plugin deja de servir sus archivos y oculta sus menús.
