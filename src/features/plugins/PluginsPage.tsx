import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  AppWindow,
  CircleAlert,
  ExternalLink,
  FolderOpen,
  FolderPlus,
  Globe,
  PanelLeft,
  Puzzle,
  RefreshCw,
  Settings,
  TriangleAlert,
  X,
} from "lucide-react";
import { useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge, type Tone } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Switch } from "../../components/Field";
import { Card, CopyButton, EmptyState, PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { PluginConfig, PluginSource, PluginStatus, PluginView } from "../../lib/types";
import { pluginRoute, type Route } from "../layout/navigation";
import { pluginIcon } from "./icons";
import { usePlugins } from "./usePlugins";

export const SOURCE_LABELS: Record<PluginSource, string> = {
  builtin: "Incluido",
  user: "Instalado",
  dev: "Desarrollo",
};

const SOURCE_TONES: Record<PluginSource, Tone> = { builtin: "neutral", user: "info", dev: "accent" };

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

export function PluginsPage({ onNavigate, onOpenSettings }: { onNavigate: (route: Route) => void; onOpenSettings: (pluginId: string) => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const plugins = usePlugins();
  const config = useQuery({ queryKey: queryKeys.pluginConfig, queryFn: () => ipc.getPluginConfig() });
  const [reloading, setReloading] = useState(false);

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

  return (
    <>
      <PageHeader
        title="Plugins"
        description="Extensiones cargadas desde carpetas: páginas, menús, ventanas y ajustes propios."
        actions={
          <>
            <Button icon={<FolderOpen size={15} />} onClick={openFolder}>
              Abrir carpeta de plugins
            </Button>
            <Button icon={<RefreshCw size={15} />} onClick={reload} loading={reloading}>
              Recargar
            </Button>
          </>
        }
      />

      <div className="mx-auto max-w-4xl space-y-5 px-6 py-5">
        {plugins.isPending ? <Spinner label="Buscando plugins…" /> : null}
        {plugins.isError ? <Alert tone="danger">{errorMessage(plugins.error)}</Alert> : null}

        {plugins.isSuccess && list.length === 0 ? (
          <Card>
            <EmptyState
              icon={<Puzzle size={22} />}
              title="No hay plugins instalados"
              description={
                <>
                  Copia la carpeta de un plugin (con su <code className="font-mono">plugin.json</code>) dentro de la carpeta de
                  plugins y pulsa «Recargar». Para desarrollar, activa el modo desarrollador y carga una carpeta.
                </>
              }
              action={config.data ? <FolderPath path={config.data.userPluginsDir} /> : undefined}
            />
          </Card>
        ) : null}

        {list.map((plugin) => (
          <PluginCard key={`${plugin.source}:${plugin.path}`} plugin={plugin} onNavigate={onNavigate} onOpenSettings={onOpenSettings} />
        ))}

        {config.isError ? <Alert tone="danger">{errorMessage(config.error)}</Alert> : null}
        {config.data ? <DeveloperSection config={config.data} hasPlugins={list.length > 0} /> : null}
      </div>
    </>
  );
}

function FolderPath({ path }: { path: string }) {
  return (
    <div className="flex max-w-full items-center gap-1 rounded-md border border-border bg-surface-2 py-1 pr-1 pl-2.5">
      <code className="truncate font-mono text-xs text-muted" title={path}>
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
  const enabled = plugin.status === "enabled";
  const canToggle = plugin.status === "enabled" || plugin.status === "disabled";
  const errors = plugin.issues.filter((issue) => issue.severity === "error");
  const warnings = plugin.issues.filter((issue) => issue.severity === "warning");
  const firstPage = plugin.pages[0];
  const sidebarIcon = plugin.menus.find((menu) => menu.location === "sidebar")?.icon;
  const Icon = pluginIcon(sidebarIcon);
  const needsBackend = plugin.hasBackend || plugin.destinations.length > 0 || plugin.hooks.length > 0;

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

  return (
    <section
      aria-label={`Plugin ${plugin.name}`}
      className={`rounded-xl border bg-surface shadow-card ${plugin.status === "error" ? "border-danger/40" : "border-border"}`}
    >
      <header className="flex items-start justify-between gap-4 px-5 pt-4 pb-3">
        <div className="flex min-w-0 flex-1 items-start gap-3">
          <div
            className={`mt-0.5 flex h-9 w-9 shrink-0 items-center justify-center rounded-lg ${enabled ? "bg-accent-soft text-accent" : "bg-surface-2 text-subtle"}`}
          >
            <Icon size={18} aria-hidden="true" />
          </div>
          <div className="min-w-0">
            <div className="flex flex-wrap items-center gap-1.5">
              <h2 className="text-[15px] font-semibold">{plugin.name}</h2>
              {plugin.version ? <span className="text-xs text-muted tabular">v{plugin.version}</span> : null}
              <Badge tone={SOURCE_TONES[plugin.source]}>{SOURCE_LABELS[plugin.source]}</Badge>
              <Badge tone={STATUS_TONES[plugin.status]}>{PLUGIN_STATUS_LABELS[plugin.status]}</Badge>
            </div>
            <p className="mt-0.5 font-mono text-[11px] text-subtle">{plugin.id}</p>
            {plugin.description ? <p className="mt-1 text-[13px] text-muted">{plugin.description}</p> : null}
            {plugin.author ? <p className="mt-0.5 text-xs text-subtle">Autor: {plugin.author}</p> : null}
          </div>
        </div>
        <div className="w-40 shrink-0 pt-0.5">
          <Switch
            checked={enabled}
            disabled={!canToggle || toggling}
            onChange={toggle}
            label={enabled ? "Activado" : "Desactivado"}
            description={
              plugin.status === "shadowed"
                ? "Otro plugin con el mismo id tiene prioridad."
                : plugin.status === "error"
                  ? "Corrige los errores y recarga."
                  : undefined
            }
          />
        </div>
      </header>

      <div className="space-y-3 border-t border-border px-5 py-3.5">
        {errors.length > 0 || warnings.length > 0 ? (
          <ul className="space-y-1.5" aria-label="Problemas del plugin">
            {[...errors, ...warnings].map((issue, index) => (
              <li key={`${issue.code}-${index}`} className="flex items-start gap-2 text-[13px]">
                {issue.severity === "error" ? (
                  <CircleAlert size={14} className="mt-0.5 shrink-0 text-danger" aria-label="Error" />
                ) : (
                  <TriangleAlert size={14} className="mt-0.5 shrink-0 text-warning" aria-label="Aviso" />
                )}
                <span className="min-w-0">
                  <span className="text-fg">{issue.message}</span>
                  <span className="ml-1.5 font-mono text-[11px] text-subtle">
                    {issue.code}
                    {issue.field ? ` · ${issue.field}` : ""}
                  </span>
                </span>
              </li>
            ))}
          </ul>
        ) : null}

        <div className="flex flex-wrap gap-1.5" aria-label="Aportes">
          <Badge icon={<ExternalLink size={11} />}>{countLabel(plugin.pages.length, "página", "páginas")}</Badge>
          <Badge icon={<PanelLeft size={11} />}>{countLabel(plugin.menus.length, "menú", "menús")}</Badge>
          <Badge icon={<AppWindow size={11} />}>{countLabel(plugin.windows.length, "ventana", "ventanas")}</Badge>
          {plugin.hasSettings ? <Badge icon={<Settings size={11} />}>Ajustes</Badge> : null}
          {plugin.destinations.length > 0 ? (
            <Badge tone="warning" title="Requiere backend (próximamente)">
              {countLabel(plugin.destinations.length, "destino", "destinos")} · requiere backend (próximamente)
            </Badge>
          ) : null}
          {plugin.hooks.length > 0 ? (
            <Badge tone="warning" title={plugin.hooks.join(", ")}>
              {countLabel(plugin.hooks.length, "hook", "hooks")} · requiere backend (próximamente)
            </Badge>
          ) : null}
          {plugin.hasBackend && plugin.destinations.length === 0 && plugin.hooks.length === 0 ? (
            <Badge tone="warning">Backend · requiere soporte (próximamente)</Badge>
          ) : null}
        </div>

        <div className="flex flex-wrap items-center gap-1.5 text-xs text-muted">
          <Globe size={13} className="text-subtle" aria-hidden="true" />
          {plugin.permissions.network.length > 0 ? (
            <>
              <span>Red:</span>
              {plugin.permissions.network.map((host) => (
                <code key={host} className="rounded bg-surface-2 px-1.5 py-0.5 font-mono text-[11px] text-fg">
                  {host}
                </code>
              ))}
            </>
          ) : (
            <span>Sin acceso a red</span>
          )}
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2 pt-1">
          <code className="min-w-0 truncate font-mono text-[11px] text-subtle" title={plugin.path}>
            {plugin.path}
          </code>
          <div className="flex gap-2">
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
        {needsBackend ? (
          <p className="text-xs text-subtle">
            Los destinos de backup, hooks y backends WebAssembly se habilitarán en la próxima fase; el resto del plugin ya funciona.
          </p>
        ) : null}
      </div>
    </section>
  );
}

function countLabel(count: number, singular: string, plural: string): string {
  return `${count} ${count === 1 ? singular : plural}`;
}

function DeveloperSection({ config, hasPlugins }: { config: PluginConfig; hasPlugins: boolean }) {
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

        <div className="space-y-1.5">
          <p className="text-[13px] font-medium">Carpeta de plugins instalados</p>
          <FolderPath path={config.userPluginsDir} />
          {!hasPlugins ? (
            <p className="text-xs text-muted">Copia aquí la carpeta de un plugin y pulsa «Recargar».</p>
          ) : null}
        </div>
      </div>
    </Card>
  );
}
