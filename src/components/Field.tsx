import { Eye, EyeOff } from "lucide-react";
import { forwardRef, useId, useState, type InputHTMLAttributes, type ReactNode, type SelectHTMLAttributes } from "react";

/** `w-full` salvo que `className` ya fije un ancho. */
function widthFor(className: string): string {
  return /(^|\s)w-/.test(className) ? "" : "w-full";
}

const CONTROL =
  "h-control rounded-md border border-border-strong bg-surface px-2.5 text-sm text-fg placeholder:text-subtle transition-colors hover:border-subtle focus:border-accent focus:outline-none focus:ring-2 focus:ring-accent/25 disabled:cursor-not-allowed disabled:opacity-60 aria-[invalid=true]:border-danger";

export type FieldProps = {
  label: ReactNode;
  hint?: ReactNode;
  error?: string;
  children: (ids: { id: string; describedBy: string | undefined; invalid: boolean }) => ReactNode;
  className?: string;
  optional?: boolean;
};

/** Etiqueta + control + ayuda/error con los atributos ARIA conectados. */
export function Field({ label, hint, error, children, className = "", optional }: FieldProps) {
  const id = useId();
  const hintId = `${id}-hint`;
  const errorId = `${id}-error`;
  const describedBy = [hint ? hintId : null, error ? errorId : null].filter(Boolean).join(" ") || undefined;
  return (
    <div className={`space-y-1 ${className}`}>
      <label htmlFor={id} className="flex items-baseline gap-1.5 text-[13px] font-medium text-fg">
        {label}
        {optional ? <span className="text-xs font-normal text-subtle">(opcional)</span> : null}
      </label>
      {children({ id, describedBy, invalid: Boolean(error) })}
      {error ? (
        <p id={errorId} className="text-xs text-danger">
          {error}
        </p>
      ) : hint ? (
        <p id={hintId} className="text-xs text-muted">
          {hint}
        </p>
      ) : null}
    </div>
  );
}

export const TextInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function TextInput(
  { className = "", ...props },
  ref,
) {
  return <input ref={ref} className={`${CONTROL} ${widthFor(className)} ${className}`} {...props} />;
});

export const PasswordInput = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement>>(function PasswordInput(
  { className = "", ...props },
  ref,
) {
  const [visible, setVisible] = useState(false);
  return (
    <div className="relative">
      <input
        ref={ref}
        type={visible ? "text" : "password"}
        autoComplete="new-password"
        spellCheck={false}
        className={`${CONTROL} w-full pr-9 ${className}`}
        {...props}
      />
      <button
        type="button"
        onClick={() => setVisible((v) => !v)}
        aria-label={visible ? "Ocultar" : "Mostrar"}
        title={visible ? "Ocultar" : "Mostrar"}
        className="absolute inset-y-0 right-0 flex w-9 items-center justify-center text-subtle hover:text-fg"
      >
        {visible ? <EyeOff size={15} /> : <Eye size={15} />}
      </button>
    </div>
  );
});

export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(function Select(
  { className = "", children, ...props },
  ref,
) {
  return (
    <select ref={ref} className={`${CONTROL} ${widthFor(className)} pr-8 ${className}`} {...props}>
      {children}
    </select>
  );
});

type CheckboxProps = Omit<InputHTMLAttributes<HTMLInputElement>, "type"> & { label: ReactNode; description?: ReactNode };

export const Checkbox = forwardRef<HTMLInputElement, CheckboxProps>(function Checkbox({ label, description, id, ...props }, ref) {
  const autoId = useId();
  const inputId = id ?? autoId;
  return (
    <div className="flex items-start gap-2.5">
      <input
        ref={ref}
        id={inputId}
        type="checkbox"
        className="mt-0.5 h-4 w-4 shrink-0 cursor-pointer rounded border-border-strong accent-[var(--app-accent)]"
        {...props}
      />
      <label htmlFor={inputId} className="cursor-pointer text-sm leading-tight">
        <span className="font-medium">{label}</span>
        {description ? <span className="mt-0.5 block text-xs text-muted">{description}</span> : null}
      </label>
    </div>
  );
});

export type SwitchProps = {
  checked: boolean;
  onChange: (checked: boolean) => void;
  label: ReactNode;
  description?: ReactNode;
  disabled?: boolean;
};

export function Switch({ checked, onChange, label, description, disabled }: SwitchProps) {
  const id = useId();
  return (
    <div className="flex items-start justify-between gap-4">
      <label htmlFor={id} className={`text-sm leading-tight ${disabled ? "opacity-60" : "cursor-pointer"}`}>
        <span className="font-medium">{label}</span>
        {description ? <span className="mt-0.5 block text-xs text-muted">{description}</span> : null}
      </label>
      <button
        id={id}
        type="button"
        role="switch"
        aria-checked={checked}
        disabled={disabled}
        onClick={() => onChange(!checked)}
        className={`relative h-5 w-9 shrink-0 rounded-full transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${checked ? "bg-accent" : "bg-border-strong"}`}
      >
        <span
          className={`absolute top-0.5 left-0.5 h-4 w-4 rounded-full bg-white shadow transition-transform ${checked ? "translate-x-4" : ""}`}
        />
      </button>
    </div>
  );
}

type NullableNumberProps = {
  value: number | null;
  onChange: (value: number | null) => void;
  min: number;
  max: number;
  nullLabel: string;
  unit?: string;
  id?: string;
  describedBy?: string;
};

/** Número con opción "sin límite" (null). */
export function NullableNumberInput({ value, onChange, min, max, nullLabel, unit, id, describedBy }: NullableNumberProps) {
  const [lastValue, setLastValue] = useState<number>(value ?? min);
  const checkboxId = useId();
  const disabled = value === null;
  return (
    <div className="flex flex-wrap items-center gap-3">
      <div className="flex items-center gap-2">
        <input
          id={id}
          type="number"
          min={min}
          max={max}
          disabled={disabled}
          aria-describedby={describedBy}
          value={disabled ? "" : value}
          onChange={(event) => {
            const parsed = Number.parseInt(event.target.value, 10);
            if (Number.isNaN(parsed)) return;
            const clamped = Math.min(max, Math.max(min, parsed));
            setLastValue(clamped);
            onChange(clamped);
          }}
          className={`${CONTROL} w-24 tabular`}
        />
        {unit ? <span className="text-sm text-muted">{unit}</span> : null}
      </div>
      <label htmlFor={checkboxId} className="flex cursor-pointer items-center gap-2 text-sm text-muted">
        <input
          id={checkboxId}
          type="checkbox"
          checked={disabled}
          onChange={(event) => onChange(event.target.checked ? null : lastValue)}
          className="h-4 w-4 accent-[var(--app-accent)]"
        />
        {nullLabel}
      </label>
    </div>
  );
}
