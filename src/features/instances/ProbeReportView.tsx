import { ChevronRight, CircleCheck, CircleMinus, CircleX } from "lucide-react";
import type { ReactNode } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { messageForCode } from "../../lib/errors";
import { formatDateTime, formatOdooVersion } from "../../lib/format";
import { PROBE_WARNINGS, PROTOCOL_LABELS, WARNING_ORDER } from "../../lib/labels";
import type { CheckStatus, ProbeReport, ProbeWarning } from "../../lib/types";
import type { Guidance } from "./readiness";

const SKIP_REASONS: Record<string, string> = {
  no_credentials: "sin credenciales",
  no_database: "sin base de datos",
  auth_failed: "requiere autenticación correcta",
};

/** Advertencias que se muestran siempre; el resto queda en los detalles. */
const PROMINENT_WARNINGS: ProbeWarning[] = ["insecure_http", "unsupported_version"];

function CheckRow({ label, check, okText, hint }: { label: string; check: CheckStatus; okText: string; hint?: ReactNode }) {
  let icon: ReactNode;
  let text: ReactNode;
  if (check.status === "ok") {
    icon = <CircleCheck size={16} className="text-success" aria-label="Correcto" />;
    text = okText;
  } else if (check.status === "failed") {
    icon = <CircleX size={16} className="text-danger" aria-label="Falló" />;
    text = (
      <>
        {messageForCode(check.code)}
        {check.message ? (
          <span className="mt-0.5 block font-mono text-[11px] break-all text-subtle" title={check.message}>
            {check.message}
          </span>
        ) : null}
      </>
    );
  } else {
    icon = <CircleMinus size={16} className="text-subtle" aria-label="Omitido" />;
    text = <span className="text-muted">Omitido ({SKIP_REASONS[check.reason] ?? check.reason})</span>;
  }
  return (
    <li className="flex items-start gap-2.5 py-2">
      <span className="mt-0.5 shrink-0">{icon}</span>
      <div className="min-w-0 flex-1 text-[13px]">
        <p className="font-medium">{label}</p>
        <div className="text-muted">{text}</div>
        {hint && check.status !== "ok" ? <p className="mt-0.5 text-xs text-subtle">{hint}</p> : null}
      </div>
    </li>
  );
}

/** "Odoo 18.0 · XML-RPC · BD cliente1": lo detectado, en una línea. */
export function probeFacts(report: ProbeReport): string {
  return [
    `Odoo ${formatOdooVersion(report.version)}`,
    report.protocol ? PROTOCOL_LABELS[report.protocol] : null,
    report.database ? `BD ${report.database}` : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/**
 * Resultado de "Probar conexión": primero el siguiente paso (`guidance`) y las advertencias graves;
 * después las comprobaciones. Con `collapseDetails` las comprobaciones quedan plegadas.
 */
export function ProbeReportView({
  report,
  guidance,
  action,
  collapseDetails = false,
}: {
  report: ProbeReport;
  guidance: Guidance;
  /** Botón que aplica `guidance.action`. */
  action?: ReactNode;
  collapseDetails?: boolean;
}) {
  const warnings = WARNING_ORDER.filter((w) => report.warnings.includes(w));
  const prominent = warnings.filter((w) => PROMINENT_WARNINGS.includes(w));
  const secondary = warnings.filter((w) => !PROMINENT_WARNINGS.includes(w));

  const details = (
    <div className="space-y-3">
      <div className="flex flex-wrap items-center gap-2">
        <Badge tone={report.supported ? "accent" : "warning"}>Odoo {formatOdooVersion(report.version)}</Badge>
        {report.protocol ? <Badge tone="neutral">{PROTOCOL_LABELS[report.protocol]}</Badge> : null}
        {report.database ? <Badge tone="neutral">BD: {report.database}</Badge> : null}
        <Badge tone={report.https ? "success" : "danger"}>{report.https ? "HTTPS" : "HTTP sin cifrar"}</Badge>
        {report.uid !== null ? <Badge tone="neutral">uid {report.uid}</Badge> : null}
      </div>

      <ul className="divide-y divide-border rounded-lg border border-border px-3">
        <CheckRow label="Autenticación" check={report.auth} okText="Credenciales válidas." />
        <CheckRow
          label="Módulo obd_backup"
          check={report.module}
          okText={`Instalado (API v${report.moduleApiVersion ?? "?"}). Funciona aunque list_db esté deshabilitado.`}
          hint="Necesario para respaldar instancias con list_db = False."
        />
        <CheckRow
          label="Gestor de bases de datos"
          check={report.dbManager}
          okText="Habilitado (list_db = True)."
          hint="Solo se usa con el método Gestor de BD."
        />
      </ul>

      {secondary.map((warning) => (
        <WarningAlert key={warning} warning={warning} />
      ))}

      <p className="text-xs text-subtle">
        Probado {formatDateTime(report.checkedAt)} · {report.version.serverVersion}
      </p>
    </div>
  );

  return (
    <div className="space-y-3">
      <Alert tone={guidance.tone} title={guidance.title}>
        {guidance.detail}
        {action ? <div className="mt-2">{action}</div> : null}
      </Alert>

      {prominent.map((warning) => (
        <WarningAlert key={warning} warning={warning} />
      ))}

      {collapseDetails ? (
        <details className="group rounded-lg border border-border">
          <summary className="flex cursor-pointer list-none items-center gap-2 rounded-lg px-3 py-2 text-[13px] hover:bg-surface-2 [&::-webkit-details-marker]:hidden">
            <ChevronRight size={14} className="shrink-0 text-muted transition-transform group-open:rotate-90" aria-hidden="true" />
            <span className="shrink-0 font-medium">Detalles de la prueba</span>
            <span className="min-w-0 truncate text-muted">{probeFacts(report)}</span>
          </summary>
          <div className="border-t border-border px-3 py-3">{details}</div>
        </details>
      ) : (
        details
      )}
    </div>
  );
}

function WarningAlert({ warning }: { warning: ProbeWarning }) {
  const info = PROBE_WARNINGS[warning];
  return (
    <Alert tone={info.severity} title={info.title}>
      {info.detail}
    </Alert>
  );
}
