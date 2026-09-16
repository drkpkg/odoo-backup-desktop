import type { ButtonHTMLAttributes, ReactNode } from "react";

import { Spinner } from "./Spinner";

type Variant = "primary" | "secondary" | "ghost" | "danger";
type Size = "sm" | "md";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-accent text-accent-fg hover:bg-accent-hover border border-transparent shadow-card",
  secondary: "bg-surface text-fg border border-border-strong hover:bg-surface-2 shadow-card",
  ghost: "bg-transparent text-muted border border-transparent hover:bg-surface-2 hover:text-fg",
  danger: "bg-danger text-white border border-transparent hover:opacity-90 shadow-card",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-2.5 text-[13px] gap-1.5",
  md: "h-9 px-3.5 text-sm gap-2",
};

export type ButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  variant?: Variant;
  size?: Size;
  icon?: ReactNode;
  loading?: boolean;
};

export function Button({
  variant = "secondary",
  size = "md",
  icon,
  loading = false,
  disabled,
  className = "",
  children,
  type = "button",
  ...rest
}: ButtonProps) {
  return (
    <button
      type={type}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={`inline-flex shrink-0 items-center justify-center rounded-md font-medium whitespace-nowrap transition-colors disabled:cursor-not-allowed disabled:opacity-50 ${VARIANTS[variant]} ${SIZES[size]} ${className}`}
      {...rest}
    >
      {loading ? <Spinner size={size === "sm" ? 12 : 14} /> : icon}
      {children}
    </button>
  );
}

type IconButtonProps = ButtonHTMLAttributes<HTMLButtonElement> & {
  label: string;
  icon: ReactNode;
  tone?: "default" | "danger";
};

/** Botón solo-icono con etiqueta accesible y tooltip nativo. */
export function IconButton({ label, icon, tone = "default", className = "", type = "button", ...rest }: IconButtonProps) {
  const toneClass = tone === "danger" ? "hover:text-danger hover:bg-danger-soft" : "hover:text-fg hover:bg-surface-2";
  return (
    <button
      type={type}
      aria-label={label}
      title={label}
      className={`inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors disabled:cursor-not-allowed disabled:opacity-40 ${toneClass} ${className}`}
      {...rest}
    >
      {icon}
    </button>
  );
}
