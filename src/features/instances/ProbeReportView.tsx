import { CircleCheck, CircleMinus, CircleX } from "lucide-react";
import type { ReactNode } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { messageForCode } from "../../lib/errors";
import { formatDateTime, formatOdooVersion } from "../../lib/format";
import { PROBE_WARNINGS, PROTOCOL_LABELS, TRANSPORT_LABELS, WARNING_ORDER } from "../../lib/labels";
import type { CheckStatus, ProbeReport } from "../../lib/types";

const SKIP_REASONS: Record<string, string> = {
  no_credentials: "sin credenciales",
  no_database: "sin base de datos",
  auth_failed: "requiere autenticación correcta",
};

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

/** Resultado de "Probar conexión". */
export function ProbeReportView({ report }: { report: ProbeReport }) {
  const warnings = WARNING_ORDER.filter((w) => report.warnings.includes(w));
  return (
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
          label="Módulo appex_backup"
          check={report.module}
          okText={`Instalado (API v${report.moduleApiVersion ?? "?"}). Funciona aunque list_db esté deshabilitado.`}
          hint="Necesario para respaldar instancias con list_db = False."
        />
        <CheckRow
          label="Gestor de bases de datos"
          check={report.dbManager}
          okText="Habilitado (list_db = True)."
          hint="Solo se usa si eliges el transporte Gestor de BD."
        />
      </ul>

      {report.recommendedTransport ? (
        <Alert tone="success" title={`Transporte recomendado: ${TRANSPORT_LABELS[report.recommendedTransport]}`}>
          {report.recommendedTransport === "appex_module"
            ? "Usa la API key y no necesita la contraseña maestra."
            : "Envía la contraseña maestra al servidor en cada backup."}
        </Alert>
      ) : (
        <Alert tone="warning" title="No hay un transporte de backup disponible">
          Instala el módulo appex_backup (requiere API key) o habilita list_db y guarda la contraseña maestra.
        </Alert>
      )}

      {warnings.map((warning) => {
        const info = PROBE_WARNINGS[warning];
        return (
          <Alert key={warning} tone={info.severity} title={info.title}>
            {info.detail}
          </Alert>
        );
      })}

      <p className="text-xs text-subtle">
        Probado {formatDateTime(report.checkedAt)} · {report.version.serverVersion}
      </p>
    </div>
  );
}
