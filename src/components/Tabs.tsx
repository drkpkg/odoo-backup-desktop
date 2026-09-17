import { useId, useRef, type KeyboardEvent, type ReactNode } from "react";

export type TabItem<T extends string> = { id: T; label: ReactNode };

/**
 * Pestañas accesibles (patrón tabs con activación automática): ←/→/Inicio/Fin cambian de pestaña.
 * Usa `tabPanelProps(id)` en cada panel para conectar los atributos ARIA.
 */
export function useTabs<T extends string>() {
  const base = useId();
  return {
    tabId: (id: T) => `${base}-tab-${id}`,
    panelId: (id: T) => `${base}-panel-${id}`,
  };
}

export function Tabs<T extends string>({
  label,
  items,
  value,
  onChange,
  ids,
}: {
  label: string;
  items: TabItem<T>[];
  value: T;
  onChange: (value: T) => void;
  ids: ReturnType<typeof useTabs<T>>;
}) {
  const refs = useRef(new Map<T, HTMLButtonElement>());

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const index = items.findIndex((item) => item.id === value);
    const target =
      event.key === "ArrowRight"
        ? items[(index + 1) % items.length]
        : event.key === "ArrowLeft"
          ? items[(index - 1 + items.length) % items.length]
          : event.key === "Home"
            ? items[0]
            : event.key === "End"
              ? items.at(-1)
              : undefined;
    if (!target) return;
    event.preventDefault();
    onChange(target.id);
    refs.current.get(target.id)?.focus();
  };

  return (
    <div role="tablist" aria-label={label} onKeyDown={onKeyDown} className="flex gap-1 border-b border-border">
      {items.map((item) => {
        const selected = item.id === value;
        return (
          <button
            key={item.id}
            ref={(element) => {
              if (element) refs.current.set(item.id, element);
              else refs.current.delete(item.id);
            }}
            type="button"
            role="tab"
            id={ids.tabId(item.id)}
            aria-selected={selected}
            aria-controls={ids.panelId(item.id)}
            tabIndex={selected ? 0 : -1}
            onClick={() => onChange(item.id)}
            className={`-mb-px flex items-center gap-1.5 border-b-2 px-3 py-2.5 text-[13px] whitespace-nowrap transition-colors ${
              selected ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg"
            }`}
          >
            {item.label}
          </button>
        );
      })}
    </div>
  );
}
