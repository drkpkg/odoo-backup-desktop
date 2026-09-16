import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Cloud, CloudOff, ExternalLink } from "lucide-react";
import { useEffect, useState } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Checkbox, Field, NullableNumberInput, PasswordInput, TextInput } from "../../components/Field";
import { Card } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { useToast } from "../../components/Toast";
import { errorMessage, toAppError } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { DriveSettings, DriveStatus, Settings } from "../../lib/types";

export function DriveSection({ settings }: { settings: Settings }) {
  const drive = useQuery({ queryKey: queryKeys.driveStatus, queryFn: () => ipc.getDriveStatus() });

  return (
    <Card
      title={
        <span className="flex items-center gap-2">
          <Cloud size={16} className="text-accent" /> Google Drive
        </span>
      }
      description="Sube automáticamente los backups validados a una carpeta de Google Drive."
    >
      {drive.isPending ? <Spinner label="Consultando Google Drive…" /> : null}
      {drive.isError ? <Alert tone="danger">{errorMessage(drive.error)}</Alert> : null}
      {drive.data ? (
        <div className="space-y-6">
          <AccountBlock status={drive.data} />
          <ClientBlock status={drive.data} />
          <OptionsBlock settings={settings} />
        </div>
      ) : null}
    </Card>
  );
}

function AccountBlock({ status }: { status: DriveStatus }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [connecting, setConnecting] = useState(false);
  const [confirmDisconnect, setConfirmDisconnect] = useState(false);

  const connect = async () => {
    setConnecting(true);
    try {
      const next = await ipc.connectDrive();
      queryClient.setQueryData(queryKeys.driveStatus, next);
      toast.success("Google Drive conectado", next.email ?? undefined);
    } catch (err) {
      const appError = toAppError(err);
      if (appError.code !== "cancelled") toast.error("No se pudo conectar Google Drive", errorMessage(appError));
    } finally {
      setConnecting(false);
    }
  };

  const cancel = async () => {
    try {
      await ipc.cancelDriveConnect();
    } catch (err) {
      toast.error("No se pudo cancelar", errorMessage(err));
    }
  };

  return (
    <section aria-label="Cuenta" className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-3">
          <div className={`flex h-9 w-9 items-center justify-center rounded-full ${status.connected ? "bg-success-soft text-success" : "bg-surface-2 text-subtle"}`}>
            {status.connected ? <Cloud size={17} /> : <CloudOff size={17} />}
          </div>
          <div className="text-sm">
            {status.connected ? (
              <>
                <p className="font-medium">{status.displayName ?? "Cuenta conectada"}</p>
                <p className="text-xs text-muted">{status.email}</p>
              </>
            ) : (
              <>
                <p className="font-medium">Sin conectar</p>
                <p className="text-xs text-muted">
                  {status.configured ? "Autoriza el acceso con tu cuenta de Google." : "Configura primero el cliente OAuth."}
                </p>
              </>
            )}
          </div>
        </div>
        <div className="flex gap-2">
          {status.connected ? (
            <Button variant="ghost" onClick={() => setConfirmDisconnect(true)}>
              Desconectar
            </Button>
          ) : connecting ? (
            <Button onClick={cancel}>Cancelar</Button>
          ) : (
            <Button variant="primary" onClick={connect} disabled={!status.configured} icon={<ExternalLink size={14} />}>
              Conectar cuenta
            </Button>
          )}
        </div>
      </div>

      {connecting ? (
        <Alert tone="info" title="Esperando autorización en el navegador…">
          Se abrió tu navegador en la página de Google. Inicia sesión, acepta el acceso y vuelve a esta ventana (hasta 5 minutos).
        </Alert>
      ) : null}

      <ConfirmDialog
        open={confirmDisconnect}
        title="Desconectar Google Drive"
        confirmLabel="Desconectar"
        onClose={() => setConfirmDisconnect(false)}
        onConfirm={async () => {
          queryClient.setQueryData(queryKeys.driveStatus, await ipc.disconnectDrive());
          toast.success("Google Drive desconectado");
        }}
      >
        <p>Se revocará el acceso de Appex Backup y se borrará el token guardado. Los archivos ya subidos no se eliminan.</p>
      </ConfirmDialog>
    </section>
  );
}

function ClientBlock({ status }: { status: DriveStatus }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [clientId, setClientId] = useState(status.clientId ?? "");
  const [clientSecret, setClientSecret] = useState("");
  const [removeSecret, setRemoveSecret] = useState(false);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setClientId(status.clientId ?? ""), [status.clientId]);

  const dirty = clientId.trim() !== (status.clientId ?? "") || clientSecret.length > 0 || removeSecret;

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const next = await ipc.setDriveClient({
        clientId: clientId.trim(),
        clientSecret: removeSecret ? null : clientSecret.length > 0 ? clientSecret : undefined,
      });
      queryClient.setQueryData(queryKeys.driveStatus, next);
      setClientSecret("");
      setRemoveSecret(false);
      toast.success("Cliente OAuth guardado");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section aria-label="Cliente OAuth" className="space-y-3 border-t border-border pt-5">
      <div>
        <h3 className="text-sm font-semibold">Cliente OAuth de escritorio</h3>
        <details className="mt-1 text-xs text-muted">
          <summary className="cursor-pointer select-none hover:text-fg">¿Cómo crear el cliente?</summary>
          <ol className="mt-2 list-decimal space-y-1 pl-5">
            <li>En Google Cloud Console, crea o elige un proyecto y habilita la «Google Drive API».</li>
            <li>
              En «Google Auth Platform», configura la pantalla de consentimiento y agrega solo el permiso{" "}
              <code className="font-mono">…/auth/drive.file</code> (no sensible).
            </li>
            <li>Publica la app «En producción»: en modo «Prueba» los tokens caducan a los 7 días.</li>
            <li>En «Clientes», crea un cliente de tipo «App de escritorio» y copia el ID y el secreto.</li>
          </ol>
          <p className="mt-2">
            Con <code className="font-mono">drive.file</code> la aplicación solo ve los archivos y carpetas que ella misma crea.
          </p>
        </details>
      </div>

      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Client ID">
          {({ id }) => (
            <TextInput
              id={id}
              value={clientId}
              onChange={(e) => setClientId(e.target.value)}
              placeholder="000000000000-xxxx.apps.googleusercontent.com"
              spellCheck={false}
              className="font-mono text-[12px]"
            />
          )}
        </Field>
        <Field label="Client secret" hint={status.hasClientSecret ? "Déjalo vacío para conservar el guardado." : "Google lo exige para clientes de escritorio."}>
          {({ id, describedBy }) => (
            <PasswordInput
              id={id}
              aria-describedby={describedBy}
              value={clientSecret}
              disabled={removeSecret}
              onChange={(e) => setClientSecret(e.target.value)}
              placeholder={status.hasClientSecret ? "•••••• (sin cambios)" : ""}
            />
          )}
        </Field>
      </div>

      <div className="flex flex-wrap items-center justify-between gap-3">
        {status.hasClientSecret ? (
          <Checkbox
            label="Eliminar el client secret guardado"
            checked={removeSecret}
            onChange={(e) => {
              setRemoveSecret(e.target.checked);
              if (e.target.checked) setClientSecret("");
            }}
          />
        ) : (
          <span />
        )}
        <Button onClick={save} loading={saving} disabled={!dirty || clientId.trim().length === 0}>
          Guardar cliente
        </Button>
      </div>
      {status.connected && clientId.trim() !== (status.clientId ?? "") ? (
        <Alert tone="warning">Al cambiar el Client ID tendrás que volver a conectar la cuenta.</Alert>
      ) : null}
      {error ? <Alert tone="danger">{error}</Alert> : null}
    </section>
  );
}

function OptionsBlock({ settings }: { settings: Settings }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const [values, setValues] = useState<DriveSettings>(settings.drive);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => setValues(settings.drive), [settings.drive]);

  const dirty = JSON.stringify(values) !== JSON.stringify(settings.drive);
  const set = <K extends keyof DriveSettings>(key: K, value: DriveSettings[K]) => setValues((v) => ({ ...v, [key]: value }));

  const save = async () => {
    setSaving(true);
    setError(null);
    try {
      const current = queryClient.getQueryData<Settings>(queryKeys.settings) ?? settings;
      const next = await ipc.updateSettings({
        ...current,
        drive: {
          ...values,
          rootFolderName: values.rootFolderName.trim(),
          sharedDriveId: values.sharedDriveId?.trim() ? values.sharedDriveId.trim() : null,
        },
      });
      queryClient.setQueryData(queryKeys.settings, next);
      toast.success("Opciones de Google Drive guardadas");
    } catch (err) {
      setError(errorMessage(err));
    } finally {
      setSaving(false);
    }
  };

  return (
    <section aria-label="Opciones de subida" className="space-y-4 border-t border-border pt-5">
      <h3 className="text-sm font-semibold">Opciones de subida</h3>
      <div className="grid gap-4 sm:grid-cols-2">
        <Field label="Carpeta raíz" hint="Dentro se crea una subcarpeta por instancia." error={values.rootFolderName.trim() ? undefined : "Ingresa un nombre de carpeta."}>
          {({ id, describedBy, invalid }) => (
            <TextInput id={id} aria-describedby={describedBy} aria-invalid={invalid} value={values.rootFolderName} onChange={(e) => set("rootFolderName", e.target.value)} />
          )}
        </Field>
        <Field label="Unidad compartida (ID)" optional hint="Vacío = «Mi unidad».">
          {({ id, describedBy }) => (
            <TextInput
              id={id}
              aria-describedby={describedBy}
              value={values.sharedDriveId ?? ""}
              onChange={(e) => set("sharedDriveId", e.target.value || null)}
              spellCheck={false}
              className="font-mono text-[12px]"
            />
          )}
        </Field>
      </div>

      <Field label="Backups a conservar en Drive por instancia">
        {({ id, describedBy }) => (
          <NullableNumberInput
            id={id}
            describedBy={describedBy}
            value={values.keepLast}
            onChange={(v) => set("keepLast", v)}
            min={1}
            max={1000}
            nullLabel="Conservar todos"
          />
        )}
      </Field>

      <fieldset className="space-y-2">
        <legend className="text-[13px] font-medium">Al aplicar la retención</legend>
        <label className="flex cursor-pointer items-start gap-2.5 text-sm">
          <input
            type="radio"
            name="drive-delete-mode"
            checked={!values.permanentDelete}
            onChange={() => set("permanentDelete", false)}
            className="mt-0.5 h-4 w-4 accent-[var(--app-accent)]"
          />
          <span>
            <span className="font-medium">Mover a la papelera</span>
            <span className="block text-xs text-muted">Google la vacía a los 30 días. Recomendado.</span>
          </span>
        </label>
        <label className="flex cursor-pointer items-start gap-2.5 text-sm">
          <input
            type="radio"
            name="drive-delete-mode"
            checked={values.permanentDelete}
            onChange={() => set("permanentDelete", true)}
            className="mt-0.5 h-4 w-4 accent-[var(--app-accent)]"
          />
          <span>
            <span className="font-medium">Eliminar definitivamente</span>
            <span className="block text-xs text-muted">En unidades compartidas requiere rol de administrador.</span>
          </span>
        </label>
      </fieldset>

      {error ? <Alert tone="danger">{error}</Alert> : null}
      <div className="flex justify-end gap-2">
        {dirty ? (
          <Button variant="ghost" onClick={() => setValues(settings.drive)} disabled={saving}>
            Descartar
          </Button>
        ) : null}
        <Button variant="primary" onClick={save} loading={saving} disabled={!dirty || !values.rootFolderName.trim()}>
          Guardar opciones
        </Button>
      </div>
      <Badge tone="neutral">Permiso usado: drive.file</Badge>
    </section>
  );
}
