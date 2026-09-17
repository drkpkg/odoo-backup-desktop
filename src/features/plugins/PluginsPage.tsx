import { useQuery, useQueryClient } from "@tanstack/react-query";
import { ChevronRight, ExternalLink, FolderOpen, FolderPlus, Puzzle, RefreshCw, Settings, X } from "lucide-react";
import { useId, useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge, type Tone } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Switch } from "../../components/Field";
import { Card, CopyButton, DefinitionList, EmptyState, PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { Tabs, useTabs } from "../../components/Tabs";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { PluginConfig, PluginSource, PluginStatus, PluginView } from "../../lib/types";
import { pluginRoute, type Route } from "../layout/navigation";
import { pluginIcon } from "./icons";
import { issueText, needsBackend, pluginHealth } from "./issues";
import { usePlugins } from "./usePlugins";

export const SOURCE_LABELS: Record<PluginSource, string> = {
  builtin: "Incluido",
  user: "Instalado",
  dev: "Desarrollo",
};

export const PLUGIN_STATUS_LABELS: Record<PluginStatus, string> = {
  enabled: "Activo",
  disabled: "Desactivado",
  error: "Con errores",
  shadowed: "Reemplazado",
};

const STATUS_TONES: Record<PluginStatus, Tone> = {
  enabled: "success",
  disabled: "neutral",
  error: "danger",
  shadowed: "warning",
};

type Tab = "installed" | "development";

/**
 * Administración de plugins en dos pestañas: "Instalados" para usarlos (estado, causa visible y
 * acciones; lo técnico plegado) y "Desarrollo" para el modo desarrollador y las carpetas cargadas.
 */
export function PluginsPage({ onNavigate, onOpenSettings }: { onNavigate: (route: Route) => void; onOpenSettings: (pluginId: string) => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const plugins = usePlugins();
  const config = useQuery({ queryKey: queryKeys.pluginConfig, queryFn: () => ipc.getPluginConfig() });
  const [reloading, setReloading] = useState(false);
  const [tab, setTab] = useState<Tab>("installed");
  const tabs = useTabs<Tab>();

  const reload = async () => {
    setReloading(true);
    try {
      queryClient.setQueryData(queryKeys.plugins, await ipc.reloadPlugins());
      void queryClient.invalidateQueries({ queryKey: queryKeys.pluginSettingsAll });
      toast.success("Plugins recargados");
    } catch (err) {
      toast.error("No se pudieron recargar los plugins", errorMessage(err));
    } finally {
      setReloading(false);
    }
  };

  const openFolder = async () => {
    try {
      await ipc.openPluginsFolder();
    } catch (err) {
      toast.error("No se pudo abrir la carpeta de plugins", errorMessage(err));
    }
  };

  const list = plugins.data ?? [];
  const withProblems = list.filter((plugin) => plugin.status === "error").length;

  return (
    <>
      <PageHeader
        title="Plugins"
        description="Extensiones que agregan páginas, menús, ventanas y ajustes a la app."
        actions={
          <Button icon={<RefreshCw size={15} />} onClick={reload} loading={reloading}>
            Recargar
          </Button>
        }
      />

      <div className="mx-auto max-w-4xl px-page py-section">
        <Tabs
          label="Plugins"
          ids={tabs}
          value={tab}
          onChange={setTab}
          items={[
            {
              id: "installed",
              label: (
                <>
                  Instalados
                  {plugins.isSuccess ? <span className="text-xs font-normal text-muted tabular">{list.length}</span> : null}
                  {withProblems > 0 ? (
                    <span className="h-1.5 w-1.5 rounded-full bg-danger" role="img" aria-label={`${withProblems} con errores`} />
                  ) : null}
                </>
              ),
            },
            {
              id: "development",
              label: (
                <>
                  Desarrollo
                  {config.data?.developerMode ? <Badge tone="accent">Activo</Badge> : null}
                </>
              ),
            },
          ]}
        />

        <div role="tabpanel" id={tabs.panelId("installed")} aria-labelledby={tabs.tabId("installed")} hidden={tab !== "installed"} className="space-y-4 pt-5">
          {plugins.isPending ? <Spinner label="Buscando plugins…" /> : null}
          {plugins.isError ? <Alert tone="danger">{errorMessage(plugins.error)}</Alert> : null}

          {plugins.isSuccess && list.length === 0 ? (
            <Card>
              <EmptyState
                icon={<Puzzle size={22} />}
                title="No hay plugins instalados"
                description={
                  <>
                    Copia la carpeta de un plugin (con su <code className="font-mono">plugin.json</code>) en la carpeta de plugins y
                    pulsa «Recargar».
                  </>
                }
                action={
                  <Button icon={<FolderOpen size={15} />} onClick={openFolder}>
                    Abrir carpeta
                  </Button>
                }
              />
            </Card>
          ) : null}

          {list.map((plugin) => (
            <PluginCard key={`${plugin.source}:${plugin.path}`} plugin={plugin} onNavigate={onNavigate} onOpenSettings={onOpenSettings} />
          ))}

          {list.length > 0 ? (
            <div className="flex flex-wrap items-center justify-between gap-3 rounded-card border border-dashed border-border px-4 py-3">
              <div className="min-w-0 text-[13px]">
                <p className="font-medium">Instalar otro plugin</p>
                <p className="text-xs text-muted">Copia su carpeta en la carpeta de plugins y pulsa «Recargar».</p>
              </div>
              <Button size="sm" icon={<FolderOpen size={14} />} onClick={openFolder}>
                Abrir carpeta
              </Button>
            </div>
          ) : null}
        </div>

        <div
          role="tabpanel"
          id={tabs.panelId("development")}
          aria-labelledby={tabs.tabId("development")}
          hidden={tab !== "development"}
          className="space-y-4 pt-5"
        >
          {config.isPending ? <Spinner label="Cargando…" /> : null}
          {config.isError ? <Alert tone="danger">{errorMessage(config.error)}</Alert> : null}
          {config.data ? <DeveloperSection config={config.data} onOpenFolder={openFolder} /> : null}
        </div>
      </div>
    </>
  );
}

function FolderPath({ path }: { path: string }) {
  return (
    <div className="flex max-w-full min-w-0 items-center gap-1 rounded-md border border-border bg-surface-2 py-1 pr-1 pl-2.5">
      <code className="min-w-0 truncate font-mono text-xs text-muted" title={path}>
        {path}
      </code>
      <CopyButton value={path} label="Copiar ruta" />
    </div>
  );
}

function PluginCard({
  plugin,
  onNavigate,
  onOpenSettings,
}: {
  plugin: PluginView;
  onNavigate: (route: Route) => void;
  onOpenSettings: (pluginId: string) => void;
}) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [toggling, setToggling] = useState(false);
  const [detailsOpen, setDetailsOpen] = useState(false);
  const detailsId = useId();
  const enabled = plugin.status === "enabled";
  const canToggle = plugin.status === "enabled" || plugin.status === "disabled";
  const firstPage = plugin.pages[0];
  const Icon = pluginIcon(plugin.menus.find((menu) => menu.location === "sidebar")?.icon);
  const health = pluginHealth(plugin);
  const backend = needsBackend(plugin);
  const network = plugin.permissions.network;

  const toggle = async (next: boolean) => {
    setToggling(true);
    try {
      queryClient.setQueryData(queryKeys.plugins, await ipc.setPluginEnabled(plugin.id, next));
      toast.success(next ? `${plugin.name} activado` : `${plugin.name} desactivado`);
    } catch (err) {
      toast.error("No se pudo cambiar el estado del plugin", errorMessage(err));
    } finally {
      setToggling(false);
    }
  };

  const contributions = [
    countLabel(plugin.pages.length, "página", "páginas"),
    countLabel(plugin.menus.length, "menú", "menús"),
    countLabel(plugin.windows.length, "ventana", "ventanas"),
    plugin.hasSettings ? "ajustes" : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <section
      aria-label={`Plugin ${plugin.name}`}
      className={`rounded-card border bg-surface shadow-card ${plugin.status === "error" ? "border-danger/40" : "border-border"}`}
    >
      <div className="flex items-start gap-3 px-5 pt-4 pb-3">
        <div
          className={`mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${enabled ? "bg-accent-soft text-accent" : "bg-surface-2 text-subtle"}`}
        >
          <Icon size={18} aria-hidden="true" />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-x-2 gap-y-1">
            <h2 className="text-[15px] font-semibold">{plugin.name}</h2>
            {plugin.version ? <span className="text-xs text-muted tabular">v{plugin.version}</span> : null}
            <Badge tone={STATUS_TONES[plugin.status]}>{PLUGIN_STATUS_LABELS[plugin.status]}</Badge>
            {plugin.source === "dev" ? <Badge tone="accent">Desarrollo</Badge> : null}
          </div>
          {plugin.description ? <p className="mt-0.5 text-[13px] text-muted">{plugin.description}</p> : null}
        </div>
        {canToggle ? (
          <div className="shrink-0 pt-1">
            <Switch checked={enabled} disabled={toggling} onChange={toggle} label={<span className="sr-only">Activar {plugin.name}</span>} />
          </div>
        ) : null}
      </div>

      {health ? (
        <div className="px-5 pb-3">
          <Alert tone={health.tone} title={health.title}>
            {health.detail}
          </Alert>
        </div>
      ) : null}

      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 border-t border-border px-5 py-3">
        <p className="flex min-w-0 flex-1 flex-wrap items-center gap-x-3 gap-y-1 text-xs text-muted">
          <span>{network.length > 0 ? `Accede a ${network.join(", ")}` : "Sin acceso a red"}</span>
          {backend ? (
            <Badge tone="neutral" title="Los destinos, hooks y backends WebAssembly llegarán con el backend de plugins.">
              Backend no disponible en esta versión
            </Badge>
          ) : null}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            aria-expanded={detailsOpen}
            aria-controls={detailsId}
            onClick={() => setDetailsOpen((current) => !current)}
            className="inline-flex h-control-sm items-center gap-1 rounded-md px-2 text-[13px] text-muted hover:bg-surface-2 hover:text-fg"
          >
            <ChevronRight size={14} className={`transition-transform ${detailsOpen ? "rotate-90" : ""}`} aria-hidden="true" />
            Detalles técnicos
          </button>
          {plugin.hasSettings && plugin.status !== "error" ? (
            <Button size="sm" icon={<Settings size={13} />} onClick={() => onOpenSettings(plugin.id)}>
              Ajustes
            </Button>
          ) : null}
          {firstPage ? (
            <Button
              size="sm"
              variant="primary"
              icon={<ExternalLink size={13} />}
              disabled={!enabled}
              title={enabled ? undefined : "Activa el plugin para abrir sus páginas"}
              onClick={() => onNavigate(pluginRoute(plugin.id, firstPage.id))}
            >
              Abrir página
            </Button>
          ) : null}
        </div>
      </div>

      <div id={detailsId} hidden={!detailsOpen} className="space-y-3 border-t border-border bg-surface-2/40 px-5 py-3">
        <DefinitionList
          items={[
            { label: "Id", value: <code className="font-mono text-xs">{plugin.id}</code> },
            { label: "Origen", value: SOURCE_LABELS[plugin.source] },
            ...(plugin.author ? [{ label: "Autor", value: plugin.author }] : []),
            {
              label: "Carpeta",
              value: (
                <span className="flex min-w-0 items-center gap-1">
                  <code className="min-w-0 font-mono text-xs break-all">{plugin.path}</code>
                  <CopyButton value={plugin.path} label="Copiar ruta" />
                </span>
              ),
            },
            { label: "Aporta", value: contributions },
            { label: "Red", value: network.length > 0 ? network.join(", ") : "Sin acceso" },
            ...(backend
              ? [
                  {
                    label: "Backend",
                    value: [
                      plugin.destinations.length > 0 ? `Destinos: ${plugin.destinations.map((d) => d.label).join(", ")}` : null,
                      plugin.hooks.length > 0 ? `Hooks: ${plugin.hooks.join(", ")}` : null,
                      plugin.hasBackend ? "Módulo WebAssembly" : null,
                    ]
                      .filter(Boolean)
                      .join(" · "),
                  },
                ]
              : []),
          ]}
        />
        {plugin.issues.length > 0 ? (
          <div>
            <p className="text-[13px] font-medium">Problemas</p>
            <ul className="mt-1 space-y-1.5">
              {plugin.issues.map((issue, index) => (
                <li key={`${issue.code}-${index}`} className="text-[13px]">
                  <span className={issue.severity === "error" ? "text-danger" : "text-warning"}>
                    {issue.severity === "error" ? "Error" : "Aviso"}:
                  </span>{" "}
                  {issueText(issue)}
                  <span className="mt-0.5 block font-mono text-[11px] break-all text-subtle">
                    {issue.code}
                    {issue.field ? ` · ${issue.field}` : ""} · {issue.message}
                  </span>
                </li>
              ))}
            </ul>
          </div>
        ) : null}
      </div>
    </section>
  );
}

function countLabel(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

function DeveloperSection({ config, onOpenFolder }: { config: PluginConfig; onOpenFolder: () => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [busy, setBusy] = useState(false);

  const apply = async (action: () => Promise<PluginConfig>, success?: string) => {
    setBusy(true);
    try {
      queryClient.setQueryData(queryKeys.pluginConfig, await action());
      void queryClient.invalidateQueries({ queryKey: queryKeys.plugins });
      if (success) toast.success(success);
    } catch (err) {
      toast.error("No se pudo actualizar el modo desarrollador", errorMessage(err));
    } finally {
      setBusy(false);
    }
  };

  const addFolder = async () => {
    try {
      const path = await ipc.pickDirectory();
      if (path) await apply(() => ipc.addDevPlugin(path), "Carpeta de plugin agregada");
    } catch (err) {
      toast.error("No se pudo elegir la carpeta", errorMessage(err));
    }
  };

  return (
    <>
      <Card title="Modo desarrollador" description="Carga plugins desde cualquier carpeta y recárgalos al guardar cambios.">
        <div className="space-y-4">
          <Switch
            checked={config.developerMode}
            disabled={busy}
            onChange={(enabled) =>
              apply(() => ipc.setDeveloperMode(enabled), enabled ? "Modo desarrollador activado" : "Modo desarrollador desactivado")
            }
            label="Activar modo desarrollador"
            description="Vigila la carpeta de plugins y las carpetas cargadas; recarga automáticamente al detectar cambios."
          />

          {config.developerMode ? (
            <div className="space-y-2">
              <p className="text-[13px] font-medium">Carpetas cargadas</p>
              {config.devPluginPaths.length === 0 ? (
                <p className="text-xs text-muted">Aún no cargaste ninguna carpeta.</p>
              ) : (
                <ul className="divide-y divide-border rounded-lg border border-border">
                  {config.devPluginPaths.map((path) => (
                    <li key={path} className="flex items-center gap-2 py-1.5 pr-1.5 pl-3">
                      <code className="min-w-0 flex-1 truncate font-mono text-xs" title={path}>
                        {path}
                      </code>
                      <IconButton
                        label={`Quitar ${path}`}
                        tone="danger"
                        icon={<X size={14} />}
                        disabled={busy}
                        onClick={() => apply(() => ipc.removeDevPlugin(path), "Carpeta quitada")}
                      />
                    </li>
                  ))}
                </ul>
              )}
              <Button size="sm" icon={<FolderPlus size={14} />} onClick={addFolder} disabled={busy}>
                Cargar desde carpeta…
              </Button>
            </div>
          ) : null}
        </div>
      </Card>

      <Card title="Carpeta de plugins" description="Los plugins copiados aquí se cargan al iniciar la app o al pulsar «Recargar».">
        <div className="flex flex-wrap items-center gap-2">
          <div className="min-w-0 flex-1">
            <FolderPath path={config.userPluginsDir} />
          </div>
          <Button size="sm" icon={<FolderOpen size={14} />} onClick={onOpenFolder}>
            Abrir carpeta
          </Button>
        </div>
        <p className="mt-3 text-xs text-muted">
          Para empezar uno nuevo: <code className="font-mono">scripts/new-plugin.sh</code> copia la plantilla. Guía completa en{" "}
          <code className="font-mono">docs/plugins.md</code>.
        </p>
      </Card>
    </>
  );
}
