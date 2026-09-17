# Hola OBD (plugin de ejemplo)

Muestra todo lo que un plugin puede aportar en la API v1:

- Página (`ui/index.html`) con entrada en la barra lateral.
- Ventana (`ui/detail.html`) abierta desde la página o desde «Más acciones» de una instancia.
- Ajustes con todos los tipos de campo (`settings.schema.json`), incluido un secreto.
- Almacenamiento propio (`obd.storage`), avisos (`obd.ui.toast`) e instancias/historial de solo lectura.

## Probarlo

1. En la app: *Plugins → Modo desarrollador → Cargar desde carpeta…* y elige esta carpeta.
2. O copia la carpeta a la carpeta de plugins instalados y pulsa «Recargar».

En el navegador (`pnpm dev`) el backend simulado ya lo carga.
