# Revisión UI/UX — Odoo Backup Desktop

> Informe generado con Codex (2026-09-16) a partir de 32 capturas (mock claro/oscuro, 900×600 y app real en WebKitGTK) y del código. Referencias `archivo:línea` verificadas al commit `1162f6c`.
>
> **Estado:** lote 1 (tablas operables) implementado. Decisión: la UI usa «respaldo» en lugar de «backup».

## Resumen

La base visual es sólida: jerarquía calmada, componentes consistentes, buen soporte claro/oscuro y controles semánticos. Los principales problemas son de densidad y prioridad de información: a 900×600 las tablas ocultan acciones críticas, el formulario de instancia divide una tarea lineal en dos columnas que compiten por el alto disponible, y el progreso de backup ocupa dos lugares.

La dirección recomendada es “operación rápida y verificable”: conservar una tabla compacta para power users, pero mostrar únicamente lo necesario en su vista inicial; usar un único centro de progreso; y convertir alta/prueba de instancia en un flujo progresivo. No requiere rediseñar contratos salvo mejoras futuras de metadatos o acciones de plugins.

## Validación de sugerencias preliminares

| # | Veredicto | Evidencia | Propuesta refinada |
|---|---|---|---|
| 1 | Ajustada | `04`, `15`; `InstancesPage.tsx:124-135,289-307` | Las acciones no son inaccesibles técnicamente (la tabla hace scroll horizontal), pero quedan fuera de vista sin indicador; dejar “Respaldar” y un menú único de acciones, ocultar columnas secundarias a 900 px. |
| 2 | Confirmada | `03`, `04`, `15`; `AppLayout.tsx:74-80` | Reducir el bloque de marca: nombre en una línea o versión abreviada, menos alto vertical y más espacio para navegación. |
| 3 | Ajustada | `08-10`; `InstancesPage.tsx:267-272`, `BackupJobsPanel.tsx:27-34`, `Spinner.tsx:12-29` | Sí hay duplicación; la barra ya es indeterminada cuando `value=null`, pero conviene mostrar el progreso solo en el panel global y dejar la fila con estado breve/enlace. |
| 4 | Confirmada | `05-07`, `16`; `InstanceFormDialog.tsx:137-297`, `Dialog.tsx:60-77` | Separar “Datos y credenciales” de “Opciones avanzadas”; mover diagnóstico a una etapa/panel posterior y fijar las acciones al pie del modal. |
| 5 | Confirmada | `01`, `real-01`; `VaultGate.tsx:19-21`, `InstancesPage.tsx:110-120` | Tras crear la bóveda, mostrar un checklist de arranque con “Añadir primera instancia” como CTA. |
| 6 | Confirmada | `04`, `15`; `InstancesPage.tsx:129-132,231-265` | Sustituir columnas Protocolo/Transporte por “Conexión” resumida; conservar detalles en menú o ficha de instancia. |
| 7 | Confirmada | `12`; `SettingsPage.tsx:20-30` | Añadir navegación ancla lateral o pestañas: Backups, Seguridad, Google Drive, Plugins; cada sección conserva su guardado independiente. |
| 8 | Confirmada | `13-14`, `real-02`; `PluginsPage.tsx:87-127,314-391`, `PluginPageView.tsx:56-80` | Separar operación de plugins de herramientas de desarrollo; reforzar chrome y contrato visual del iframe. |
| 9 | Confirmada | `11`, `17`; `HistoryPage.tsx:65-137,150-192` | Filtros correctos, pero SHA, Drive y acción se salen a 900 px; priorizar resultado, tamaño y abrir carpeta, relegando metadatos. |
| 10 | Ajustada | `dark-01` a `dark-14`; `styles.css:36-65,117-120` | Tema coherente y foco global existente; falta auditoría WCAG medida de texto sutil, badges, bordes y estados disabled. |

## Plan priorizado

### P0 — Bloqueos de operación

**1. Hacer alcanzables las acciones de instancia e historial en 900×600**

- Problema: las tablas tienen `min-w-[820px]` dentro de scroll horizontal; tras sidebar y márgenes, las acciones quedan fuera de la vista inicial (`15`, `17`; `InstancesPage.tsx:124-135,289-307`, `HistoryPage.tsx:115-137,185-192`).
- Impacto: no se descubre cómo respaldar, editar, eliminar o abrir archivos.
- Propuesta: en instancias mostrar “Respaldar” + menú `Más acciones` (Probar conexión, Historial, Editar, Eliminar y acciones de plugins). A ≤1000 px ocultar Protocolo y Transporte; añadir “Conexión: Lista / Sin probar / Requiere atención”. En historial ocultar SHA y Drive tras un menú “Detalles”; mantener Fecha, Instancia, Estado, Tamaño y “Abrir carpeta”.
- Archivos: `InstancesPage.tsx`, `HistoryPage.tsx`, `InstanceActionsMenu.tsx`, `labels.ts`.
- Backend/contrato: No.
- Esfuerzo: M.
- Aceptación: a 900×600 y en oscuro, cada fila expone respaldo y menú sin scroll horizontal; todas las acciones actuales siguen disponibles por teclado.

**2. Reestructurar el alta/edición de instancia**

- Problema: el diálogo de dos columnas es largo; al mínimo tamaño el diagnóstico y pie quedan fuera de la primera vista (`05-07`, `16`; `InstanceFormDialog.tsx:137-297`).
- Impacto: alta lenta, errores de configuración y pérdida de contexto sobre qué corregir.
- Propuesta: sección inicial “Conexión” (Nombre, URL, base, usuario, credencial) + CTA “Probar conexión”. Tras éxito, revelar “Configuración avanzada” colapsada: protocolo, transporte, contraseña maestra, filestore y Drive. Mostrar el diagnóstico debajo de la CTA con bloque “Siguiente paso” prioritario: “Usa el Gestor de BD: guarda la contraseña maestra” o “Instala `obd_backup` y usa una API key”. Pie sticky con “Cancelar” y “Crear instancia”.
- Archivos: `InstanceFormDialog.tsx`, `ProbeReportView.tsx`, `Dialog.tsx`, `labels.ts`.
- Backend/contrato: No.
- Esfuerzo: M.
- Aceptación: a 900×600 el formulario permite completar URL, probar y guardar sin que el CTA desaparezca; el resultado de prueba comunica una acción concreta; funciona en oscuro.

**3. Unificar el estado de backups activos**

- Problema: la misma información aparece en fila y tarjeta flotante (`08-10`; `InstancesPage.tsx:267-272`, `BackupJobsPanel.tsx:27-34`).
- Impacto: ruido visual y dudas sobre cuál estado es autoritativo.
- Propuesta: el panel flotante es la única vista de progreso detallado. En la fila: badge “Backup en curso” y enlace/botón “Ver progreso”. Mantener barras indeterminadas para `requesting`, `server_preparing`, `validating`, `retention` y descargas sin total, comportamiento ya implementado en `Spinner.tsx:12-29`.
- Archivos: `InstancesPage.tsx`, `BackupJobsPanel.tsx`, `AppLayout.tsx`.
- Backend/contrato: No.
- Esfuerzo: S.
- Aceptación: un backup activo tiene una sola barra visible; fases sin porcentaje no muestran un valor numérico ni sugieren completitud.

### P1 — Mejoras UX claras

**4. Incorporar onboarding posbóveda y estado vacío guiado**

- Problema: al crear la bóveda se llega a un estado vacío genérico (`01`, `real-01`; `VaultGate.tsx:19-21`, `InstancesPage.tsx:110-120`).
- Impacto: se desconoce el orden recomendado y Drive aparece demasiado pronto.
- Propuesta: banner persistente hasta primer backup: “1. Añade una instancia”, “2. Prueba la conexión”, “3. Elige dónde guardar los backups”, “4. Ejecuta tu primer backup”. CTA principal: “Añadir primera instancia”; secundarios: “Elegir carpeta” y “Configurar Google Drive (opcional)”.
- Archivos: `InstancesPage.tsx`, `Layout.tsx`, `App.tsx`.
- Backend/contrato: No; persistir descarte sería un cambio opcional de settings.
- Esfuerzo: M.
- Aceptación: una instalación nueva puede iniciar el flujo principal desde una sola pantalla y no se presenta Drive como requisito.

**5. Reordenar Ajustes por intención**

- Problema: cuatro áreas heterogéneas se apilan en una página larga (`12`; `SettingsPage.tsx:20-30`).
- Impacto: navegar, detectar cambios y volver a una sección resulta lento.
- Propuesta: navegación interna sticky: “Backups”, “Seguridad”, “Google Drive”, “Plugins”; mantener guardado por sección y mostrar `Cambios sin guardar` junto al título de esa sección. En Google Drive, conservar OAuth bajo “Configuración avanzada”.
- Archivos: `SettingsPage.tsx`, `GeneralSection.tsx`, `SecuritySection.tsx`, `DriveSection.tsx`, `PluginsSettingsSection.tsx`.
- Backend/contrato: No.
- Esfuerzo: M.
- Aceptación: cada sección es alcanzable en un clic y deja claro qué bloque está modificado, también con panel de backup activo.

**6. Simplificar la tabla de instancias para operación cotidiana**

- Problema: XML-RPC, JSON-2 y transporte ocupan espacio aunque se detectan automáticamente (`04`; `InstancesPage.tsx:129-132,231-265`).
- Impacto: aumenta carga cognitiva sin ayudar a decidir el siguiente backup.
- Propuesta: columnas: Instancia, Conexión, Último backup, Acciones. En “Conexión” usar `Lista`, `Sin probar`, `Atención requerida`; tooltip/detalle muestra Odoo, protocolo y transporte. Mantener etiquetas técnicas en el diálogo de prueba.
- Archivos: `InstancesPage.tsx`, `ProbeReportView.tsx`.
- Backend/contrato: No.
- Esfuerzo: S.
- Aceptación: la vista inicial no muestra XML-RPC/JSON-2; el detalle técnico sigue disponible y no se pierde información.

**7. Separar administrador de plugins de modo desarrollador**

- Problema: la página mezcla catálogo operativo, rutas locales y herramientas de desarrollo (`13`; `PluginsPage.tsx:87-127,314-391`).
- Impacto: al propietario no desarrollador le cuesta distinguir estado, error y cómo instalar.
- Propuesta: pestañas “Instalados” y “Desarrollo”; en tarjetas, cabecera con Estado + causa visible y bloque plegable “Detalles técnicos” para ID, ruta, permisos y códigos. Para Phase B: badge neutral “Backend no disponible en esta versión”, sin prometer “próximamente”.
- Archivos: `PluginsPage.tsx`, `PluginsSettingsSection.tsx`.
- Backend/contrato: No.
- Esfuerzo: M.
- Aceptación: un plugin con error comunica primero el arreglo; herramientas de carpeta solo aparecen en Desarrollo.

### P2 — Pulido

**8. Compactar marca lateral y normalizar densidad**

- Problema: la marca usa dos líneas y consume alto (`03`, `15`; `AppLayout.tsx:74-80`).
- Impacto: compite con navegación, especialmente con muchas extensiones.
- Propuesta: ancho 208 px: icono + “Odoo Backup” en una línea; subtítulo opcional eliminarlo o mostrarlo como tooltip. Reducir padding superior/inferior del bloque.
- Archivos: `AppLayout.tsx`.
- Backend/contrato: No.
- Esfuerzo: S.
- Aceptación: marca no envuelve a 900 px y los elementos de extensiones conservan área de scroll.

## Sistema de diseño

- Conservar los tokens semánticos actuales (`--app-bg`, `--app-surface`, `--app-surface-2`, `--app-sidebar`, `--app-border`, `--app-border-strong`, `--app-text`, `--app-muted`, `--app-subtle`, `--app-accent`) definidos en `styles.css:4-33`.
- Añadir tokens de densidad: `--app-space-page`, `--app-space-section`, `--app-control-height`, `--app-sidebar-width`, `--app-radius-card`; sustituir valores repetidos de `px-6`, `py-5`, `h-9`, `rounded-xl`.
- Formalizar variantes de tabla: `compact`, `responsive` y `details`; evitar que cada pantalla resuelva su propia sobrecarga horizontal.
- Unificar superficies flotantes: panel de backup, toast y menú comparten `--app-shadow-lg`, borde y radio; el panel debería responder a ancho disponible (`w-[min(24rem,calc(100vw-2rem))]`).

## Accesibilidad

- Buenas bases: foco visible global (`styles.css:117-120`), diálogo nativo (`Dialog.tsx:49-83`), etiquetas y descripciones de campos (`Field.tsx:21-43`), menú de plugins navegable con flechas (`InstanceActionsMenu.tsx:64-84`).
- Medir contraste WCAG AA de `--app-subtle` sobre `--app-bg`/`--app-surface`, badges de tono suave y `disabled:opacity-60`; las capturas oscuras muestran texto secundario al límite (`dark-04`, `dark-12`).
- Hacer que el texto de iconos de estado no dependa del color: el informe de prueba ya aporta `aria-label`, pero conviene exponer el estado completo en la fila resumida (`ProbeReportView.tsx:20-37`).
- Tras abrir/cerrar el menú de acciones, devolver foco al disparador también tras seleccionar una acción si no hay navegación; validar Tab/Shift+Tab en la tabla responsiva.
- El panel flotante debe anunciar cambios de fase sin repetir anuncios: añadir un único `role="status"` textual por job, no a la barra y tarjeta simultáneamente.
- Probar orden de foco, Escape y restauración de foco en el modal de instancia y el modal de confirmación, en Chromium y WebKitGTK.

## Microcopy

| Actual | Propuesta | Evidencia |
|---|---|---|
| “Backup” | “Respaldar ahora” (botón), “Respaldo en curso” (estado) | `InstancesPage.tsx:291-302` |
| “Correcto” | “Completado” o “Listo” según contexto | `labels.ts:49-53` |
| “Gestor de BD” | “Administrador de bases de datos” en ayuda; abreviatura solo en tabla | `labels.ts:18-27` |
| “Base de datos deducida del subdominio” | “Verifica la base de datos sugerida” | `labels.ts:87-90` |
| “No hay un transporte…” | “Falta configurar un método de respaldo” | `ProbeReportView.tsx:87-89` |
| “Subir a Google Drive” | “Subir este respaldo a Google Drive” | `InstanceFormDialog.tsx:258` |
| “Abrir carpeta de plugins” | “Abrir carpeta” + subtítulo explicativo | `PluginsPage.tsx:92-94` |

## Plugins UX

- Mantener el iframe sandboxed; no ampliar permisos (`PluginFrame.tsx:93-102`).
- El SDK duplica correctamente la paleta de la app (`plugin-sdk/obd-plugin.css:4-30,60-84`), pero debe publicar tokens de espaciado, alturas, radios y capas además de color. Así un plugin no queda “parecido”, sino realmente integrado.
- Añadir al SDK patrones documentados: encabezado de página, empty state, banner de capacidad no disponible, tabla responsive y estado de carga.
- El chrome nativo debe indicar claramente “Plugin · {nombre}”, conservar “Ajustes” y añadir “Volver a Plugins” en páginas a pantalla completa; actualmente solo hay breadcrumb compacto (`PluginPageView.tsx:56-68`).
- Para permisos/red y backend Phase B, mostrar declaración resumida para usuario y detalles técnicos plegables para desarrollador; no mezclar permisos con el CTA principal.
- El ejemplo ya importa la hoja común (`examples/plugins/hello-obd/ui/index.html:7`); añadir ejemplos de foco, tabla angosta y vacío para hacer la integración plug-and-play.

## Orden de implementación sugerido

1. **Tablas operables:** menú unificado de acciones, columnas responsivas y prueba a 900×600.
2. **Flujo de instancia:** secciones progresivas, diagnóstico accionable y footer sticky.
3. **Estado global:** eliminar duplicación de progreso, añadir acceso “Ver progreso” y revisar anuncios.
4. **Ajustes y primer uso:** onboarding + navegación interna de ajustes.
5. **Plugins y sistema visual:** separar modo desarrollador, extender tokens SDK y ejecutar auditoría de contraste/teclado.