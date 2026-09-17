import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Puzzle } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Alert } from "../../components/Alert";
import { Card } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { PluginView } from "../../lib/types";
import { pluginIcon } from "../plugins/icons";
import { SchemaForm } from "../plugins/SchemaForm";
import { usePlugins } from "../plugins/usePlugins";

/** Ajustes de los plugins que declaran un esquema; `focusPluginId` abre y enfoca uno. */
export function PluginsSettingsSection({ focusPluginId }: { focusPluginId: string | null }) {
  const plugins = usePlugins();
  const withSettings = (plugins.data ?? []).filter((plugin) => plugin.hasSettings && plugin.status !== "error" && plugin.status !== "shadowed");

  if (plugins.isSuccess && withSettings.length === 0 && !focusPluginId) return null;

  return (
    <Card
      title={
        <span className="flex items-center gap-2">
          <Puzzle size={16} className="text-accent" /> Plugins
        </span>
      }
      description="Ajustes declarados por cada plugin. Los campos secretos se guardan cifrados en la bóveda."
    >
      {plugins.isPending ? <Spinner label="Cargando plugins…" /> : null}
      {plugins.isError ? <Alert tone="danger">{errorMessage(plugins.error)}</Alert> : null}
      {plugins.isSuccess && withSettings.length === 0 ? (
        <p className="text-[13px] text-muted">Ningún plugin activo declara ajustes.</p>
      ) : null}
      <div className="divide-y divide-border">
        {withSettings.map((plugin) => (
          <PluginSettingsItem key={plugin.id} plugin={plugin} focused={plugin.id === focusPluginId} />
        ))}
      </div>
    </Card>
  );
}

function PluginSettingsItem({ plugin, focused }: { plugin: PluginView; focused: boolean }) {
  const [open, setOpen] = useState(focused);
  const ref = useRef<HTMLDivElement>(null);
  const Icon = pluginIcon(plugin.menus.find((menu) => menu.location === "sidebar")?.icon);
  const panelId = `plugin-settings-${plugin.id}`;

  const settings = useQuery({
    queryKey: queryKeys.pluginSettings(plugin.id),
    queryFn: () => ipc.getPluginSettings(plugin.id),
    enabled: open,
  });

  useEffect(() => {
    if (focused) setOpen(true);
  }, [focused]);

  // Se desplaza cuando el formulario ya tiene su altura final (tras cargar los ajustes).
  const loaded = settings.isSuccess || settings.isError;
  useEffect(() => {
    if (focused && loaded) ref.current?.scrollIntoView({ block: "start" });
  }, [focused, loaded]);

  return (
    <div ref={ref} className="scroll-mt-4 py-2 first:pt-0 last:pb-0">
      <button
        type="button"
        aria-expanded={open}
        aria-controls={panelId}
        onClick={() => setOpen((value) => !value)}
        className="flex w-full items-center gap-2.5 rounded-md px-1 py-1.5 text-left hover:bg-surface-2/60"
      >
        <Icon size={15} className="shrink-0 text-muted" aria-hidden="true" />
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-medium">{settings.data?.schema.title ?? plugin.name}</span>
          <span className="block truncate text-xs text-subtle">
            {plugin.name}
            {plugin.status === "disabled" ? " · desactivado" : ""}
          </span>
        </span>
        <ChevronDown size={16} className={`shrink-0 text-subtle transition-transform ${open ? "rotate-180" : ""}`} aria-hidden="true" />
      </button>
      {open ? (
        <div id={panelId} className="px-1 pt-3 pb-2">
          {settings.isPending ? <Spinner label="Cargando ajustes…" /> : null}
          {settings.isError ? <Alert tone="danger">{errorMessage(settings.error)}</Alert> : null}
          {settings.data ? <SchemaForm pluginId={plugin.id} settings={settings.data} /> : null}
        </div>
      ) : null}
    </div>
  );
}
