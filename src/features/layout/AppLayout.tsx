import { useQueryClient } from "@tanstack/react-query";
import { History, Loader, Lock, Puzzle, Server, Settings } from "lucide-react";
import type { ReactNode } from "react";

import { Logo } from "../../components/Logo";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { useBackupJobs } from "../backups/BackupJobsProvider";
import { BackupJobsPanel } from "../backups/BackupJobsPanel";
import { pluginIcon } from "../plugins/icons";
import { useOpenPluginMenu, usePlugins } from "../plugins/usePlugins";
import { coreRoute, pluginMenus, sameRoute, type CorePage, type Route } from "./navigation";

const NAV: { page: CorePage; label: string; icon: ReactNode }[] = [
  { page: "instances", label: "Instancias", icon: <Server size={16} /> },
  { page: "history", label: "Historial", icon: <History size={16} /> },
  { page: "plugins", label: "Plugins", icon: <Puzzle size={16} /> },
  { page: "settings", label: "Ajustes", icon: <Settings size={16} /> },
];

function NavButton({ active, icon, label, onClick, title }: { active: boolean; icon: ReactNode; label: string; onClick: () => void; title?: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-current={active ? "page" : undefined}
      className={`flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors ${
        active ? "bg-surface font-medium text-fg shadow-card" : "text-muted hover:bg-surface/60 hover:text-fg"
      }`}
    >
      <span className={`shrink-0 ${active ? "text-accent" : ""}`}>{icon}</span>
      <span className="min-w-0 truncate">{label}</span>
    </button>
  );
}

export function AppLayout({
  route,
  onNavigate,
  status,
  children,
}: {
  route: Route;
  onNavigate: (route: Route) => void;
  status: AppStatus;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const { running, state } = useBackupJobs();
  const plugins = usePlugins();
  const openMenu = useOpenPluginMenu(onNavigate);
  const sidebarMenus = pluginMenus(plugins.data, "sidebar");
  const isPluginPage = route.kind === "plugin";

  const lock = async () => {
    try {
      const next = await ipc.lockVault();
      queryClient.removeQueries({
        predicate: (query) => query.queryKey[0] !== queryKeys.appStatus[0] && query.queryKey[0] !== queryKeys.plugins[0],
      });
      queryClient.setQueryData(queryKeys.appStatus, next);
    } catch (err) {
      toast.error("No se pudo bloquear la bóveda", errorMessage(err));
    }
  };

  return (
    <div className="flex h-full">
      <aside className="flex w-52 shrink-0 flex-col border-r border-border bg-sidebar">
        <div className="flex items-center gap-2.5 px-4 pt-5 pb-6">
          <Logo size={28} />
          <div className="leading-tight">
            <p className="text-[15px] font-semibold tracking-tight">Odoo Backup Desktop</p>
            <p className="text-[11px] text-subtle">Respaldos de Odoo</p>
          </div>
        </div>
        <nav aria-label="Principal" className="flex-1 space-y-0.5 overflow-y-auto px-2">
          {NAV.map((item) => (
            <NavButton
              key={item.page}
              active={sameRoute(route, coreRoute(item.page))}
              icon={item.icon}
              label={item.label}
              onClick={() => onNavigate(coreRoute(item.page))}
            />
          ))}
          {sidebarMenus.length > 0 ? (
            <div className="pt-4" role="group" aria-labelledby="sidebar-plugins-heading">
              <p id="sidebar-plugins-heading" className="px-3 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">
                Extensiones
              </p>
              {sidebarMenus.map((entry) => {
                const Icon = pluginIcon(entry.menu.icon);
                const active =
                  isPluginPage &&
                  entry.menu.page !== null &&
                  route.pluginId === entry.plugin.id &&
                  route.pageId === entry.menu.page;
                return (
                  <NavButton
                    key={`${entry.plugin.id}:${entry.menu.id}`}
                    active={active}
                    icon={<Icon size={16} />}
                    label={entry.menu.label}
                    title={`${entry.menu.label} · ${entry.plugin.name}`}
                    onClick={() => void openMenu(entry)}
                  />
                );
              })}
            </div>
          ) : null}
        </nav>
        <div className="space-y-2 border-t border-border px-3 py-3">
          {running.length > 0 ? (
            <p className="flex items-center gap-2 px-1 text-xs text-accent" role="status">
              <Loader size={13} className="animate-spin motion-reduce:animate-none" />
              {running.length === 1 ? "1 respaldo en curso" : `${running.length} respaldos en curso`}
            </p>
          ) : null}
          <button
            type="button"
            onClick={lock}
            className="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-[13px] text-muted hover:bg-surface/60 hover:text-fg"
          >
            <Lock size={14} /> Bloquear bóveda
          </button>
          <p className="px-2 text-[11px] text-subtle tabular">v{status.appVersion}</p>
        </div>
      </aside>
      <main
        className={`relative min-w-0 flex-1 ${isPluginPage ? "overflow-hidden" : "overflow-y-auto"} ${
          state.order.length > 0 && !isPluginPage ? "pb-48" : ""
        }`}
      >
        {children}
      </main>
      <BackupJobsPanel />
    </div>
  );
}
