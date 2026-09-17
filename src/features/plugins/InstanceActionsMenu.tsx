import { Ellipsis } from "lucide-react";
import { useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent } from "react";

import type { InstanceView } from "../../lib/types";
import type { PluginMenuEntry } from "../layout/navigation";
import { pluginIcon } from "./icons";

/** Menú "más acciones" de una instancia con las acciones que aportan los plugins. */
export function InstanceActionsMenu({
  instance,
  entries,
  onSelect,
}: {
  instance: InstanceView;
  entries: PluginMenuEntry[];
  onSelect: (entry: PluginMenuEntry, instance: InstanceView) => void;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({});
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuId = useId();

  useEffect(() => {
    if (!open) return;
    menuRef.current?.querySelector<HTMLButtonElement>('[role="menuitem"]')?.focus({ preventScroll: true });
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!menuRef.current?.contains(target) && !buttonRef.current?.contains(target)) setOpen(false);
    };
    const onViewportChange = () => setOpen(false);
    document.addEventListener("pointerdown", onPointerDown);
    window.addEventListener("resize", onViewportChange);
    document.addEventListener("scroll", onViewportChange, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown);
      window.removeEventListener("resize", onViewportChange);
      document.removeEventListener("scroll", onViewportChange, true);
    };
  }, [open]);

  // Posición fija junto al botón: la tabla tiene overflow y recortaría un menú absoluto.
  const toggleOpen = (next: boolean) => {
    if (next && buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      const estimatedHeight = 36 + entries.length * 36;
      const openUp = rect.bottom + estimatedHeight + 8 > window.innerHeight && rect.top > estimatedHeight;
      setPosition({
        position: "fixed",
        right: Math.max(8, window.innerWidth - rect.right),
        ...(openUp ? { bottom: window.innerHeight - rect.top + 4 } : { top: rect.bottom + 4 }),
      });
    }
    setOpen(next);
  };

  if (entries.length === 0) return null;

  const close = (focusButton: boolean) => {
    setOpen(false);
    if (focusButton) buttonRef.current?.focus();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = [...(menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])];
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Escape") {
      event.preventDefault();
      close(true);
    } else if (event.key === "ArrowDown") {
      event.preventDefault();
      items[(index + 1) % items.length]?.focus();
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      items[(index - 1 + items.length) % items.length]?.focus();
    } else if (event.key === "Home") {
      event.preventDefault();
      items[0]?.focus();
    } else if (event.key === "End") {
      event.preventDefault();
      items[items.length - 1]?.focus();
    } else if (event.key === "Tab") {
      close(false);
    }
  };

  return (
    <div>
      <button
        ref={buttonRef}
        type="button"
        aria-label="Más acciones"
        title="Más acciones"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => toggleOpen(!open)}
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" && !open) {
            event.preventDefault();
            toggleOpen(true);
          }
        }}
        className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg"
      >
        <Ellipsis size={15} />
      </button>
      {open ? (
        <div
          ref={menuRef}
          id={menuId}
          role="menu"
          aria-label={`Acciones de plugins para ${instance.name}`}
          onKeyDown={onMenuKeyDown}
          style={position}
          className="z-40 min-w-56 overflow-hidden rounded-lg border border-border bg-surface py-1 shadow-overlay"
        >
          <p className="px-3 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase">Plugins</p>
          {entries.map((entry) => {
            const Icon = pluginIcon(entry.menu.icon);
            return (
              <button
                key={`${entry.plugin.id}:${entry.menu.id}`}
                type="button"
                role="menuitem"
                onClick={() => {
                  close(false);
                  onSelect(entry, instance);
                }}
                className="flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] text-fg hover:bg-surface-2 focus:bg-surface-2 focus:outline-none"
              >
                <Icon size={14} className="shrink-0 text-muted" aria-hidden="true" />
                <span className="min-w-0 flex-1 truncate">{entry.menu.label}</span>
                <span className="shrink-0 text-[11px] text-subtle">{entry.plugin.name}</span>
              </button>
            );
          })}
        </div>
      ) : null}
    </div>
  );
}
