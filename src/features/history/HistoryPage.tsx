import { useQuery } from "@tanstack/react-query";
import { FolderOpen, History as HistoryIcon, RefreshCw } from "lucide-react";
import { useMemo, useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Select } from "../../components/Field";
import { CopyButton, EmptyState, PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatBytes, formatDateTime, formatDuration, secondsBetween, shortHash } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { DRIVE_STATUS_LABELS, HISTORY_STATUS_LABELS, TRANSPORT_LABELS } from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import type { HistoryEntry, HistoryStatus } from "../../lib/types";
import { DRIVE_TONES, STATUS_TONES } from "./tones";

const HISTORY_LIMIT = 500;

type StatusFilter = "all" | HistoryStatus;

export function HistoryPage({
  instanceId,
  onInstanceChange,
}: {
  instanceId: string | null;
  onInstanceChange: (instanceId: string | null) => void;
}) {
  const toast = useToast();
  const [statusFilter, setStatusFilter] = useState<StatusFilter>("all");
  const instances = useQuery({ queryKey: queryKeys.instances, queryFn: () => ipc.listInstances() });
  const history = useQuery({
    queryKey: queryKeys.history(instanceId),
    queryFn: () => ipc.listHistory({ instanceId: instanceId ?? undefined, limit: HISTORY_LIMIT }),
    staleTime: 5_000,
  });

  const rows = useMemo(
    () => (history.data ?? []).filter((entry) => statusFilter === "all" || entry.status === statusFilter),
    [history.data, statusFilter],
  );

  const reveal = async (entry: HistoryEntry) => {
    try {
      await ipc.revealBackup(entry.id);
    } catch (err) {
      toast.error("No se pudo abrir la carpeta", errorMessage(err));
    }
  };

  return (
    <>
      <PageHeader
        title="Historial"
        description="Backups realizados en este equipo."
        actions={
          <Button icon={<RefreshCw size={14} />} onClick={() => void history.refetch()} loading={history.isFetching && !history.isPending}>
            Actualizar
          </Button>
        }
      />

      <div className="space-y-4 px-6 py-5">
        <div className="flex flex-wrap items-end gap-3">
          <label className="space-y-1 text-[13px]">
            <span className="block font-medium">Instancia</span>
            <Select
              value={instanceId ?? ""}
              onChange={(e) => onInstanceChange(e.target.value || null)}
              className="w-64"
            >
              <option value="">Todas las instancias</option>
              {(instances.data ?? []).map((instance) => (
                <option key={instance.id} value={instance.id}>
                  {instance.name}
                </option>
              ))}
            </Select>
          </label>
          <label className="space-y-1 text-[13px]">
            <span className="block font-medium">Estado</span>
            <Select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value as StatusFilter)} className="w-44">
              <option value="all">Todos</option>
              {(Object.keys(HISTORY_STATUS_LABELS) as HistoryStatus[]).map((status) => (
                <option key={status} value={status}>
                  {HISTORY_STATUS_LABELS[status]}
                </option>
              ))}
            </Select>
          </label>
          {history.data ? (
            <p className="pb-2 text-xs text-muted tabular">
              {rows.length} de {history.data.length} registros
            </p>
          ) : null}
        </div>

        {history.isPending ? <Spinner label="Cargando historial…" /> : null}
        {history.isError ? <Alert tone="danger">{errorMessage(history.error)}</Alert> : null}

        {history.isSuccess && rows.length === 0 ? (
          <EmptyState
            icon={<HistoryIcon size={22} />}
            title="Sin registros"
            description={
              history.data.length === 0
                ? "Todavía no se ha hecho ningún backup con estos filtros."
                : "Ningún registro coincide con el estado seleccionado."
            }
          />
        ) : null}

        {rows.length > 0 ? (
          <div className="overflow-x-auto rounded-xl border border-border bg-surface shadow-card">
            <table className="w-full min-w-[820px] text-left text-[13px]">
              <thead className="border-b border-border bg-surface-2/60 text-xs text-muted">
                <tr>
                  <th scope="col" className="py-2.5 pr-3 pl-4 font-medium">Fecha</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Instancia</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Estado</th>
                  <th scope="col" className="px-3 py-2.5 text-right font-medium">Tamaño · duración</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">SHA-256</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Drive</th>
                  <th scope="col" className="py-2.5 pr-4 pl-2">
                    <span className="sr-only">Acciones</span>
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {rows.map((entry) => (
                  <HistoryRow key={entry.id} entry={entry} onReveal={() => reveal(entry)} />
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </div>
    </>
  );
}

function HistoryRow({ entry, onReveal }: { entry: HistoryEntry; onReveal: () => void }) {
  const duration = secondsBetween(entry.startedAt, entry.finishedAt);
  const subtitle = [entry.transport ? TRANSPORT_LABELS[entry.transport] : null, entry.odooVersion ? `Odoo ${entry.odooVersion}` : null]
    .filter(Boolean)
    .join(" · ");
  return (
    <tr className="align-top hover:bg-surface-2/40">
      <td className="py-2.5 pr-3 pl-4 whitespace-nowrap tabular">{formatDateTime(entry.startedAt)}</td>
      <td className="max-w-[13rem] px-3 py-2.5">
        <p className="truncate font-medium" title={entry.instanceName}>
          {entry.instanceName}
        </p>
        {subtitle ? <p className="truncate text-xs text-subtle">{subtitle}</p> : null}
      </td>
      <td className="w-[13rem] max-w-[13rem] px-3 py-2.5">
        <Badge tone={STATUS_TONES[entry.status]}>{HISTORY_STATUS_LABELS[entry.status]}</Badge>
        {entry.status === "failed" && entry.errorCode ? (
          <p className="mt-1 line-clamp-2 text-xs text-danger" title={entry.errorMessage ?? messageForCode(entry.errorCode)}>
            {messageForCode(entry.errorCode)}
          </p>
        ) : null}
      </td>
      <td className="px-3 py-2.5 text-right whitespace-nowrap tabular">
        <p>{formatBytes(entry.sizeBytes)}</p>
        <p className="text-xs text-subtle">{formatDuration(duration)}</p>
      </td>
      <td className="px-3 py-2.5 whitespace-nowrap">
        {entry.sha256 ? (
          <span className="inline-flex items-center gap-1 font-mono text-xs text-muted">
            <span title={entry.sha256}>{shortHash(entry.sha256)}</span>
            <CopyButton value={entry.sha256} label="Copiar SHA-256" />
          </span>
        ) : (
          <span className="text-subtle">—</span>
        )}
      </td>
      <td className="max-w-[9rem] px-3 py-2.5">
        <Badge tone={DRIVE_TONES[entry.drive.status]} title={entry.drive.errorMessage ?? undefined}>
          {DRIVE_STATUS_LABELS[entry.drive.status]}
        </Badge>
      </td>
      <td className="py-2.5 pr-4 pl-2 text-right">
        <IconButton
          label="Mostrar en carpeta"
          icon={<FolderOpen size={15} />}
          onClick={onReveal}
          disabled={entry.status !== "success" || !entry.filePath}
        />
      </td>
    </tr>
  );
}
