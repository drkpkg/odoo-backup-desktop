import { X } from "lucide-react";
import { useEffect, useId, useRef, type ReactNode } from "react";

import { IconButton } from "./Button";

type Size = "sm" | "md" | "lg" | "xl";

const WIDTHS: Record<Size, string> = {
  sm: "max-w-md",
  md: "max-w-xl",
  lg: "max-w-3xl",
  xl: "max-w-5xl",
};

export type DialogProps = {
  open: boolean;
  onClose: () => void;
  title: string;
  description?: ReactNode;
  children?: ReactNode;
  footer?: ReactNode;
  size?: Size;
  /** Evita cerrar con Esc o clic fuera (p. ej. mientras se guarda). */
  dismissible?: boolean;
  /**
   * Los hijos arman su propio cuerpo y pie con `DialogBody` y `DialogFooter` (p. ej. un formulario
   * cuyo botón de guardar vive en el pie). Deben ocupar el alto disponible: `flex min-h-0 flex-1 flex-col`.
   */
  plain?: boolean;
};

const FOOTER = "flex items-center justify-end gap-2 border-t border-border bg-surface-2/50 px-5 py-3";

/** Área con scroll del diálogo. */
export function DialogBody({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <div className={`min-h-0 flex-1 overflow-y-auto px-5 py-4 ${className}`}>{children}</div>;
}

/** Pie fijo del diálogo (fuera del área con scroll). */
export function DialogFooter({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <footer className={`${FOOTER} ${className}`}>{children}</footer>;
}

/**
 * Modal accesible sobre `<dialog>` nativo (foco atrapado y Esc gratis).
 * El contenido solo se monta mientras está abierto, así el estado (y los secretos
 * escritos en formularios) se descarta al cerrar.
 */
export function Dialog({ open, onClose, title, description, children, footer, size = "md", dismissible = true, plain = false }: DialogProps) {
  const ref = useRef<HTMLDialogElement>(null);
  const titleId = useId();
  const descriptionId = useId();

  useEffect(() => {
    const dialog = ref.current;
    if (!dialog) return;
    if (open && !dialog.open) {
      if (typeof dialog.showModal === "function") dialog.showModal();
      else dialog.setAttribute("open", "");
    } else if (!open && dialog.open) {
      dialog.close();
    }
  }, [open]);

  return (
    <dialog
      ref={ref}
      aria-labelledby={titleId}
      aria-describedby={description ? descriptionId : undefined}
      onCancel={(event) => {
        event.preventDefault();
        if (dismissible) onClose();
      }}
      onMouseDown={(event) => {
        if (dismissible && event.target === ref.current) onClose();
      }}
      className={`m-auto max-h-[calc(100vh-2rem)] w-[calc(100%-2rem)] ${WIDTHS[size]} overflow-hidden rounded-overlay border border-border bg-surface p-0 text-fg shadow-overlay`}
    >
      {open ? (
        <div className="flex max-h-[calc(100vh-2rem)] flex-col">
          <header className="flex items-start justify-between gap-4 border-b border-border px-5 py-4">
            <div className="min-w-0">
              <h2 id={titleId} className="text-base font-semibold">
                {title}
              </h2>
              {description ? (
                <div id={descriptionId} className="mt-0.5 text-[13px] text-muted">
                  {description}
                </div>
              ) : null}
            </div>
            {dismissible ? <IconButton label="Cerrar" icon={<X size={16} />} onClick={onClose} /> : null}
          </header>
          {plain ? children : <DialogBody>{children}</DialogBody>}
          {footer ? <DialogFooter>{footer}</DialogFooter> : null}
        </div>
      ) : null}
    </dialog>
  );
}
