import { LoaderCircle } from "lucide-react";

export function Spinner({ size = 16, className = "", label }: { size?: number; className?: string; label?: string }) {
  return (
    <span role={label ? "status" : undefined} className={`inline-flex items-center gap-2 ${className}`}>
      <LoaderCircle size={size} className="animate-spin motion-reduce:animate-none" aria-hidden="true" />
      {label ? <span className="text-muted">{label}</span> : null}
    </span>
  );
}

/** Barra de progreso; `value` null = indeterminada. */
export function ProgressBar({ value, tone = "accent", label }: { value: number | null; tone?: "accent" | "danger"; label?: string }) {
  const color = tone === "danger" ? "bg-danger" : "bg-accent";
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-valuenow={value ?? undefined}
      className="h-1.5 w-full overflow-hidden rounded-full bg-surface-2"
    >
      {value === null ? (
        <div className={`progress-indeterminate h-full w-2/5 rounded-full ${color}`} />
      ) : (
        <div className={`h-full rounded-full transition-[width] duration-300 ${color}`} style={{ width: `${value}%` }} />
      )}
    </div>
  );
}
