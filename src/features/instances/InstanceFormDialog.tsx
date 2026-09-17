import { zodResolver } from "@hookform/resolvers/zod";
import { useQueryClient } from "@tanstack/react-query";
import { ChevronRight, PlugZap } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { Controller, useForm } from "react-hook-form";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { Dialog, DialogBody, DialogFooter } from "../../components/Dialog";
import { Checkbox, Field, PasswordInput, Select, TextInput } from "../../components/Field";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { formatRelative } from "../../lib/format";
import { ipc } from "../../lib/ipc";
import { PROTOCOL_PREFERENCE_LABELS, SECRET_KIND_LABELS, TRANSPORT_PREFERENCE_LABELS } from "../../lib/labels";
import { queryKeys } from "../../lib/query";
import type { InstanceView, ProbeReport, ProtocolPreference, SecretKind, TransportPreference } from "../../lib/types";
import { ProbeReportView } from "./ProbeReportView";
import { assessReadiness, GUIDANCE_ACTION_LABELS, probeGuidance, type Guidance, type GuidanceAction } from "./readiness";
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

type FieldName = keyof InstanceFormValues;

/** Cambiarlos invalida la última prueba. */
const CONNECTION_FIELDS: ReadonlySet<string> = new Set<FieldName>(["url", "database", "login", "secretKind", "secret", "protocol"]);
/** Campos dentro de "Opciones de respaldo" (plegado). */
const OPTION_FIELDS: readonly FieldName[] = ["transport", "protocol", "masterPassword"];

const DOT_TONES: Record<Guidance["tone"] | "neutral", string> = {
  success: "bg-success",
  info: "bg-info",
  warning: "bg-warning",
  danger: "bg-danger",
  neutral: "bg-subtle",
};

export function InstanceFormDialog({ open, instance, onClose }: Props) {
  const title = instance ? `Editar ${instance.name}` : "Nueva instancia";
  return (
    <Dialog open={open} onClose={onClose} title={title} size="md" plain description="Los secretos se guardan cifrados en la bóveda y nunca se muestran.">
      {/* Montado solo mientras está abierto: al cerrar se descartan los secretos escritos. */}
      <InstanceForm instance={instance} onClose={onClose} />
    </Dialog>
  );
}

/**
 * Flujo progresivo: datos de conexión → probar → siguiente paso concreto. El método, el protocolo,
 * la contraseña maestra y las opciones de archivo quedan plegados en "Opciones de respaldo" y se
 * abren solos cuando el siguiente paso o un error de validación los necesita.
 */
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
    setFocus,
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
  const [probeStale, setProbeStale] = useState(false);
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [optionsOpen, setOptionsOpen] = useState(false);
  const [pendingFocus, setPendingFocus] = useState<FieldName | null>(null);
  const lastSuggestion = useRef<string | null>(instance ? databaseFromUrl(instance.url) : null);
  const resultRef = useRef<HTMLDivElement>(null);
  const optionsId = useId();

  const values = watch();
  const hasMasterPassword = !values.removeMasterPassword && (values.masterPassword.length > 0 || Boolean(instance?.hasMasterPassword));
  const readinessInput = {
    transport: values.transport,
    protocol: values.protocol,
    secretKind: values.secretKind,
    hasMasterPassword,
  };
  const guidance = probe ? probeGuidance(assessReadiness(probe, readinessInput), readinessInput) : null;

  useEffect(() => {
    const subscription = watch((_, { name }) => {
      if (name && CONNECTION_FIELDS.has(name)) setProbeStale(true);
    });
    return () => subscription.unsubscribe();
  }, [watch]);

  // Enfocar un campo de las opciones cuando ya se desplegaron.
  useEffect(() => {
    if (optionsOpen && pendingFocus) {
      setFocus(pendingFocus);
      setPendingFocus(null);
    }
  }, [optionsOpen, pendingFocus, setFocus]);

  const revealOption = (field: FieldName) => {
    setOptionsOpen(true);
    setPendingFocus(field);
  };

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
      setProbeStale(false);
      if (instance) void queryClient.invalidateQueries({ queryKey: queryKeys.instances });
    } catch (err) {
      setProbeError(errorMessage(err));
    } finally {
      setProbing(false);
      // El resultado queda debajo del botón: traerlo a la vista sin mover el foco.
      requestAnimationFrame(() => resultRef.current?.scrollIntoView({ block: "nearest", behavior: "smooth" }));
    }
  };

  const applyAction = (action: GuidanceAction) => {
    switch (action) {
      case "add_master_password":
        setValue("removeMasterPassword", false);
        revealOption("masterPassword");
        break;
      case "use_api_key":
        setValue("secretKind", "api_key", { shouldValidate: true });
        setFocus("secret");
        break;
      case "use_auto_transport":
        setValue("transport", "auto", { shouldValidate: true });
        break;
      case "use_auto_protocol":
        setValue("protocol", "auto", { shouldValidate: true });
        break;
    }
  };

  const onSubmit = handleSubmit(
    async (formValues) => {
      setSaveError(null);
      try {
        const saved = await ipc.saveInstance(buildInstanceInput(formValues, instance));
        await queryClient.invalidateQueries({ queryKey: queryKeys.instances });
        toast.success(instance ? "Instancia actualizada" : "Instancia creada", saved.name);
        close();
      } catch (err) {
        setSaveError(errorMessage(err));
      }
    },
    (formErrors) => {
      // react-hook-form no puede enfocar campos ocultos: desplegar las opciones si el error está ahí.
      const visibleError = (Object.keys(formErrors) as FieldName[]).some((name) => !OPTION_FIELDS.includes(name));
      const optionError = OPTION_FIELDS.find((name) => formErrors[name]);
      if (!visibleError && optionError) revealOption(optionError);
    },
  );

  const secretLabel = SECRET_KIND_LABELS[values.secretKind];
  const editingWithSecret = Boolean(instance?.hasSecret);
  const suggestedDb = databaseFromUrl(values.url);
  const optionErrors = OPTION_FIELDS.filter((name) => errors[name]).length;
  const optionsSummary = [
    `Método: ${TRANSPORT_PREFERENCE_LABELS[values.transport]}`,
    values.protocol !== "auto" ? PROTOCOL_PREFERENCE_LABELS[values.protocol] : null,
    hasMasterPassword ? "con contraseña maestra" : null,
    values.includeFilestore ? "con filestore" : "sin filestore",
    values.uploadToDrive ? "sube a Google Drive" : null,
  ]
    .filter(Boolean)
    .join(" · ");

  const status: { tone: keyof typeof DOT_TONES; text: string } = probing
    ? { tone: "neutral", text: "Probando conexión…" }
    : probeError
      ? { tone: "danger", text: "No se pudo conectar" }
      : !guidance
        ? { tone: "neutral", text: "Sin probar" }
        : probeStale
          ? { tone: "neutral", text: "Datos cambiados: vuelve a probar" }
          : { tone: guidance.tone, text: guidance.tone === "success" ? "Lista para respaldar" : "Requiere atención" };

  return (
    <form onSubmit={onSubmit} noValidate className="flex min-h-0 flex-1 flex-col">
      <DialogBody className="space-y-4">
        <Field label="Nombre" error={errors.name?.message}>
          {({ id, describedBy, invalid }) => (
            <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} placeholder="Cliente S.A." autoFocus {...register("name")} />
          )}
        </Field>

        <Field label="URL" error={errors.url?.message}>
          {({ id, describedBy, invalid }) => (
            <TextInput
              id={id}
              aria-describedby={describedBy}
              aria-invalid={invalid}
              placeholder="https://cliente.nube.example.com"
              inputMode="url"
              spellCheck={false}
              {...urlField}
            />
          )}
        </Field>

        <div className="grid gap-4 sm:grid-cols-2">
          <Field
            label="Base de datos"
            error={errors.database?.message}
            hint={suggestedDb && values.database === suggestedDb && values.url !== instance?.url ? "Sugerida por el subdominio: verifícala." : undefined}
          >
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

        <div className="grid gap-4 sm:grid-cols-[9rem_minmax(0,1fr)]">
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
            hint={
              editingWithSecret
                ? "Déjalo vacío para conservar el valor guardado."
                : values.secretKind === "api_key"
                  ? "Preferencias → Seguridad de la cuenta → Nueva API key."
                  : undefined
            }
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

        <section aria-label="Comprobación de la conexión" className="border-t border-border pt-4">
          <div className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2">
            <p className="min-w-0 flex-1 text-xs text-muted">
              {probe && !probeStale
                ? `Probada ${formatRelative(probe.checkedAt)}.`
                : "Detecta la versión de Odoo y el método de respaldo."}
            </p>
            <Button onClick={runProbe} loading={probing} icon={<PlugZap size={14} />}>
              {probe ? "Probar de nuevo" : "Probar conexión"}
            </Button>
          </div>

          <div ref={resultRef} aria-live="polite" className={probing || probeError || probe ? "mt-3 space-y-3" : ""}>
            {probing ? <Spinner label="Conectando con el servidor…" /> : null}
            {probeError && !probing ? (
              <Alert tone="danger" title="No se pudo conectar">
                {probeError}
              </Alert>
            ) : null}
            {probe && guidance && !probing && !probeError ? (
              <>
                {probeStale ? (
                  <p className="text-xs text-warning">Cambiaste datos de conexión después de la prueba: vuelve a probar para confirmarlos.</p>
                ) : null}
                <ProbeReportView
                  report={probe}
                  guidance={guidance}
                  collapseDetails
                  action={
                    guidance.action ? (
                      <Button size="sm" onClick={() => guidance.action && applyAction(guidance.action)}>
                        {GUIDANCE_ACTION_LABELS[guidance.action]}
                      </Button>
                    ) : null
                  }
                />
              </>
            ) : null}
          </div>
        </section>

        <section className="rounded-lg border border-border">
          <h3>
            <button
              type="button"
              aria-expanded={optionsOpen}
              aria-controls={optionsId}
              onClick={() => setOptionsOpen((current) => !current)}
              className="flex w-full items-center gap-2.5 rounded-lg px-3 py-2.5 text-left hover:bg-surface-2"
            >
              <ChevronRight size={15} className={`shrink-0 text-muted transition-transform ${optionsOpen ? "rotate-90" : ""}`} aria-hidden="true" />
              <span className="min-w-0 flex-1">
                <span className="block text-[13px] font-medium">Opciones de respaldo</span>
                <span className="block truncate text-xs text-muted">{optionsSummary}</span>
              </span>
              {optionErrors > 0 && !optionsOpen ? (
                <span className="shrink-0 text-xs font-medium text-danger">
                  {optionErrors === 1 ? "1 campo por revisar" : `${optionErrors} campos por revisar`}
                </span>
              ) : null}
            </button>
          </h3>

          <div id={optionsId} hidden={!optionsOpen} className="space-y-4 border-t border-border px-3 py-3">
            <div className="grid gap-4 sm:grid-cols-2">
              <Field label="Método de respaldo" error={errors.transport?.message}>
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

            <div className="space-y-2">
              {!values.removeMasterPassword ? (
                <Field
                  label="Contraseña maestra"
                  optional={values.transport !== "db_manager"}
                  error={errors.masterPassword?.message}
                  hint="Solo para el método Gestor de BD (/web/database/backup). No hace falta con el módulo obd_backup."
                >
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
            </div>

            <div className="space-y-2.5">
              <Checkbox label="Incluir filestore" description="Adjuntos y archivos de la base. Desactivarlo solo es efectivo en Odoo 19 o con el módulo." {...register("includeFilestore")} />
              <Checkbox label="Subir los respaldos a Google Drive" description="Tras validar cada respaldo, se sube a la carpeta configurada en Ajustes." {...register("uploadToDrive")} />
            </div>
          </div>
        </section>

        {saveError ? <Alert tone="danger">{saveError}</Alert> : null}
      </DialogBody>

      {/* Fuera del área con scroll: guardar siempre está a la vista. */}
      <DialogFooter className="gap-3">
        <p className="flex min-w-0 flex-1 items-center gap-2 text-xs text-muted">
          <span className={`h-2 w-2 shrink-0 rounded-full ${DOT_TONES[status.tone]}`} aria-hidden="true" />
          <span className="truncate">{status.text}</span>
        </p>
        <Button onClick={close} disabled={isSubmitting}>
          Cancelar
        </Button>
        <Button type="submit" variant="primary" loading={isSubmitting}>
          {instance ? "Guardar cambios" : "Crear instancia"}
        </Button>
      </DialogFooter>
    </form>
  );
}
