import { useQueryClient } from "@tanstack/react-query";
import { KeyRound } from "lucide-react";
import { useState, type FormEvent } from "react";

import { Alert } from "../../components/Alert";
import { Badge } from "../../components/Badge";
import { Button } from "../../components/Button";
import { ConfirmDialog } from "../../components/ConfirmDialog";
import { Dialog } from "../../components/Dialog";
import { Field, PasswordInput, Switch } from "../../components/Field";
import { Card } from "../../components/Layout";
import { useToast } from "../../components/Toast";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { MIN_MASTER_PASSWORD_LENGTH } from "../vault/VaultGate";

export function SecuritySection({ status }: { status: AppStatus }) {
  const queryClient = useQueryClient();
  const toast = useToast();
  const { keychainAvailable, keychainEnabled, passwordEnabled } = status.vault;
  const [passwordDialog, setPasswordDialog] = useState(false);
  const [confirmRemovePassword, setConfirmRemovePassword] = useState(false);
  const [confirmDisableKeychain, setConfirmDisableKeychain] = useState(false);
  const [togglingKeychain, setTogglingKeychain] = useState(false);

  const apply = (next: AppStatus) => queryClient.setQueryData(queryKeys.appStatus, next);

  const enableKeychain = async () => {
    setTogglingKeychain(true);
    try {
      apply(await ipc.setKeychainEnabled({ enabled: true }));
      toast.success("Llavero del sistema activado");
    } catch (err) {
      toast.error("No se pudo activar el llavero", errorMessage(err));
    } finally {
      setTogglingKeychain(false);
    }
  };

  return (
    <Card title="Seguridad de la bóveda" description="Cómo se abre la bóveda cifrada que guarda las credenciales.">
      <div className="space-y-5">
        <div className="flex flex-wrap gap-2">
          <Badge tone={keychainEnabled ? "success" : "neutral"}>
            Llavero: {keychainEnabled ? "activo" : keychainAvailable ? "desactivado" : "no disponible"}
          </Badge>
          <Badge tone={passwordEnabled ? "success" : "warning"}>
            Contraseña maestra: {passwordEnabled ? "configurada" : "sin configurar"}
          </Badge>
        </div>

        <Switch
          checked={keychainEnabled}
          disabled={togglingKeychain || (!keychainEnabled && !keychainAvailable) || (keychainEnabled && !passwordEnabled)}
          onChange={(checked) => (checked ? void enableKeychain() : setConfirmDisableKeychain(true))}
          label="Abrir con el llavero del sistema"
          description={
            !keychainAvailable
              ? "No hay un servicio de llavero disponible en este equipo."
              : keychainEnabled && !passwordEnabled
                ? "Configura una contraseña maestra antes de desactivarlo."
                : "Guarda la clave de la bóveda en Secret Service / Credential Manager."
          }
        />

        <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border pt-4">
          <div className="text-sm">
            <p className="font-medium">Contraseña maestra</p>
            <p className="text-xs text-muted">
              {passwordEnabled
                ? "Permite abrir la bóveda sin el llavero y recuperar el acceso."
                : "Recomendado: define una para no depender solo del llavero del sistema."}
            </p>
          </div>
          <div className="flex gap-2">
            {passwordEnabled && keychainEnabled ? (
              <Button variant="ghost" onClick={() => setConfirmRemovePassword(true)}>
                Quitar
              </Button>
            ) : null}
            <Button icon={<KeyRound size={15} />} onClick={() => setPasswordDialog(true)}>
              {passwordEnabled ? "Cambiar" : "Definir"}
            </Button>
          </div>
        </div>

        {!keychainEnabled && !passwordEnabled ? (
          <Alert tone="danger">La bóveda no tiene métodos de desbloqueo configurados.</Alert>
        ) : null}
      </div>

      <MasterPasswordDialog
        open={passwordDialog}
        replacing={passwordEnabled}
        onClose={() => setPasswordDialog(false)}
        onSaved={(next) => {
          apply(next);
          toast.success(passwordEnabled ? "Contraseña maestra cambiada" : "Contraseña maestra definida");
        }}
      />

      <ConfirmDialog
        open={confirmRemovePassword}
        title="Quitar contraseña maestra"
        confirmLabel="Quitar contraseña"
        onClose={() => setConfirmRemovePassword(false)}
        onConfirm={async () => {
          apply(await ipc.setMasterPassword({}));
          toast.success("Contraseña maestra eliminada");
        }}
      >
        <p>
          La bóveda solo podrá abrirse con el llavero del sistema de este equipo. Si el llavero se borra, perderás el acceso a
          las credenciales guardadas.
        </p>
      </ConfirmDialog>

      <ConfirmDialog
        open={confirmDisableKeychain}
        title="Desactivar el llavero del sistema"
        confirmLabel="Desactivar"
        tone="primary"
        onClose={() => setConfirmDisableKeychain(false)}
        onConfirm={async () => {
          apply(await ipc.setKeychainEnabled({ enabled: false }));
          toast.success("Llavero del sistema desactivado");
        }}
      >
        <p>Se borrará la clave del llavero y la bóveda pedirá la contraseña maestra cada vez que se abra.</p>
      </ConfirmDialog>
    </Card>
  );
}

function MasterPasswordDialog({
  open,
  replacing,
  onClose,
  onSaved,
}: {
  open: boolean;
  replacing: boolean;
  onClose: () => void;
  onSaved: (status: AppStatus) => void;
}) {
  return (
    <Dialog open={open} onClose={onClose} title={replacing ? "Cambiar contraseña maestra" : "Definir contraseña maestra"} size="sm">
      <MasterPasswordForm onClose={onClose} onSaved={onSaved} />
    </Dialog>
  );
}

function MasterPasswordForm({ onClose, onSaved }: { onClose: () => void; onSaved: (status: AppStatus) => void }) {
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const passwordError =
    password.length < MIN_MASTER_PASSWORD_LENGTH ? `Usa al menos ${MIN_MASTER_PASSWORD_LENGTH} caracteres.` : undefined;
  const confirmError = confirm !== password ? "Las contraseñas no coinciden." : undefined;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitted(true);
    if (passwordError || confirmError) return;
    setBusy(true);
    setError(null);
    try {
      const next = await ipc.setMasterPassword({ newPassword: password });
      setPassword("");
      setConfirm("");
      onSaved(next);
      onClose();
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="space-y-4" noValidate>
      <Field label="Nueva contraseña maestra" error={submitted ? passwordError : undefined} hint="No se puede recuperar si la olvidas.">
        {({ id, describedBy, invalid }) => (
          <PasswordInput id={id} aria-describedby={describedBy} aria-invalid={invalid} value={password} onChange={(e) => setPassword(e.target.value)} autoFocus />
        )}
      </Field>
      <Field label="Confirmar contraseña" error={submitted ? confirmError : undefined}>
        {({ id, describedBy, invalid }) => (
          <PasswordInput id={id} aria-describedby={describedBy} aria-invalid={invalid} value={confirm} onChange={(e) => setConfirm(e.target.value)} />
        )}
      </Field>
      <p className="text-xs text-muted">Volver a cifrar la clave puede tardar unos segundos.</p>
      {error ? <Alert tone="danger">{error}</Alert> : null}
      <div className="flex justify-end gap-2">
        <Button onClick={onClose} disabled={busy}>
          Cancelar
        </Button>
        <Button type="submit" variant="primary" loading={busy}>
          Guardar
        </Button>
      </div>
    </form>
  );
}
