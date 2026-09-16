import { useQuery, useQueryClient } from "@tanstack/react-query";
import { CloudUpload, DatabaseBackup, History, Pencil, PlugZap, Plus, Server, Trash2 } from "lucide-react";
import { useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Dialog } from "../../components/Dialog";
import { EmptyState, PageHeader } from "../../components/Layout";
import { ProgressBar, Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatOdooVersion, formatRelative } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import {
  HISTORY_STATUS_LABELS,
  PROTOCOL_LABELS,
  PROTOCOL_PREFERENCE_LABELS,
  TRANSPORT_LABELS,
  TRANSPORT_PREFERENCE_LABELS,
} from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import type { InstanceView, ProbeReport } from "../../lib/types";
import { useBackupJobs } from "../backups/BackupJobsProvider";
import { describeJob } from "../backups/jobs";
import { STATUS_TONES } from "../history/tones";
import { InstanceFormDialog } from "./InstanceFormDialog";
import { ProbeReportView } from "./ProbeReportView";
import { probeRequestForInstance } from "./schema";

type ProbeState = { instance: InstanceView; report: ProbeReport | null; error: string | null };

export function InstancesPage({ onShowHistory }: { onShowHistory: (instanceId: string) => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const jobs = useBackupJobs();
  const instances = useQuery({ queryKey: queryKeys.instances, queryFn: () => ipc.listInstances() });

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
      toast.error(`No se pudo iniciar el backup de ${instance.name}`, errorMessage(err));
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
            description="Registra una instancia Odoo (15 a 19) con su URL y credenciales para empezar a hacer backups."
            action={
              <Button variant="primary" icon={<Plus size={15} />} onClick={openCreate}>
                Nueva instancia
              </Button>
            }
          />
        ) : null}

        {list.length > 0 ? (
          <div className="overflow-x-auto rounded-xl border border-border bg-surface shadow-card">
            <table className="w-full min-w-[820px] text-left text-[13px]">
              <thead className="border-b border-border bg-surface-2/60 text-xs text-muted">
                <tr>
                  <th scope="col" className="px-4 py-2.5 font-medium">Instancia</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Versión</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Protocolo</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Transporte</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Último backup</th>
                  <th scope="col" className="px-4 py-2.5 text-right font-medium">
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
            <Button variant="primary" onClick={() => setProbe(null)}>
              Cerrar
            </Button>
          </>
        }
      >
        {probe && !probe.report && !probe.error ? <Spinner label="Conectando con el servidor…" /> : null}
        {probe?.error ? <Alert tone="danger" title="No se pudo probar la conexión">{probe.error}</Alert> : null}
        {probe?.report ? <ProbeReportView report={probe.report} /> : null}
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
          backup ya descargados y el historial no se borran.
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
}: {
  instance: InstanceView;
  starting: boolean;
  onBackup: () => void;
  onProbe: () => void;
  onEdit: () => void;
  onDelete: () => void;
  onHistory: () => void;
}) {
  const { runningFor } = useBackupJobs();
  const job = runningFor(instance.id);
  const probe = instance.lastProbe;
  const last = instance.lastBackup;

  // Con preferencia automática se muestra lo detectado en la última prueba.
  const autoTransport = instance.transport === "auto" && probe?.recommendedTransport ? probe.recommendedTransport : null;
  const transportText = autoTransport ? TRANSPORT_LABELS[autoTransport] : TRANSPORT_PREFERENCE_LABELS[instance.transport];
  const autoProtocol = instance.protocol === "auto" && probe?.protocol ? probe.protocol : null;
  const protocolText = autoProtocol ? PROTOCOL_LABELS[autoProtocol] : PROTOCOL_PREFERENCE_LABELS[instance.protocol];

  return (
    <tr className="align-middle hover:bg-surface-2/40">
      <td className="max-w-[14rem] py-3 pr-3 pl-4">
        <div className="flex items-center gap-2">
          <p className="truncate font-medium text-fg">{instance.name}</p>
          {instance.uploadToDrive ? (
            <CloudUpload size={13} className="shrink-0 text-subtle" aria-label="Sube a Google Drive" />
          ) : null}
        </div>
        <p className="truncate text-xs text-muted" title={instance.url}>
          {instance.url.replace(/^https?:\/\//, "")} · {instance.database}
        </p>
      </td>
      <td className="px-3 py-3 whitespace-nowrap">
        {probe ? (
          <Badge tone={probe.supported ? "neutral" : "warning"} title={probe.version.serverVersion}>
            {formatOdooVersion(probe.version)}
          </Badge>
        ) : (
          <span className="text-xs text-subtle">Sin probar</span>
        )}
      </td>
      <td className="px-3 py-3 text-muted">
        <span className="whitespace-nowrap">{protocolText}</span>
        {autoProtocol ? <span className="block text-[11px] text-subtle">automático</span> : null}
      </td>
      <td className="max-w-[8.5rem] px-3 py-3 text-muted">
        <span>{transportText}</span>
        {autoTransport ? <span className="block text-[11px] text-subtle">automático</span> : null}
      </td>
      <td className="min-w-[9rem] px-3 py-3">
        {job ? (
          <div className="space-y-1">
            <p className="truncate text-xs text-accent">{describeJob(job).label}</p>
            <ProgressBar value={describeJob(job).percent} />
          </div>
        ) : last ? (
          <div className="flex flex-col items-start gap-0.5">
            <Badge
              tone={STATUS_TONES[last.status]}
              title={last.status === "failed" && last.errorCode ? messageForCode(last.errorCode) : undefined}
            >
              {HISTORY_STATUS_LABELS[last.status]}
            </Badge>
            <span className="text-xs text-muted" title={last.startedAt}>
              {formatRelative(last.finishedAt ?? last.startedAt)}
            </span>
          </div>
        ) : (
          <span className="text-xs text-subtle">Nunca</span>
        )}
      </td>
      <td className="py-3 pr-3 pl-2">
        <div className="flex items-center justify-end gap-0.5">
          <Button
            size="sm"
            variant="primary"
            icon={<DatabaseBackup size={14} />}
            onClick={onBackup}
            loading={starting}
            disabled={Boolean(job)}
            aria-label="Backup ahora"
            title={job ? "Ya hay un backup en curso" : "Backup ahora"}
          >
            Backup
          </Button>
          <IconButton label="Probar conexión" icon={<PlugZap size={15} />} onClick={onProbe} />
          <IconButton label="Ver historial" icon={<History size={15} />} onClick={onHistory} />
          <IconButton label="Editar" icon={<Pencil size={15} />} onClick={onEdit} />
          <IconButton label="Eliminar" tone="danger" icon={<Trash2 size={15} />} onClick={onDelete} disabled={Boolean(job)} />
        </div>
      </td>
    </tr>
  );
}
