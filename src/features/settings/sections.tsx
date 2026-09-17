import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

import { Badge } from "../../components/Badge";

export type SettingsSectionId = "backups" | "security" | "drive" | "plugins";

export const SETTINGS_SECTIONS: { id: SettingsSectionId; label: string }[] = [
  { id: "backups", label: "Respaldos" },
  { id: "security", label: "Seguridad" },
  { id: "drive", label: "Google Drive" },
  { id: "plugins", label: "Plugins" },
];

export const settingsAnchor = (section: SettingsSectionId) => `ajustes-${section}`;

/** Petición de abrir Ajustes en una sección (y opcionalmente en los ajustes de un plugin). */
export type SettingsFocus = { section: SettingsSectionId; pluginId?: string; nonce: number };

type DirtyApi = {
  dirty: ReadonlySet<SettingsSectionId>;
  report: (section: SettingsSectionId, key: string, dirty: boolean) => void;
};

const DirtyContext = createContext<DirtyApi | null>(null);

/** Reúne qué bloques de cada sección tienen cambios sin guardar. */
export function SettingsDirtyProvider({ children }: { children: ReactNode }) {
  const [blocks, setBlocks] = useState<ReadonlySet<string>>(new Set());

  const report = useCallback((section: SettingsSectionId, key: string, dirty: boolean) => {
    const id = `${section}:${key}`;
    setBlocks((current) => {
      if (current.has(id) === dirty) return current;
      const next = new Set(current);
      if (dirty) next.add(id);
      else next.delete(id);
      return next;
    });
  }, []);

  const api = useMemo<DirtyApi>(
    () => ({ dirty: new Set([...blocks].map((id) => id.split(":")[0] as SettingsSectionId)), report }),
    [blocks, report],
  );
  return <DirtyContext.Provider value={api}>{children}</DirtyContext.Provider>;
}

/** Informa si un bloque tiene cambios sin guardar (sin proveedor no hace nada). */
export function useReportDirty(section: SettingsSectionId, key: string, dirty: boolean) {
  const report = useContext(DirtyContext)?.report;
  useEffect(() => {
    report?.(section, key, dirty);
  }, [report, section, key, dirty]);
  useEffect(() => () => report?.(section, key, false), [report, section, key]);
}

export function useDirtySections(): ReadonlySet<SettingsSectionId> {
  return useContext(DirtyContext)?.dirty ?? EMPTY;
}

const EMPTY: ReadonlySet<SettingsSectionId> = new Set();

/** "Cambios sin guardar" junto al título de la sección. */
export function DirtyBadge({ section }: { section: SettingsSectionId }) {
  return useDirtySections().has(section) ? <Badge tone="warning">Cambios sin guardar</Badge> : null;
}
