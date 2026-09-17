import { useQuery } from "@tanstack/react-query";
import { ChevronDown, FolderOpen, History as HistoryIcon, RefreshCw } from "lucide-react";
import { Fragment, useId, useMemo, useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button, IconButton } from "../../components/Button";
import { Select } from "../../components/Field";
import { CopyButton, DefinitionList, EmptyState, PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, messageForCode } from "../../lib/errors";
import { formatBytes, formatDateTime, formatDuration, secondsBetween } from "../../lib/format";
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
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
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

  const toggle = (id: string) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

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
        description="Respaldos realizados en este equipo."
        actions={
          <Button icon={<RefreshCw size={14} />} onClick={() => void history.refetch()} loading={history.isFetching && !history.isPending}>
            Actualizar
          </Button>
        }
      />

      <div className="space-y-4 px-page py-section">
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
                ? "Todavía no se ha hecho ningún respaldo con estos filtros."
                : "Ningún registro coincide con el estado seleccionado."
            }
          />
        ) : null}

        {rows.length > 0 ? (
          // Columnas esenciales siempre visibles desde 900×600; SHA-256, Drive, archivo y errores
          // completos viven en la fila de detalles.
          <div className="@container overflow-hidden rounded-card border border-border bg-surface shadow-card">
            <table className="w-full table-fixed text-left text-[13px]">
              <colgroup>
                <col className="w-[9.5rem]" />
                <col />
                <col className="w-[9rem] @3xl:w-[13rem]" />
                <col className="w-[6.5rem]" />
                <col className="w-[5.25rem]" />
              </colgroup>
              <thead className="border-b border-border bg-surface-2/60 text-xs text-muted">
                <tr>
                  <th scope="col" className="py-2.5 pr-3 pl-4 font-medium">Fecha</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Instancia</th>
                  <th scope="col" className="px-3 py-2.5 font-medium">Estado</th>
                  <th scope="col" className="px-3 py-2.5 text-right font-medium">Tamaño</th>
                  <th scope="col" className="py-2.5 pr-4 pl-2">
                    <span className="sr-only">Acciones</span>
                  </th>
                </tr>
              </thead>
              <tbody className="divide-y divide-border">
                {rows.map((entry) => (
                  <HistoryRow
                    key={entry.id}
                    entry={entry}
                    expanded={expanded.has(entry.id)}
                    onToggle={() => toggle(entry.id)}
                    onReveal={() => reveal(entry)}
                  />
                ))}
              </tbody>
            </table>
          </div>
        ) : null}
      </div>
    </>
  );
}

function HistoryRow({
  entry,
  expanded,
  onToggle,
  onReveal,
}: {
  entry: HistoryEntry;
  expanded: boolean;
  onToggle: () => void;
  onReveal: () => void;
}) {
  const detailsId = useId();
  const duration = secondsBetween(entry.startedAt, entry.finishedAt);
  const technical = [entry.transport ? TRANSPORT_LABELS[entry.transport] : null, entry.odooVersion ? `Odoo ${entry.odooVersion}` : null]
    .filter(Boolean)
    .join(" · ");
  const errorText = entry.errorCode ? messageForCode(entry.errorCode) : null;
  const canReveal = entry.status === "success" && Boolean(entry.filePath);

  return (
    <Fragment>
      <tr className={`align-top ${expanded ? "bg-surface-2/40" : "hover:bg-surface-2/40"}`}>
        <td className="py-2.5 pr-3 pl-4 tabular">{formatDateTime(entry.startedAt)}</td>
        <td className="px-3 py-2.5">
          <p className="truncate font-medium" title={entry.instanceName}>
            {entry.instanceName}
          </p>
          {technical ? <p className="hidden truncate text-xs text-subtle @3xl:block">{technical}</p> : null}
        </td>
        <td className="px-3 py-2.5">
          <Badge tone={STATUS_TONES[entry.status]}>{HISTORY_STATUS_LABELS[entry.status]}</Badge>
          {entry.status === "failed" && errorText ? (
            <p className="mt-1 truncate text-xs text-danger" title={errorText}>
              {errorText}
            </p>
          ) : null}
          {entry.drive.status === "failed" ? (
            <p className="mt-1 truncate text-xs text-warning" title={entry.drive.errorMessage ?? undefined}>
              Falló la subida a Drive
            </p>
          ) : null}
        </td>
        <td className="px-3 py-2.5 text-right tabular">
          <p>{formatBytes(entry.sizeBytes)}</p>
          <p className="text-xs text-subtle">{formatDuration(duration)}</p>
        </td>
        <td className="py-2 pr-3 pl-2">
          <div className="flex items-center justify-end gap-0.5">
            <IconButton
              label={canReveal ? "Mostrar en carpeta" : "Sin archivo local"}
              icon={<FolderOpen size={15} />}
              onClick={onReveal}
              disabled={!canReveal}
            />
            <button
              type="button"
              aria-expanded={expanded}
              aria-controls={detailsId}
              aria-label={expanded ? "Ocultar detalles" : "Mostrar detalles"}
              title={expanded ? "Ocultar detalles" : "Mostrar detalles"}
              onClick={onToggle}
              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg"
            >
              <ChevronDown size={15} aria-hidden="true" className={`transition-transform motion-reduce:transition-none ${expanded ? "rotate-180" : ""}`} />
            </button>
          </div>
        </td>
      </tr>
      <tr id={detailsId} hidden={!expanded} className="bg-surface-2/40">
        <td colSpan={5} className="px-4 pt-1 pb-3">
          <DefinitionList
            items={[
              { label: "Inicio", value: <span className="tabular">{formatDateTime(entry.startedAt)}</span> },
              { label: "Fin", value: <span className="tabular">{entry.finishedAt ? formatDateTime(entry.finishedAt) : "—"}</span> },
              ...(technical ? [{ label: "Método", value: technical }] : []),
              {
                label: "Archivo",
                value: entry.filePath ? (
                  <span className="flex min-w-0 items-center gap-1">
                    <span className="truncate font-mono text-xs" title={entry.filePath}>
                      {entry.filePath}
                    </span>
                    <CopyButton value={entry.filePath} label="Copiar ruta" />
                  </span>
                ) : (
                  "—"
                ),
              },
              {
                label: "SHA-256",
                value: entry.sha256 ? (
                  <span className="flex min-w-0 items-start gap-1">
                    <span className="font-mono text-xs break-all">{entry.sha256}</span>
                    <CopyButton value={entry.sha256} label="Copiar SHA-256" />
                  </span>
                ) : (
                  "—"
                ),
              },
              {
                label: "Google Drive",
                value: (
                  <span className="flex flex-wrap items-center gap-2">
                    <Badge tone={DRIVE_TONES[entry.drive.status]}>{DRIVE_STATUS_LABELS[entry.drive.status]}</Badge>
                    {entry.drive.errorMessage ? <span className="text-xs text-muted">{entry.drive.errorMessage}</span> : null}
                  </span>
                ),
              },
              ...(entry.status === "failed" && errorText
                ? [
                    {
                      label: "Error",
                      value: (
                        <span className="text-danger">
                          {errorText}
                          {entry.errorMessage ? <span className="block text-xs text-muted">{entry.errorMessage}</span> : null}
                        </span>
                      ),
                    },
                  ]
                : []),
            ]}
          />
        </td>
      </tr>
    </Fragment>
  );
}
