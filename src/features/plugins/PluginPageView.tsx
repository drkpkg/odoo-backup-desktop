import { ArrowLeft, Puzzle, Settings } from "lucide-react";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { EmptyState } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { errorMessage } from "../../lib/errors";
import { coreRoute, pluginRoute, type Route } from "../layout/navigation";
import { pluginIcon } from "./icons";
import { PluginFrame } from "./PluginFrame";
import { usePlugins } from "./usePlugins";

/** Página de un plugin dentro de la ventana principal. */
export function PluginPageView({
  pluginId,
  pageId,
  params,
  appVersion,
  onNavigate,
  onOpenSettings,
}: {
  pluginId: string;
  pageId: string;
  params: Record<string, unknown>;
  appVersion: string;
  onNavigate: (route: Route) => void;
  onOpenSettings: (pluginId: string) => void;
}) {
  const plugins = usePlugins();

  if (plugins.isPending) return <div className="p-6"><Spinner label="Cargando plugin…" /></div>;
  if (plugins.isError) return <div className="p-6"><Alert tone="danger">{errorMessage(plugins.error)}</Alert></div>;

  const plugin = plugins.data.find((p) => p.id === pluginId && p.status === "enabled");
  const page = plugin?.pages.find((p) => p.id === pageId);

  if (!plugin || !page) {
    return (
      <EmptyState
        icon={<Puzzle size={22} />}
        title="Página no disponible"
        description="El plugin fue desactivado, eliminado o ya no declara esta página."
        action={
          <Button icon={<ArrowLeft size={14} />} onClick={() => onNavigate(coreRoute("plugins"))}>
            Ir a Plugins
          </Button>
        }
      />
    );
  }

  const menuIcon = plugin.menus.find((menu) => menu.page === page.id)?.icon;
  const Icon = pluginIcon(menuIcon);

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex h-11 shrink-0 items-center justify-between gap-3 border-b border-border bg-surface px-4">
        <div className="flex min-w-0 items-center gap-2">
          <Icon size={15} className="shrink-0 text-accent" aria-hidden="true" />
          <h1 className="truncate text-sm font-semibold">{page.title}</h1>
          <span className="truncate text-xs text-subtle">· {plugin.name}</span>
        </div>
        {plugin.hasSettings ? (
          <Button size="sm" variant="ghost" icon={<Settings size={13} />} onClick={() => onOpenSettings(plugin.id)}>
            Ajustes
          </Button>
        ) : null}
      </div>
      <div className="min-h-0 flex-1">
        <PluginFrame
          plugin={plugin}
          path={page.path}
          title={`${page.title} · ${plugin.name}`}
          surface="page"
          params={params}
          appVersion={appVersion}
          onNavigate={(nextPageId, nextParams) => onNavigate(pluginRoute(plugin.id, nextPageId, nextParams))}
          onOpenSettings={() => onOpenSettings(plugin.id)}
        />
      </div>
    </div>
  );
}
