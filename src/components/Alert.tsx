import { CircleAlert, CircleCheck, Info, TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

type Tone = "info" | "success" | "warning" | "danger";

const STYLES: Record<Tone, string> = {
  info: "bg-info-soft text-info",
  success: "bg-success-soft text-success",
  warning: "bg-warning-soft text-warning",
  danger: "bg-danger-soft text-danger",
};

const ICONS: Record<Tone, ReactNode> = {
  info: <Info size={16} aria-hidden="true" />,
  success: <CircleCheck size={16} aria-hidden="true" />,
  warning: <TriangleAlert size={16} aria-hidden="true" />,
  danger: <CircleAlert size={16} aria-hidden="true" />,
};

export function Alert({ tone = "info", title, children, action }: { tone?: Tone; title?: ReactNode; children?: ReactNode; action?: ReactNode }) {
  return (
    <div role={tone === "danger" ? "alert" : "note"} className={`flex gap-2.5 rounded-lg px-3 py-2.5 text-[13px] ${STYLES[tone]}`}>
      <span className="mt-0.5 shrink-0">{ICONS[tone]}</span>
      <div className="min-w-0 flex-1">
        {title ? <p className="font-semibold">{title}</p> : null}
        {children ? <div className={`${title ? "mt-0.5" : ""} text-fg/85`}>{children}</div> : null}
      </div>
      {action ? <div className="shrink-0">{action}</div> : null}
    </div>
  );
}
