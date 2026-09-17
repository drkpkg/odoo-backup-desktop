import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CloudUpload, DatabaseBackup, History, Pencil, PlugZap, Plus, Server, Trash2 } from "lucide-react";
import { useState } from "react";

import { ActionMenu, type ActionMenuSection } from "../../components/ActionMenu";
import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Dialog } from "../../components/Dialog";
import { EmptyState, PageHeader } from "../../components/Layout";
import { ProgressBar, Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatRelative } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { HISTORY_STATUS_LABELS } from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import type { InstanceView, ProbeReport } from "../../lib/types";
import { useBackupJobs } from "../backups/BackupJobsProvider";
import { describeJob } from "../backups/jobs";
import { pluginMenus, type PluginMenuEntry, type Route } from "../layout/navigation";
import { pluginIcon } from "../plugins/icons";
import { useOpenPluginMenu, usePlugins } from "../plugins/usePlugins";
import { STATUS_TONES } from "../history/tones";
import { connectionState } from "./connection";
import { InstanceFormDialog } from "./InstanceFormDialog";
import { ProbeReportView } from "./ProbeReportView";
import { assessReadiness, probeGuidance } from "./readiness";
import { probeRequestForInstance } from "./schema";

type ProbeState = { instance: InstanceView; report: ProbeReport | null; error: string | null };

export function InstancesPage({
  onShowHistory,
  onNavigate,
}: {
  onShowHistory: (instanceId: string) => void;
  onNavigate: (route: Route) => void;
}) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const jobs = useBackupJobs();
  const instances = useQuery({ queryKey: queryKeys.instances, queryFn: () => ipc.listInstances() });
  const plugins = usePlugins();
  const pluginActions = pluginMenus(plugins.data, "instance_actions");
  const openPluginMenu = useOpenPluginMenu(onNavigate);

  const [formOpen, setFormOpen] = useState(false);
  const [editing, setEditing] = useState<InstanceView | null>(null);
  const [deleting, setDeleting] = useState<InstanceView | null>(null);
  const [probe, setProbe] = useState<ProbeState | null>(null);
  const [starting, setStarting] = useState<string | null>(null);

  const openCreate = () => {
    setEditing(null);
    setFormOpen(true);
  };

  const openEdit = (instance: InstanceView) => {
    setEditing(instance);
    setFormOpen(true);
  };

  const startBackup = async (instance: InstanceView) => {
    setStarting(instance.id);
    try {
      await jobs.start(instance.id);
    } catch (err) {
      toast.error(`No se pudo iniciar el respaldo de ${instance.name}`, errorMessage(err));
    } finally {
      setStarting(null);
    }
  };

  const runProbe = async (instance: InstanceView) => {
    setProbe({ instance, report: null, error: null });
    try {
      const report = await ipc.probeInstance(probeRequestForInstance(instance));
      setProbe((current) => (current?.instance.id === instance.id ? { instance, report, error: null } : current));
      void queryClient.invalidateQueries({ queryKey: queryKeys.instances });
    } catch (err) {
      setProbe((current) =>
        current?.instance.id === instance.id ? { instance, report: null, error: errorMessage(err) } : current,
      );
    }
  };

  const list = instances.data ?? [];
  const probeReadiness = probe?.report ? probeGuidance(assessReadiness(probe.report, probe.instance), probe.instance) : null;

  const editFromProbe = ({ instance, report }: ProbeState) => {
    // La lista puede no haberse recargado todavía: abrir con la prueba recién hecha.
    const current = list.find((item) => item.id === instance.id) ?? instance;
    setProbe(null);
    openEdit(report ? { ...current, lastProbe: report } : current);
  };

  return (
    <>
      <PageHeader
        title="Instancias"
        description="Instancias Odoo registradas. Las credenciales están cifradas en la bóveda."
        actions={
          <Button variant="primary" icon={<Plus size={15} />} onClick={openCreate}>
            Nueva instancia
          </Button>
        }
      />

      <div className="px-6 py-5">
        {instances.isPending ? <Spinner label="Cargando instancias…" /> : null}
        {instances.isError ? <Alert tone="danger">{errorMessage(instances.error)}</Alert> : null}

        {instances.isSuccess && list.length === 0 ? (
          <EmptyState
            icon={<Server size={22} />}
            title="Aún no hay instancias"
            description="Registra una instancia Odoo (15 a 19) con su URL y credenciales para empezar a hacer respaldos."
            action={
              <Button variant="primary" icon={<Plus size={15} />} onClick={openCreate}>
                Nueva instancia
              </Button>
            }
          />
        ) : null}

        {list.length > 0 ? (
          // `table-fixed` + anchos por columna: sin scroll horizontal desde 900×600. El detalle técnico
          // de la conexión solo aparece cuando el contenedor es ancho (container query).
          <div className="@container overflow-hidden rounded-xl border border-border bg-surface shadow-card">
            <table className="w-full table-fixed text-left text-[13px]">
              <colgroup>
                <col />
                <col className="w-[9.75rem] @3xl:w-[17rem]" />
                <col className="w-[8.5rem] @3xl:w-[11rem]" />
                <col className="w-[8.75rem]" />
              </colgroup>
              <thead className="border-b border-border bg-surface-2/60 text-xs text-muted">
                <tr>
                  <th scope="col" className="py-2.5 pr-3 pl-4 font-medium">Instancia</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Conexión</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Último respaldo</th>
                  <th scope="col" className="py-2.5 pr-4 pl-2 text-right font-medium">
                    <span className="sr-only">Acciones</span>
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {list.map((instance) => (
                  <InstanceRow
                    key={instance.id}
                    instance={instance}
                    starting={starting === instance.id}
                    onBackup={() => startBackup(instance)}
                    onProbe={() => runProbe(instance)}
                    onEdit={() => openEdit(instance)}
                    onDelete={() => setDeleting(instance)}
                    onHistory={() => onShowHistory(instance.id)}
                    pluginActions={pluginActions}
                    onPluginAction={(entry) => void openPluginMenu(entry, instance)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </div>

      <InstanceFormDialog open={formOpen} instance={editing} onClose={() => setFormOpen(false)} />

      <Dialog
        open={probe !== null}
        onClose={() => setProbe(null)}
        title={probe ? `Prueba de conexión · ${probe.instance.name}` : "Prueba de conexión"}
        description={probe?.instance.url}
        footer={
          <>
            {probe ? (
              <Button onClick={() => runProbe(probe.instance)} disabled={!probe.report && !probe.error} icon={<PlugZap size={14} />}>
                Repetir
              </Button>
            ) : null}
            {probeReadiness && probeReadiness.tone !== "success" && probe ? (
              <Button icon={<Pencil size={14} />} onClick={() => editFromProbe(probe)}>
                Editar instancia
              </Button>
            ) : null}
            <Button variant="primary" onClick={() => setProbe(null)}>
              Cerrar
            </Button>
          </>
        }
      >
        {probe && !probe.report && !probe.error ? <Spinner label="Conectando con el servidor…" /> : null}
        {probe?.error ? <Alert tone="danger" title="No se pudo probar la conexión">{probe.error}</Alert> : null}
        {probe?.report && probeReadiness ? <ProbeReportView report={probe.report} guidance={probeReadiness} /> : null}
      </Dialog>

      <ConfirmDialog
        open={deleting !== null}
        title="Eliminar instancia"
        confirmLabel="Eliminar"
        onClose={() => setDeleting(null)}
        onConfirm={async () => {
          if (!deleting) return;
          await ipc.deleteInstance(deleting.id);
          await queryClient.invalidateQueries({ queryKey: queryKeys.instances });
          toast.success("Instancia eliminada", deleting.name);
        }}
      >
        <p>
          Se eliminará <strong className="text-fg">{deleting?.name}</strong> y sus credenciales de la bóveda. Los archivos de
          respaldo ya descargados y el historial no se borran.
        </p>
      </ConfirmDialog>
    </>
  );
}

function InstanceRow({
  instance,
  starting,
  onBackup,
  onProbe,
  onEdit,
  onDelete,
  onHistory,
  pluginActions,
  onPluginAction,
}: {
  instance: InstanceView;
  starting: boolean;
  onBackup: () => void;
  onProbe: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onHistory: () => void;
  pluginActions: PluginMenuEntry[];
  onPluginAction: (entry: PluginMenuEntry) => void;
}) {
  const { runningFor } = useBackupJobs();
  const job = runningFor(instance.id);
  const last = instance.lastBackup;
  const connection = connectionState(instance);
  const runningReason = "Espera a que termine el respaldo en curso";

  const sections: ActionMenuSection[] = [
    {
      id: "instance",
      items: [
        { id: "probe", label: "Probar conexión", icon: <PlugZap size={14} />, onSelect: onProbe },
        { id: "history", label: "Ver historial", icon: <History size={14} />, onSelect: onHistory },
        { id: "edit", label: "Editar", icon: <Pencil size={14} />, onSelect: onEdit },
      ],
    },
    {
      id: "plugins",
      title: "Extensiones",
      items: pluginActions.map((entry) => {
        const Icon = pluginIcon(entry.menu.icon);
        return {
          id: `${entry.plugin.id}:${entry.menu.id}`,
          label: entry.menu.label,
          hint: entry.plugin.name,
          icon: <Icon size={14} />,
          onSelect: () => onPluginAction(entry),
        };
      }),
    },
    {
      id: "danger",
      items: [
        {
          id: "delete",
          label: "Eliminar",
          icon: <Trash2 size={14} />,
          tone: "danger",
          disabled: Boolean(job),
          disabledReason: runningReason,
          onSelect: onDelete,
        },
      ],
    },
  ];

  return (
    <tr className="align-middle hover:bg-surface-2/40">
      <td className="py-3 pr-3 pl-4">
        <div className="flex min-w-0 items-center gap-2">
          <p className="truncate font-medium text-fg" title={instance.name}>
            {instance.name}
          </p>
          {instance.uploadToDrive ? (
            <CloudUpload size={13} className="shrink-0 text-subtle" role="img" aria-label="Sube a Google Drive" />
          ) : null}
        </div>
        <p className="truncate text-xs text-muted" title={`${instance.url} · ${instance.database}`}>
          {instance.url.replace(/^https?:\/\//, "").replace(/\/$/, "")} · {instance.database}
        </p>
      </td>
      <td className="px-3 py-3">
        <p className="flex min-w-0 items-center gap-1.5" title={connection.reason}>
          <span className={`h-2 w-2 shrink-0 rounded-full ${DOT_TONES[connection.tone]}`} aria-hidden="true" />
          <span className="truncate font-medium text-fg">{connection.label}</span>
          <span className="sr-only">. {connection.reason}</span>
        </p>
        {connection.version ? (
          <p className="truncate text-xs text-muted">
            Odoo {connection.version}
            {connection.technical ? <span className="hidden @3xl:inline"> · {connection.technical}</span> : null}
          </p>
        ) : null}
      </td>
      <td className="px-3 py-3">
        {job ? (
          <div className="min-w-0 space-y-1">
            <p className="truncate text-xs text-accent" title={describeJob(job).label}>
              {describeJob(job).label}
            </p>
            <ProgressBar value={describeJob(job).percent} label={`Progreso del respaldo de ${instance.name}`} />
          </div>
        ) : last ? (
          <div className="flex min-w-0 flex-col items-start gap-0.5">
            <Badge
              tone={STATUS_TONES[last.status]}
              title={last.status === "failed" && last.errorCode ? messageForCode(last.errorCode) : undefined}
            >
              {HISTORY_STATUS_LABELS[last.status]}
            </Badge>
            <span className="truncate text-xs text-muted" title={last.startedAt}>
              {formatRelative(last.finishedAt ?? last.startedAt)}
            </span>
          </div>
        ) : (
          <span className="text-xs text-subtle">Nunca</span>
        )}
      </td>
      <td className="py-3 pr-3 pl-2">
        <div className="flex items-center justify-end gap-1">
          <Button
            size="sm"
            variant="primary"
            icon={<DatabaseBackup size={14} aria-hidden="true" />}
            onClick={onBackup}
            loading={starting}
            disabled={Boolean(job)}
            aria-label={`Respaldar ${instance.name}`}
            title={job ? "Ya hay un respaldo en curso" : "Respaldar ahora"}
          >
            Respaldar
          </Button>
          <ActionMenu label={`Más acciones para ${instance.name}`} sections={sections} />
        </div>
      </td>
    </tr>
  );
}

const DOT_TONES: Record<ReturnType<typeof connectionState>["tone"], string> = {
  neutral: "bg-subtle",
  accent: "bg-accent",
  info: "bg-info",
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
};
