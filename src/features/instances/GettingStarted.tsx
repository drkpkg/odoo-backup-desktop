import { Check, ChevronRight, Cloud, DatabaseBackup, FolderOpen, PlugZap, Plus, Wrench, X } from "lucide-react";
import { useId, useState, type ReactNode } from "react";

import { Button, IconButton } from "../../components/Button";
import type { InstanceView } from "../../lib/types";
import { ONBOARDING_STEPS, type OnboardingNext, type OnboardingState, type OnboardingStepId } from "./onboarding";

export type GettingStartedActions = {
  onAddInstance: () => void;
  onProbe: (instance: InstanceView) => void;
  onEdit: (instance: InstanceView) => void;
  onBackup: (instance: InstanceView) => void;
  onShowProgress: (instance: InstanceView) => void;
  onChangeFolder: () => void;
  onOpenDrive: () => void;
};

const STEP_TITLES: Record<OnboardingStepId, string> = {
  add_instance: "Añade una instancia",
  probe: "Prueba la conexión",
  folder: "Revisa dónde se guardan los respaldos",
  first_backup: "Haz tu primer respaldo",
};

function stepDescription(step: OnboardingStepId, downloadDir: string | null): ReactNode {
  switch (step) {
    case "add_instance":
      return "URL, base de datos y credenciales de una instancia Odoo 15 a 19.";
    case "probe":
      return "Detecta la versión de Odoo y qué método de respaldo puedes usar.";
    case "folder":
      return downloadDir ? (
        <span className="font-mono text-[12px] break-all">{downloadDir}</span>
      ) : (
        "Se usa una carpeta por defecto; puedes cambiarla cuando quieras."
      );
    case "first_backup":
      return "Descarga un .zip validado de la base de datos (y su filestore).";
  }
}

/** Botón y texto del siguiente paso. */
function nextAction(next: OnboardingNext, actions: GettingStartedActions): { text: ReactNode; button: ReactNode } {
  switch (next.kind) {
    case "add_instance":
      return {
        text: "Empieza registrando tu primera instancia Odoo.",
        button: (
          <Button variant="primary" icon={<Plus size={15} />} onClick={actions.onAddInstance}>
            Añadir primera instancia
          </Button>
        ),
      };
    case "probe":
      return {
        text: (
          <>
            Prueba la conexión de <strong className="font-medium text-fg">{next.instance.name}</strong>.
          </>
        ),
        button: (
          <Button variant="primary" icon={<PlugZap size={14} />} onClick={() => actions.onProbe(next.instance)}>
            Probar conexión
          </Button>
        ),
      };
    case "fix":
      return {
        text: (
          <>
            <strong className="font-medium text-fg">{next.instance.name}</strong>: {next.reason}
          </>
        ),
        button: (
          <Button variant="primary" icon={<Wrench size={14} />} onClick={() => actions.onEdit(next.instance)}>
            Revisar instancia
          </Button>
        ),
      };
    case "backup":
      return {
        text: (
          <>
            <strong className="font-medium text-fg">{next.instance.name}</strong> está lista: haz su primer respaldo.
          </>
        ),
        button: (
          <Button variant="primary" icon={<DatabaseBackup size={14} />} onClick={() => actions.onBackup(next.instance)}>
            Respaldar ahora
          </Button>
        ),
      };
    case "in_progress":
      return {
        text: (
          <>
            Respaldando <strong className="font-medium text-fg">{next.instance.name}</strong>…
          </>
        ),
        button: <Button onClick={() => actions.onShowProgress(next.instance)}>Ver progreso</Button>,
      };
  }
}

function StepList({ state, downloadDir, actions }: { state: OnboardingState; downloadDir: string | null; actions: GettingStartedActions }) {
  const current = state.next?.kind === "add_instance" ? "add_instance" : state.next?.kind === "probe" || state.next?.kind === "fix" ? "probe" : "first_backup";
  return (
    <ol className="space-y-1">
      {ONBOARDING_STEPS.map((step, index) => {
        const done = state.done[step];
        const isCurrent = !done && step === current;
        return (
          <li key={step} className={`flex items-start gap-3 rounded-lg px-2 py-2 ${isCurrent ? "bg-accent-soft/60" : ""}`}>
            <span
              className={`mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full text-xs font-semibold ${
                done ? "bg-success-soft text-success" : isCurrent ? "bg-accent text-accent-fg" : "bg-surface-2 text-muted"
              }`}
              aria-hidden="true"
            >
              {done ? <Check size={13} strokeWidth={3} /> : index + 1}
            </span>
            <div className="min-w-0 flex-1">
              <p className={`text-sm ${done ? "text-muted" : "font-medium text-fg"}`}>
                {STEP_TITLES[step]}
                <span className="sr-only">{done ? " (hecho)" : isCurrent ? " (siguiente paso)" : " (pendiente)"}</span>
              </p>
              <p className="text-xs text-muted">{stepDescription(step, downloadDir)}</p>
            </div>
            {step === "folder" ? (
              <Button size="sm" variant="ghost" icon={<FolderOpen size={14} />} onClick={actions.onChangeFolder}>
                Cambiar
              </Button>
            ) : null}
          </li>
        );
      })}
    </ol>
  );
}

function DriveHint({ onOpenDrive }: { onOpenDrive: () => void }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-2 border-t border-border pt-3 text-xs text-muted">
      <span className="flex items-center gap-2">
        <Cloud size={14} className="shrink-0 text-subtle" aria-hidden="true" />
        Opcional: sube cada respaldo a Google Drive automáticamente.
      </span>
      <Button size="sm" variant="ghost" onClick={onOpenDrive}>
        Configurar Google Drive
      </Button>
    </div>
  );
}

/** Primer uso: la lista de pasos completa como estado vacío de Instancias. */
export function GettingStartedCard({ state, downloadDir, actions }: { state: OnboardingState; downloadDir: string | null; actions: GettingStartedActions }) {
  if (!state.next) return null;
  const { text, button } = nextAction(state.next, actions);
  return (
    <section aria-labelledby="getting-started-title" className="mx-auto max-w-2xl rounded-xl border border-border bg-surface p-5 shadow-card">
      <header className="flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <h2 id="getting-started-title" className="text-base font-semibold">
            Primeros pasos
          </h2>
          <p className="mt-0.5 text-[13px] text-muted">{text}</p>
        </div>
        {button}
      </header>
      <div className="mt-4">
        <StepList state={state} downloadDir={downloadDir} actions={actions} />
      </div>
      <div className="mt-3">
        <DriveHint onOpenDrive={actions.onOpenDrive} />
      </div>
    </section>
  );
}

/** Con instancias pero sin respaldos completados: una línea con el siguiente paso y los pasos plegados. */
export function GettingStartedBanner({
  state,
  downloadDir,
  actions,
  onDismiss,
}: {
  state: OnboardingState;
  downloadDir: string | null;
  actions: GettingStartedActions;
  onDismiss: () => void;
}) {
  const [open, setOpen] = useState(false);
  const stepsId = useId();
  if (!state.next) return null;
  const { text, button } = nextAction(state.next, actions);
  return (
    <section aria-label="Primeros pasos" className="mb-4 rounded-xl border border-accent/30 bg-surface shadow-card">
      <div className="flex flex-wrap items-center gap-x-4 gap-y-2 px-4 py-3">
        <div className="min-w-0 flex-1">
          <p className="text-[13px] font-semibold">
            Primeros pasos <span className="font-normal text-muted tabular">· {state.completedCount} de {ONBOARDING_STEPS.length}</span>
          </p>
          <p className="text-[13px] text-muted">{text}</p>
        </div>
        <div className="flex items-center gap-1">
          {button}
          <button
            type="button"
            aria-expanded={open}
            aria-controls={stepsId}
            onClick={() => setOpen((current) => !current)}
            className="ml-1 inline-flex h-9 items-center gap-1 rounded-md px-2 text-[13px] text-muted hover:bg-surface-2 hover:text-fg"
          >
            <ChevronRight size={14} className={`transition-transform ${open ? "rotate-90" : ""}`} aria-hidden="true" />
            Ver pasos
          </button>
          <IconButton label="Ocultar primeros pasos" icon={<X size={15} />} onClick={onDismiss} />
        </div>
      </div>
      <div id={stepsId} hidden={!open} className="space-y-3 border-t border-border px-4 py-3">
        <StepList state={state} downloadDir={downloadDir} actions={actions} />
        <DriveHint onOpenDrive={actions.onOpenDrive} />
      </div>
    </section>
  );
}
