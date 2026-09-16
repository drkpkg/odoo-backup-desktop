import { useState, type ReactNode } from "react";

import { errorMessage } from "../lib/errors";
import { Button } from "./Button";
import { Dialog } from "./Dialog";
import { Alert } from "./Alert";

export type ConfirmDialogProps = {
  open: boolean;
  title: string;
  children: ReactNode;
  confirmLabel: string;
  tone?: "danger" | "primary";
  onConfirm: () => Promise<unknown> | void;
  onClose: () => void;
};

/** Confirmación dentro de la app (nunca `window.confirm`). */
export function ConfirmDialog({ open, title, children, confirmLabel, tone = "danger", onConfirm, onClose }: ConfirmDialogProps) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const close = () => {
    if (busy) return;
    setError(null);
    onClose();
  };

  const confirm = async () => {
    setBusy(true);
    setError(null);
    try {
      await onConfirm();
      setBusy(false);
      onClose();
    } catch (err) {
      setBusy(false);
      setError(errorMessage(err));
    }
  };

  return (
    <Dialog
      open={open}
      onClose={close}
      title={title}
      size="sm"
      dismissible={!busy}
      footer={
        <>
          <Button onClick={close} disabled={busy}>
            Cancelar
          </Button>
          <Button variant={tone === "danger" ? "danger" : "primary"} onClick={confirm} loading={busy} autoFocus>
            {confirmLabel}
          </Button>
        </>
      }
    >
      <div className="space-y-3 text-sm text-muted">
        {children}
        {error ? <Alert tone="danger">{error}</Alert> : null}
      </div>
    </Dialog>
  );
}
