# Contrato del módulo Odoo `obd_backup` (API v1)

Este documento define la API que el futuro módulo `obd_backup` (Odoo 15.0–19.0) debe exponer.
El cliente de escritorio (`crates/obd-odoo`, `ObdModuleTransport`) ya lo implementa.

## Principios

- Funciona con `list_db = False`: **no** usa `odoo.service.db.dump_db` (bloqueado por
  `check_db_management_enabled`); implementa su propio `pg_dump` + zip.
- **No usa la contraseña maestra.** Autentica con API key de un usuario técnico que pertenece al
  grupo `obd_backup.group_backup_operator`.
- Generación **asíncrona** fuera del worker HTTP (evita `limit_time_real`).
- Zip con la estructura estándar de Odoo (`dump.sql`, `manifest.json`, `filestore/`), restaurable
  con el gestor de bases de datos.
- Un solo job activo por base de datos. Archivos `0600` en `<data_dir>/obd_backup/`, TTL por
  defecto 24 h.

## Métodos RPC — modelo `obd.backup.api` (`AbstractModel`, métodos `@api.model`)

Invocables por XML-RPC (`execute_kw(db, uid, key, "obd.backup.api", <método>, [], kwargs)`)
y por JSON-2 (`POST /json/2/obd.backup.api/<método>` con kwargs en el cuerpo).

### `get_info()`

```json
{ "api_version": 1, "module_version": "17.0.1.0.0", "database": "cliente1", "filestore_supported": true }
```

El cliente exige `api_version == 1`.

### `request_backup(include_filestore: bool = true)`

```json
{ "job_id": "8f5c2c1e-3a0e-4d8e-9d55-0b7f1a3f2e10" }
```

Si ya hay un job activo para la base, devuelve ese mismo `job_id`.

### `get_job(job_id: str)`

```json
{
  "job_id": "8f5c…", "state": "pending | running | done | failed | expired",
  "size": 123456789, "sha256": "hex…", "filename": "cliente1_2026-09-16_10-30-00.zip",
  "error": null, "expires_at": "2026-09-17T10:30:00Z"
}
```

`size`, `sha256` y `filename` solo están presentes con `state = "done"`.

### `discard_job(job_id: str)` → `true`

Borra el archivo generado. El cliente lo llama al terminar (best effort).

## Descarga HTTP

```
GET /obd_backup/download/<job_id>
Authorization: Bearer <api key>
X-Odoo-Database: <db>          (opcional si dbfilter resuelve la base)
Range: bytes=<offset>-         (opcional, para reanudar)
```

| Código | Significado |
|---|---|
| `200` | Archivo completo. `Content-Length`, `Accept-Ranges: bytes`, `Content-Type: application/zip` |
| `206` | Rango parcial (`Content-Range`) |
| `401` / `403` | API key inválida o usuario sin el grupo |
| `404` | Job inexistente |
| `409` | Job aún no terminado |
| `410` | Job expirado o descartado |

## Comportamiento esperado del cliente

1. `get_info` → verifica `api_version`.
2. `request_backup` → `get_job` cada `poll_interval` (por defecto 2 s) hasta `done`/`failed` o
   `prepare_timeout`.
3. Descarga con reintentos (hasta 5, backoff exponencial) reanudando con `Range`.
4. Verifica `size` y `sha256`, valida el zip y renombra `.zip.part` → `.zip`.
5. `discard_job`.
