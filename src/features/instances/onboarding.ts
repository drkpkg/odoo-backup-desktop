import type { InstanceView } from "../../lib/types";
import { connectionState } from "./connection";

export type OnboardingStepId = "add_instance" | "probe" | "folder" | "first_backup";

export const ONBOARDING_STEPS: OnboardingStepId[] = ["add_instance", "probe", "folder", "first_backup"];

/** Qué hacer ahora, con la instancia sobre la que actuar. */
export type OnboardingNext =
  | { kind: "add_instance" }
  | { kind: "probe"; instance: InstanceView }
  | { kind: "fix"; instance: InstanceView; reason: string }
  | { kind: "backup"; instance: InstanceView }
  | { kind: "in_progress"; instance: InstanceView };

export type OnboardingState = {
  done: Record<OnboardingStepId, boolean>;
  completedCount: number;
  /** La guía ya no hace falta: hay al menos un respaldo completado. */
  finished: boolean;
  next: OnboardingNext | null;
};

/**
 * Primeros pasos derivados de los datos (sin estado guardado): añadir una instancia, dejar una
 * lista para respaldar, revisar la carpeta (siempre tiene un valor por defecto) y completar el
 * primer respaldo. `runningInstanceIds` son las instancias con un respaldo en curso.
 */
export function onboardingState(instances: InstanceView[], runningInstanceIds: ReadonlySet<string> = new Set()): OnboardingState {
  const ready = instances.filter((instance) => connectionState(instance).tone === "success");
  const done: Record<OnboardingStepId, boolean> = {
    add_instance: instances.length > 0,
    probe: ready.length > 0,
    folder: true,
    first_backup: instances.some((instance) => instance.lastBackup?.status === "success"),
  };
  const completedCount = ONBOARDING_STEPS.filter((step) => done[step]).length;
  const finished = done.first_backup;

  let next: OnboardingNext | null = null;
  if (!finished) {
    const running = instances.find((instance) => runningInstanceIds.has(instance.id));
    const untested = instances.find((instance) => instance.lastProbe === null);
    const [firstReady] = ready;
    const [firstInstance] = instances;
    if (!firstInstance) next = { kind: "add_instance" };
    else if (running) next = { kind: "in_progress", instance: running };
    else if (firstReady) next = { kind: "backup", instance: firstReady };
    else if (untested) next = { kind: "probe", instance: untested };
    else next = { kind: "fix", instance: firstInstance, reason: connectionState(firstInstance).reason };
  }
  return { done, completedCount, finished, next };
}
