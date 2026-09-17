import { CircleAlert, CircleCheck, Info, X } from "lucide-react";
import { createContext, useCallback, useContext, useMemo, useRef, useState, type ReactNode } from "react";

type ToastTone = "success" | "danger" | "info";

type Toast = { id: number; tone: ToastTone; title: string; detail?: string };

type ToastApi = {
  show: (toast: Omit<Toast, "id">) => void;
  success: (title: string, detail?: string) => void;
  error: (title: string, detail?: string) => void;
  info: (title: string, detail?: string) => void;
};

const ToastContext = createContext<ToastApi | null>(null);

const ICONS: Record<ToastTone, ReactNode> = {
  success: <CircleCheck size={16} className="text-success" aria-hidden="true" />,
  danger: <CircleAlert size={16} className="text-danger" aria-hidden="true" />,
  info: <Info size={16} className="text-info" aria-hidden="true" />,
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<Toast[]>([]);
  const nextId = useRef(1);

  const dismiss = useCallback((id: number) => setToasts((all) => all.filter((t) => t.id !== id)), []);

  const show = useCallback(
    (toast: Omit<Toast, "id">) => {
      const id = nextId.current++;
      setToasts((all) => [...all.slice(-3), { ...toast, id }]);
      setTimeout(() => dismiss(id), toast.tone === "danger" ? 9000 : 5000);
    },
    [dismiss],
  );

  const api = useMemo<ToastApi>(
    () => ({
      show,
      success: (title, detail) => show({ tone: "success", title, detail }),
      error: (title, detail) => show({ tone: "danger", title, detail }),
      info: (title, detail) => show({ tone: "info", title, detail }),
    }),
    [show],
  );

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div aria-live="polite" className="pointer-events-none fixed top-4 right-4 z-50 flex w-[min(20rem,calc(100vw-2rem))] flex-col gap-2">
        {toasts.map((toast) => (
          <div
            key={toast.id}
            role={toast.tone === "danger" ? "alert" : "status"}
            className="pointer-events-auto flex items-start gap-2.5 rounded-overlay border border-border bg-surface px-3 py-2.5 shadow-overlay"
          >
            <span className="mt-0.5">{ICONS[toast.tone]}</span>
            <div className="min-w-0 flex-1">
              <p className="text-sm font-medium">{toast.title}</p>
              {toast.detail ? <p className="mt-0.5 text-xs break-words text-muted">{toast.detail}</p> : null}
            </div>
            <button
              type="button"
              aria-label="Cerrar notificación"
              onClick={() => dismiss(toast.id)}
              className="text-subtle hover:text-fg"
            >
              <X size={14} />
            </button>
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast(): ToastApi {
  const api = useContext(ToastContext);
  if (!api) throw new Error("useToast must be used inside ToastProvider");
  return api;
}
