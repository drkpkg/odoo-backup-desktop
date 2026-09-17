import { Ellipsis } from "lucide-react";
import { useEffect, useId, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactNode } from "react";
import { createPortal } from "react-dom";

export type ActionMenuItem = {
  id: string;
  label: string;
  icon?: ReactNode;
  /** Texto secundario a la derecha (p. ej. el nombre del plugin). */
  hint?: string;
  tone?: "default" | "danger";
  disabled?: boolean;
  /** Motivo mostrado como tooltip cuando está deshabilitado. */
  disabledReason?: string;
  onSelect: () => void;
};

export type ActionMenuSection = { id: string; title?: string; items: ActionMenuItem[] };

const ITEM_HEIGHT = 36;
const TITLE_HEIGHT = 28;

/**
 * Menú de acciones accesible (patrón menu button): Enter/Espacio/↓ abren, ↑/↓/Inicio/Fin navegan
 * saltando opciones deshabilitadas, Escape cierra y devuelve el foco. Se renderiza en un portal con
 * posición `fixed` junto al botón: así no lo recortan contenedores con overflow ni container queries.
 */
export function ActionMenu({
  label,
  sections,
  buttonClassName = "",
}: {
  /** Nombre accesible del botón y del menú, p. ej. "Más acciones para Cliente 1". */
  label: string;
  sections: ActionMenuSection[];
  buttonClassName?: string;
}) {
  const [open, setOpen] = useState(false);
  const [position, setPosition] = useState<CSSProperties>({});
  const buttonRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const menuId = useId();
  const visible = sections.filter((section) => section.items.length > 0);

  const enabledItems = () =>
    [...(menuRef.current?.querySelectorAll<HTMLButtonElement>('[role="menuitem"]') ?? [])].filter(
      (item) => item.getAttribute("aria-disabled") !== "true",
    );

  useEffect(() => {
    if (!open) return;
    enabledItems()[0]?.focus({ preventScroll: true });
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

  if (visible.length === 0) return null;

  const openMenu = () => {
    if (buttonRef.current) {
      const rect = buttonRef.current.getBoundingClientRect();
      const height =
        visible.reduce((sum, section) => sum + section.items.length * ITEM_HEIGHT + (section.title ? TITLE_HEIGHT : 0), 0) + 16;
      const openUp = rect.bottom + height + 8 > window.innerHeight && rect.top > height;
      setPosition({
        position: "fixed",
        right: Math.max(8, window.innerWidth - rect.right),
        ...(openUp ? { bottom: window.innerHeight - rect.top + 4 } : { top: rect.bottom + 4 }),
      });
    }
    setOpen(true);
  };

  const close = (focusButton: boolean) => {
    setOpen(false);
    if (focusButton) buttonRef.current?.focus();
  };

  const onMenuKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const items = enabledItems();
    const index = items.indexOf(document.activeElement as HTMLButtonElement);
    const focusAt = (next: number) => items[(next + items.length) % items.length]?.focus();
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        close(true);
        break;
      case "ArrowDown":
        event.preventDefault();
        focusAt(index + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        focusAt(index - 1);
        break;
      case "Home":
        event.preventDefault();
        focusAt(0);
        break;
      case "End":
        event.preventDefault();
        focusAt(items.length - 1);
        break;
      case "Tab":
        // The menu lives in a portal: put focus back on the button so Tab continues from the row.
        close(true);
        break;
    }
  };

  return (
    <div>
      <button
        ref={buttonRef}
        type="button"
        aria-label={label}
        title="Más acciones"
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? menuId : undefined}
        onClick={() => (open ? close(false) : openMenu())}
        onKeyDown={(event) => {
          if ((event.key === "ArrowDown" || event.key === "ArrowUp") && !open) {
            event.preventDefault();
            openMenu();
          }
        }}
        className={`inline-flex h-7 w-7 items-center justify-center rounded-md text-muted transition-colors hover:bg-surface-2 hover:text-fg aria-expanded:bg-surface-2 aria-expanded:text-fg ${buttonClassName}`}
      >
        <Ellipsis size={16} aria-hidden="true" />
      </button>
      {open
        ? createPortal(
            <div
              ref={menuRef}
              id={menuId}
              role="menu"
              aria-label={label}
              onKeyDown={onMenuKeyDown}
              style={position}
              className="z-40 max-h-[70vh] min-w-56 overflow-y-auto rounded-overlay border border-border bg-surface py-1 shadow-overlay"
            >
              {visible.map((section, sectionIndex) => (
                <div key={section.id} role="group" aria-label={section.title}>
                  {sectionIndex > 0 ? <div role="separator" className="my-1 border-t border-border" /> : null}
                  {section.title ? (
                    <p className="px-3 pt-1.5 pb-1 text-[11px] font-medium tracking-wide text-subtle uppercase" aria-hidden="true">
                      {section.title}
                    </p>
                  ) : null}
                  {section.items.map((item) => (
                    <button
                      key={item.id}
                      type="button"
                      role="menuitem"
                      aria-disabled={item.disabled || undefined}
                      title={item.disabled ? item.disabledReason : undefined}
                      onClick={() => {
                        if (item.disabled) return;
                        close(true);
                        item.onSelect();
                      }}
                      className={`flex w-full items-center gap-2.5 px-3 py-2 text-left text-[13px] focus:outline-none aria-disabled:cursor-not-allowed aria-disabled:opacity-50 ${
                        item.tone === "danger"
                          ? "text-danger hover:bg-danger-soft focus:bg-danger-soft"
                          : "text-fg hover:bg-surface-2 focus:bg-surface-2"
                      }`}
                    >
                      {item.icon ? (
                        <span className={`shrink-0 ${item.tone === "danger" ? "" : "text-muted"}`} aria-hidden="true">
                          {item.icon}
                        </span>
                      ) : null}
                      <span className="min-w-0 flex-1 truncate">{item.label}</span>
                      {item.hint ? <span className="shrink-0 text-[11px] text-subtle">{item.hint}</span> : null}
                    </button>
                  ))}
                </div>
              ))}
            </div>,
            document.body,
          )
        : null}
    </div>
  );
}
