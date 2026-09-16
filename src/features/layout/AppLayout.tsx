import { useQueryClient } from "@tanstack/react-query";
import { History, Loader, Lock, Server, Settings } from "lucide-react";
import type { ReactNode } from "react";

import { Logo } from "../../components/Logo";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { useBackupJobs } from "../backups/BackupJobsProvider";
import { BackupJobsPanel } from "../backups/BackupJobsPanel";

export type Page = "instances" | "history" | "settings";

const NAV: { page: Page; label: string; icon: ReactNode }[] = [
  { page: "instances", label: "Instancias", icon: <Server size={16} /> },
  { page: "history", label: "Historial", icon: <History size={16} /> },
  { page: "settings", label: "Ajustes", icon: <Settings size={16} /> },
];

export function AppLayout({
  page,
  onNavigate,
  status,
  children,
}: {
  page: Page;
  onNavigate: (page: Page) => void;
  status: AppStatus;
  children: ReactNode;
}) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const { running, state } = useBackupJobs();

  const lock = async () => {
    try {
      const next = await ipc.lockVault();
      queryClient.removeQueries({ predicate: (query) => query.queryKey[0] !== queryKeys.appStatus[0] });
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
            <p className="text-[11px] text-subtle">Backups de Odoo</p>
          </div>
        </div>
        <nav aria-label="Principal" className="flex-1 space-y-0.5 px-2">
          {NAV.map((item) => {
            const active = item.page === page;
            return (
              <button
                key={item.page}
                type="button"
                onClick={() => onNavigate(item.page)}
                aria-current={active ? "page" : undefined}
                className={`flex w-full items-center gap-2.5 rounded-md px-3 py-2 text-sm transition-colors ${
                  active ? "bg-surface font-medium text-fg shadow-card" : "text-muted hover:bg-surface/60 hover:text-fg"
                }`}
              >
                <span className={active ? "text-accent" : ""}>{item.icon}</span>
                {item.label}
              </button>
            );
          })}
        </nav>
        <div className="space-y-2 border-t border-border px-3 py-3">
          {running.length > 0 ? (
            <p className="flex items-center gap-2 px-1 text-xs text-accent" role="status">
              <Loader size={13} className="animate-spin motion-reduce:animate-none" />
              {running.length === 1 ? "1 backup en curso" : `${running.length} backups en curso`}
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
      <main className={`relative min-w-0 flex-1 overflow-y-auto ${state.order.length > 0 ? "pb-48" : ""}`}>{children}</main>
      <BackupJobsPanel />
    </div>
  );
}
