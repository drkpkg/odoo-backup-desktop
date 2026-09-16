import { zodResolver } from "@hookform/resolvers/zod";
import { useQueryClient } from "@tanstack/react-query";
import { PlugZap } from "lucide-react";
import { useMemo, useRef, useState } from "react";
import { Controller, useForm } from "react-hook-form";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { Dialog } from "../../components/Dialog";
import { Checkbox, Field, PasswordInput, Select, TextInput } from "../../components/Field";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import {
  PROTOCOL_PREFERENCE_LABELS,
  SECRET_KIND_LABELS,
  TRANSPORT_PREFERENCE_LABELS,
} from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import type { InstanceView, ProbeReport, ProtocolPreference, SecretKind, TransportPreference } from "../../lib/types";
import { ProbeReportView } from "./ProbeReportView";
import {
  buildInstanceInput,
  buildProbeRequest,
  databaseFromUrl,
  defaultFormValues,
  makeInstanceFormSchema,
  type InstanceFormValues,
} from "./schema";

type Props = {
  open: boolean;
  instance: InstanceView | null;
  onClose: () => void;
};

export function InstanceFormDialog({ open, instance, onClose }: Props) {
  const title = instance ? `Editar ${instance.name}` : "Nueva instancia";
  return (
    <Dialog open={open} onClose={onClose} title={title} size="lg" description="Los secretos se guardan cifrados en la bóveda y nunca se muestran.">
      {/* Montado solo mientras está abierto: al cerrar se descartan los secretos escritos. */}
      <InstanceForm instance={instance} onClose={onClose} />
    </Dialog>
  );
}

function InstanceForm({ instance, onClose }: { instance: InstanceView | null; onClose: () => void }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const schema = useMemo(
    () =>
      makeInstanceFormSchema({
        hasSecret: instance?.hasSecret ?? false,
        hasMasterPassword: instance?.hasMasterPassword ?? false,
      }),
    [instance],
  );

  const {
    register,
    control,
    handleSubmit,
    watch,
    setValue,
    getValues,
    trigger,
    reset,
    formState: { errors, isSubmitting },
  } = useForm<InstanceFormValues>({
    resolver: zodResolver(schema),
    defaultValues: defaultFormValues(instance),
    mode: "onTouched",
  });

  const [probe, setProbe] = useState<ProbeReport | null>(instance?.lastProbe ?? null);
  const [probeFresh, setProbeFresh] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const lastSuggestion = useRef<string | null>(instance ? databaseFromUrl(instance.url) : null);

  const secretKind = watch("secretKind");
  const transport = watch("transport");
  const removeMaster = watch("removeMasterPassword");

  const urlField = register("url", {
    onChange: (event: { target: { value: string } }) => {
      const suggestion = databaseFromUrl(event.target.value);
      const currentDb = getValues("database");
      // Solo autocompletar si el campo está vacío o conserva la sugerencia anterior.
      if (suggestion && (currentDb === "" || currentDb === lastSuggestion.current)) {
        setValue("database", suggestion, { shouldValidate: currentDb !== "" });
      }
      lastSuggestion.current = suggestion;
    },
  });

  const close = () => {
    reset(defaultFormValues(null));
    onClose();
  };

  const runProbe = async () => {
    const valid = await trigger(["url"]);
    if (!valid) return;
    setProbing(true);
    setProbeError(null);
    try {
      const report = await ipc.probeInstance(buildProbeRequest(getValues(), instance));
      setProbe(report);
      setProbeFresh(true);
      if (instance) void queryClient.invalidateQueries({ queryKey: queryKeys.instances });
    } catch (err) {
      setProbeError(errorMessage(err));
    } finally {
      setProbing(false);
    }
  };

  const onSubmit = handleSubmit(async (values) => {
    setSaveError(null);
    try {
      const saved = await ipc.saveInstance(buildInstanceInput(values, instance));
      await queryClient.invalidateQueries({ queryKey: queryKeys.instances });
      toast.success(instance ? "Instancia actualizada" : "Instancia creada", saved.name);
      close();
    } catch (err) {
      setSaveError(errorMessage(err));
    }
  });

  const secretLabel = SECRET_KIND_LABELS[secretKind];
  const editingWithSecret = Boolean(instance?.hasSecret);

  return (
    <form onSubmit={onSubmit} noValidate className="grid gap-6 md:grid-cols-[minmax(0,1fr)_minmax(0,0.9fr)]">
      <div className="space-y-4">
        <Field label="Nombre" error={errors.name?.message}>
          {({ id, describedBy, invalid }) => (
            <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} placeholder="Cliente S.A." autoFocus {...register("name")} />
          )}
        </Field>

        <Field label="URL" error={errors.url?.message} hint="Ej.: https://cliente.nube-appex.lat">
          {({ id, describedBy, invalid }) => (
            <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} placeholder="https://" inputMode="url" spellCheck={false} {...urlField} />
          )}
        </Field>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Base de datos" error={errors.database?.message} hint="Se sugiere desde el subdominio.">
            {({ id, describedBy, invalid }) => (
              <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} spellCheck={false} {...register("database")} />
            )}
          </Field>
          <Field label="Usuario" error={errors.login?.message}>
            {({ id, describedBy, invalid }) => (
              <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} spellCheck={false} autoComplete="off" {...register("login")} />
            )}
          </Field>
        </div>

        <div className="grid gap-4 sm:grid-cols-[10rem_minmax(0,1fr)]">
          <Field label="Credencial">
            {({ id }) => (
              <Select id={id} {...register("secretKind")}>
                {(Object.keys(SECRET_KIND_LABELS) as SecretKind[]).map((kind) => (
                  <option key={kind} value={kind}>
                    {SECRET_KIND_LABELS[kind]}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field
            label={secretLabel}
            error={errors.secret?.message}
            hint={editingWithSecret ? "Déjalo vacío para conservar el valor guardado." : secretKind === "api_key" ? "Preferencias → Seguridad de la cuenta → Nueva API key." : undefined}
          >
            {({ id, describedBy, invalid }) => (
              <PasswordInput
                id={id}
                aria-describedby={describedBy}
                aria-invalid={invalid}
                placeholder={editingWithSecret ? "•••••• (sin cambios)" : ""}
                {...register("secret")}
              />
            )}
          </Field>
        </div>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field label="Transporte de backup" error={errors.transport?.message}>
            {({ id, describedBy, invalid }) => (
              <Select id={id} aria-describedby={describedBy} aria-invalid={invalid} {...register("transport")}>
                {(Object.keys(TRANSPORT_PREFERENCE_LABELS) as TransportPreference[]).map((value) => (
                  <option key={value} value={value}>
                    {TRANSPORT_PREFERENCE_LABELS[value]}
                  </option>
                ))}
              </Select>
            )}
          </Field>
          <Field label="Protocolo RPC" error={errors.protocol?.message}>
            {({ id, describedBy, invalid }) => (
              <Select id={id} aria-describedby={describedBy} aria-invalid={invalid} {...register("protocol")}>
                {(Object.keys(PROTOCOL_PREFERENCE_LABELS) as ProtocolPreference[]).map((value) => (
                  <option key={value} value={value}>
                    {PROTOCOL_PREFERENCE_LABELS[value]}
                  </option>
                ))}
              </Select>
            )}
          </Field>
        </div>

        <fieldset className="space-y-2 rounded-lg border border-border p-3">
          <legend className="px-1 text-[13px] font-medium">Contraseña maestra (gestor de BD)</legend>
          <p className="text-xs text-muted">
            Solo la usa el transporte Gestor de BD (<code className="font-mono">/web/database/backup</code>). No hace falta con el módulo appex_backup.
          </p>
          {!removeMaster ? (
            <Field label="Contraseña maestra" optional={transport !== "db_manager"} error={errors.masterPassword?.message}>
              {({ id, describedBy, invalid }) => (
                <PasswordInput
                  id={id}
                  aria-describedby={describedBy}
                  aria-invalid={invalid}
                  placeholder={instance?.hasMasterPassword ? "•••••• (sin cambios)" : ""}
                  {...register("masterPassword")}
                />
              )}
            </Field>
          ) : (
            <Alert tone="warning">La contraseña maestra guardada se eliminará al guardar.</Alert>
          )}
          {instance?.hasMasterPassword ? (
            <Controller
              control={control}
              name="removeMasterPassword"
              render={({ field }) => (
                <Checkbox
                  label="Eliminar la contraseña maestra guardada"
                  checked={field.value}
                  onChange={(e) => {
                    field.onChange(e.target.checked);
                    if (e.target.checked) setValue("masterPassword", "");
                  }}
                />
              )}
            />
          ) : null}
        </fieldset>

        <div className="space-y-2.5">
          <Checkbox label="Incluir filestore" description="Adjuntos y archivos de la base. Desactivarlo solo es efectivo en Odoo 19 o con el módulo." {...register("includeFilestore")} />
          <Checkbox label="Subir a Google Drive" description="Tras validar el backup, se sube a la carpeta configurada en Ajustes." {...register("uploadToDrive")} />
        </div>

        {saveError ? <Alert tone="danger">{saveError}</Alert> : null}

        <div className="flex items-center justify-end gap-2 border-t border-border pt-4">
          <Button onClick={close} disabled={isSubmitting}>
            Cancelar
          </Button>
          <Button type="submit" variant="primary" loading={isSubmitting}>
            {instance ? "Guardar cambios" : "Crear instancia"}
          </Button>
        </div>
      </div>

      <aside className="space-y-3 md:border-l md:border-border md:pl-6">
        <div className="flex items-center justify-between gap-2">
          <h3 className="text-sm font-semibold">Diagnóstico</h3>
          <Button size="sm" onClick={runProbe} loading={probing} icon={<PlugZap size={14} />}>
            Probar conexión
          </Button>
        </div>
        <p className="text-xs text-muted">
          Detecta la versión de Odoo, el protocolo (XML-RPC ≤ 18, JSON-2 ≥ 19) y qué transporte de backup está disponible.
        </p>
        {probing ? <Spinner label="Conectando con el servidor…" /> : null}
        {probeError ? <Alert tone="danger" title="No se pudo probar la conexión">{probeError}</Alert> : null}
        {probe && !probing ? (
          <>
            {!probeFresh ? <p className="text-xs text-subtle">Última prueba guardada:</p> : null}
            <ProbeReportView report={probe} />
          </>
        ) : null}
        {!probe && !probing && !probeError ? (
          <div className="rounded-lg border border-dashed border-border px-4 py-8 text-center text-xs text-subtle">
            Completa la URL y las credenciales y pulsa «Probar conexión».
          </div>
        ) : null}
      </aside>
    </form>
  );
}
