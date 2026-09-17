import { useQueryClient } from "@tanstack/react-query";
import { FolderOpen } from "lucide-react";
import { useEffect, useState } from "react";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { Field, NullableNumberInput, Select, TextInput } from "../../components/Field";
import { Card } from "../../components/Layout";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { Settings } from "../../lib/types";

export const PREPARE_TIMEOUT_RANGE = { min: 5, max: 720 } as const;
export const CONCURRENCY_OPTIONS = [1, 2, 3, 4] as const;

type GeneralValues = Pick<
  Settings,
  "downloadDir" | "keepLastLocal" | "maxConcurrentBackups" | "serverPrepareTimeoutMinutes" | "autoLockMinutes"
>;

function pickGeneral(settings: Settings): GeneralValues {
  return {
    downloadDir: settings.downloadDir,
    keepLastLocal: settings.keepLastLocal,
    maxConcurrentBackups: settings.maxConcurrentBackups,
    serverPrepareTimeoutMinutes: settings.serverPrepareTimeoutMinutes,
    autoLockMinutes: settings.autoLockMinutes,
  };
}

export function GeneralSection({ settings }: { settings: Settings }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [values, setValues] = useState<GeneralValues>(() => pickGeneral(settings));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setValues(pickGeneral(settings)), [settings]);

  const dirty = JSON.stringify(values) !== JSON.stringify(pickGeneral(settings));
  const set = <K extends keyof GeneralValues>(key: K, value: GeneralValues[K]) => setValues((v) => ({ ...v, [key]: value }));

  const pickFolder = async () => {
    try {
      const selected = await ipc.pickDirectory(values.downloadDir || undefined);
      if (selected) set("downloadDir", selected);
    } catch (err) {
      toast.error("No se pudo abrir el selector de carpetas", errorMessage(err));
    }
  };

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const current = queryClient.getQueryData<Settings>(queryKeys.settings) ?? settings;
      const next = await ipc.updateSettings({ ...current, ...values });
      queryClient.setQueryData(queryKeys.settings, next);
      toast.success("Ajustes guardados");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  const timeoutInvalid =
    !Number.isFinite(values.serverPrepareTimeoutMinutes) ||
    values.serverPrepareTimeoutMinutes < PREPARE_TIMEOUT_RANGE.min ||
    values.serverPrepareTimeoutMinutes > PREPARE_TIMEOUT_RANGE.max;

  return (
    <Card
      title="Respaldos"
      description="Dónde se guardan los archivos y cómo se ejecutan."
      footer={
        <>
          {dirty ? (
            <Button variant="ghost" onClick={() => setValues(pickGeneral(settings))} disabled={saving}>
              Descartar
            </Button>
          ) : null}
          <Button variant="primary" onClick={save} loading={saving} disabled={!dirty || timeoutInvalid || !values.downloadDir}>
            Guardar
          </Button>
        </>
      }
    >
      <div className="space-y-5">
        <Field label="Carpeta de descarga" hint="Cada instancia usa una subcarpeta: <carpeta>/<instancia>/<bd>_<fecha>.zip">
          {({ id, describedBy }) => (
            <div className="flex gap-2">
              <TextInput id={id} aria-describedby={describedBy} value={values.downloadDir} readOnly className="font-mono text-[13px]" />
              <Button icon={<FolderOpen size={15} />} onClick={pickFolder}>
                Elegir…
              </Button>
            </div>
          )}
        </Field>

        <Field label="Respaldos locales a conservar por instancia" hint="Los más antiguos se eliminan después de cada respaldo correcto.">
          {({ id, describedBy }) => (
            <NullableNumberInput
              id={id}
              describedBy={describedBy}
              value={values.keepLastLocal}
              onChange={(v) => set("keepLastLocal", v)}
              min={1}
              max={1000}
              nullLabel="Conservar todos"
            />
          )}
        </Field>

        <div className="grid gap-5 sm:grid-cols-2">
          <Field label="Respaldos simultáneos" hint="Máximo de respaldos en paralelo.">
            {({ id, describedBy }) => (
              <Select
                id={id}
                aria-describedby={describedBy}
                value={values.maxConcurrentBackups}
                onChange={(e) => set("maxConcurrentBackups", Number(e.target.value))}
                className="w-24"
              >
                {CONCURRENCY_OPTIONS.map((n) => (
                  <option key={n} value={n}>
                    {n}
                  </option>
                ))}
              </Select>
            )}
          </Field>

          <Field
            label="Tiempo máximo de preparación en el servidor"
            hint={`Entre ${PREPARE_TIMEOUT_RANGE.min} y ${PREPARE_TIMEOUT_RANGE.max} minutos. Bases grandes tardan más en generar el zip.`}
            error={timeoutInvalid ? `Usa un valor entre ${PREPARE_TIMEOUT_RANGE.min} y ${PREPARE_TIMEOUT_RANGE.max}.` : undefined}
          >
            {({ id, describedBy, invalid }) => (
              <div className="flex items-center gap-2">
                <TextInput
                  id={id}
                  type="number"
                  min={PREPARE_TIMEOUT_RANGE.min}
                  max={PREPARE_TIMEOUT_RANGE.max}
                  aria-describedby={describedBy}
                  aria-invalid={invalid}
                  value={Number.isFinite(values.serverPrepareTimeoutMinutes) ? values.serverPrepareTimeoutMinutes : ""}
                  onChange={(e) => set("serverPrepareTimeoutMinutes", Number.parseInt(e.target.value, 10))}
                  className="w-24 tabular"
                />
                <span className="text-sm text-muted">min</span>
              </div>
            )}
          </Field>
        </div>

        <Field label="Bloquear la bóveda por inactividad" hint="Se vuelve a pedir el llavero o la contraseña maestra.">
          {({ id, describedBy }) => (
            <NullableNumberInput
              id={id}
              describedBy={describedBy}
              value={values.autoLockMinutes}
              onChange={(v) => set("autoLockMinutes", v)}
              min={1}
              max={1440}
              unit="min"
              nullLabel="Nunca"
            />
          )}
        </Field>

        {error ? <Alert tone="danger">{error}</Alert> : null}
      </div>
    </Card>
  );
}
