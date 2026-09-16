import { useQueryClient } from "@tanstack/react-query";
import { KeyRound, LockKeyhole, ShieldCheck } from "lucide-react";
import { useEffect, useRef, useState, type FormEvent, type ReactNode } from "react";

import { Alert } from "../../components/Alert";
import { Button } from "../../components/Button";
import { Field, PasswordInput, Switch } from "../../components/Field";
import { Logo } from "../../components/Logo";
import { Spinner } from "../../components/Spinner";
import { errorMessage, toAppError } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";

export const MIN_MASTER_PASSWORD_LENGTH = 10;

/** Muestra crear/desbloquear bóveda hasta que esté abierta; luego renderiza la app. */
export function VaultGate({ status, children }: { status: AppStatus; children: ReactNode }) {
  if (!status.vault.exists) return <GateShell><CreateVault status={status} /></GateShell>;
  if (!status.vault.unlocked) return <GateShell><UnlockVault status={status} /></GateShell>;
  return <>{children}</>;
}

function GateShell({ children }: { children: ReactNode }) {
  return (
    <main className="flex min-h-full items-center justify-center bg-bg px-4 py-10">
      <div className="w-full max-w-md">
        <div className="mb-6 flex items-center justify-center gap-2.5">
          <Logo size={34} />
          <span className="text-lg font-semibold tracking-tight">Odoo Backup Desktop</span>
        </div>
        <div className="rounded-xl border border-border bg-surface p-6 shadow-card">{children}</div>
      </div>
    </main>
  );
}

function useApplyStatus() {
  const queryClient = useQueryClient();
  return (next: AppStatus) => queryClient.setQueryData(queryKeys.appStatus, next);
}

function CreateVault({ status }: { status: AppStatus }) {
  const applyStatus = useApplyStatus();
  const { keychainAvailable } = status.vault;
  const [useKeychain, setUseKeychain] = useState(keychainAvailable);
  const [password, setPassword] = useState("");
  const [confirm, setConfirm] = useState("");
  const [submitted, setSubmitted] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const passwordRequired = !useKeychain;
  const passwordError =
    passwordRequired && password.length === 0
      ? "Define una contraseña maestra."
      : password.length > 0 && password.length < MIN_MASTER_PASSWORD_LENGTH
        ? `Usa al menos ${MIN_MASTER_PASSWORD_LENGTH} caracteres.`
        : undefined;
  const confirmError = password.length > 0 && confirm !== password ? "Las contraseñas no coinciden." : undefined;

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    setSubmitted(true);
    if (passwordError || confirmError) return;
    setBusy(true);
    setError(null);
    try {
      const next = await ipc.createVault({ useKeychain, masterPassword: password || undefined });
      setPassword("");
      setConfirm("");
      applyStatus(next);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  return (
    <form onSubmit={submit} className="space-y-5" noValidate>
      <div>
        <h1 className="flex items-center gap-2 text-base font-semibold">
          <ShieldCheck size={18} className="text-accent" /> Crear bóveda cifrada
        </h1>
        <p className="mt-1 text-[13px] text-muted">
          Las credenciales de tus instancias Odoo y la cuenta de Google Drive se guardan cifradas en este equipo.
        </p>
      </div>

      {keychainAvailable ? (
        <Switch
          checked={useKeychain}
          onChange={setUseKeychain}
          label="Guardar la clave en el llavero del sistema"
          description="Abre la bóveda automáticamente al iniciar sesión en este equipo."
        />
      ) : (
        <Alert tone="warning" title="Llavero del sistema no disponible">
          No se detectó un servicio de llavero (Secret Service / Credential Manager). Necesitas una contraseña maestra para
          abrir la bóveda.
        </Alert>
      )}

      <div className="space-y-3">
        <Field
          label="Contraseña maestra"
          optional={!passwordRequired}
          error={submitted ? passwordError : undefined}
          hint={
            passwordRequired
              ? `Mínimo ${MIN_MASTER_PASSWORD_LENGTH} caracteres. No se puede recuperar si la olvidas.`
              : "Recomendado: permite recuperar el acceso si el llavero del sistema se borra o cambias de equipo."
          }
        >
          {({ id, describedBy, invalid }) => (
            <PasswordInput
              id={id}
              aria-describedby={describedBy}
              aria-invalid={invalid}
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              autoFocus={!keychainAvailable}
            />
          )}
        </Field>
        {password.length > 0 ? (
          <Field label="Confirmar contraseña" error={submitted ? confirmError : undefined}>
            {({ id, describedBy, invalid }) => (
              <PasswordInput
                id={id}
                aria-describedby={describedBy}
                aria-invalid={invalid}
                value={confirm}
                onChange={(e) => setConfirm(e.target.value)}
              />
            )}
          </Field>
        ) : null}
      </div>

      {error ? <Alert tone="danger">{error}</Alert> : null}

      <Button type="submit" variant="primary" className="w-full" loading={busy}>
        Crear bóveda
      </Button>
    </form>
  );
}

function UnlockVault({ status }: { status: AppStatus }) {
  const applyStatus = useApplyStatus();
  const { keychainEnabled, passwordEnabled } = status.vault;
  const [password, setPassword] = useState("");
  const [busy, setBusy] = useState(false);
  const [keychainTrying, setKeychainTrying] = useState(keychainEnabled);
  const [error, setError] = useState<string | null>(null);
  const attempted = useRef(false);

  const tryKeychain = async () => {
    setKeychainTrying(true);
    setError(null);
    try {
      applyStatus(await ipc.unlockVault({}));
    } catch (err) {
      const appError = toAppError(err);
      setError(
        passwordEnabled
          ? `${errorMessage(appError)} Ingresa la contraseña maestra.`
          : errorMessage(appError),
      );
      setKeychainTrying(false);
    }
  };

  useEffect(() => {
    // Un solo intento automático con el llavero (StrictMode monta dos veces en desarrollo).
    if (!keychainEnabled || attempted.current) return;
    attempted.current = true;
    void tryKeychain();
  }, [keychainEnabled]); // tryKeychain solo lee props estables del estado inicial.

  const submit = async (event: FormEvent) => {
    event.preventDefault();
    if (!password) {
      setError("Ingresa la contraseña maestra.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const next = await ipc.unlockVault({ masterPassword: password });
      setPassword("");
      applyStatus(next);
    } catch (err) {
      setError(errorMessage(err));
      setBusy(false);
    }
  };

  if (keychainTrying) {
    return (
      <div className="flex flex-col items-center gap-3 py-6 text-center">
        <Spinner size={20} />
        <p className="text-sm">Abriendo la bóveda con el llavero del sistema…</p>
        <p className="text-xs text-muted">Si el sistema lo pide, desbloquea tu llavero.</p>
      </div>
    );
  }

  return (
    <form onSubmit={submit} className="space-y-5" noValidate>
      <div>
        <h1 className="flex items-center gap-2 text-base font-semibold">
          <LockKeyhole size={18} className="text-accent" /> Bóveda bloqueada
        </h1>
        <p className="mt-1 text-[13px] text-muted">Desbloquéala para ver y respaldar tus instancias.</p>
      </div>

      {error ? <Alert tone="danger">{error}</Alert> : null}

      {passwordEnabled ? (
        <>
          <Field label="Contraseña maestra">
            {({ id, describedBy, invalid }) => (
              <PasswordInput
                id={id}
                aria-describedby={describedBy}
                aria-invalid={invalid}
                value={password}
                onChange={(e) => setPassword(e.target.value)}
                autoComplete="current-password"
                autoFocus
              />
            )}
          </Field>
          <Button type="submit" variant="primary" className="w-full" loading={busy} icon={<KeyRound size={15} />}>
            Desbloquear
          </Button>
        </>
      ) : null}

      {keychainEnabled ? (
        <Button type="button" variant={passwordEnabled ? "ghost" : "primary"} className="w-full" onClick={tryKeychain} disabled={busy}>
          Reintentar con el llavero del sistema
        </Button>
      ) : null}

      {!passwordEnabled && !keychainEnabled ? (
        <Alert tone="danger">La bóveda no tiene métodos de desbloqueo configurados.</Alert>
      ) : null}
    </form>
  );
}
