# Odoo Backup Desktop

Aplicación de escritorio (Linux y Windows) para descargar backups `.zip` de instancias Odoo 15.0–19.0,
guardar las credenciales cifradas y subir los backups a Google Drive.

- Detecta la versión de Odoo y elige el protocolo: **XML-RPC** (15–18) o **JSON-2** (19).
- Dos transportes de backup: **gestor de bases de datos** (`/web/database/backup`, requiere
  `list_db = True` y contraseña maestra) y **módulo `obd_backup`** (funciona con `list_db = False`,
  usa API key; el módulo se desarrolla aparte, ver [docs/obd-backup-module-api.md](docs/obd-backup-module-api.md)).
- Bóveda cifrada (XChaCha20-Poly1305) con clave en el llavero del sistema y/o contraseña maestra (Argon2id).
- Carpeta de descarga configurable, validación del zip, retención local y en Drive.

Arquitectura, contrato IPC y decisiones: [docs/architecture.md](docs/architecture.md).

## Requisitos de desarrollo

- Rust estable (≥ 1.88) y Node.js 24 con pnpm.
- Linux: `webkit2gtk-4.1`, `libsoup3`, `gtk3`, `openssl`, `librsvg` (dependencias de Tauri 2).
- Docker (opcional) para las pruebas de integración con Odoo real (`dev/odoo/`).

## Comandos

```bash
pnpm install
pnpm tauri dev                  # app en modo desarrollo
pnpm dev                        # solo la UI en el navegador (backend simulado)

cargo test --workspace          # tests de Rust
pnpm test && pnpm typecheck     # tests y tipos del frontend

pnpm tauri build                # paquetes: .deb, .rpm, AppImage (Linux) / NSIS .exe (Windows)
```

Pruebas de integración contra Odoo 15/17/19 en Docker: ver `dev/odoo/run-integration.sh`.

## Google Drive

Crea un cliente OAuth de tipo **Aplicación de escritorio** en Google Cloud con el scope
`https://www.googleapis.com/auth/drive.file` y publica la app en modo **In production** (en modo
*Testing* los tokens caducan a los 7 días). Configura el Client ID/Secret en *Ajustes → Google Drive*
o embébelos al compilar con `OBD_GDRIVE_CLIENT_ID` y `OBD_GDRIVE_CLIENT_SECRET`.

## Distribución

Los workflows de `.github/workflows/` generan los paquetes al crear un tag `v*`.
El PKGBUILD para Arch (AUR `odoo-backup-desktop-bin`) está en `packaging/aur/`.

## Licencia

[MIT](LICENSE) © 2026 Felix Daniel Coca Calvimontes
