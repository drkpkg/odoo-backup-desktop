import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState, type RefObject } from "react";

import { Alert } from "../../components/Alert";
import { PageHeader } from "../../components/Layout";
import { Spinner } from "../../components/Spinner";
import { errorMessage } from "../../lib/errors";
import { ipc } from "../../lib/ipc";
import { queryKeys } from "../../lib/query";
import type { AppStatus } from "../../lib/types";
import { usePlugins } from "../plugins/usePlugins";
import { DriveSection } from "./DriveSection";
import { GeneralSection } from "./GeneralSection";
import { pluginsWithSettings, PluginsSettingsSection } from "./PluginsSettingsSection";
import {
  SETTINGS_SECTIONS,
  settingsAnchor,
  SettingsDirtyProvider,
  useDirtySections,
  type SettingsFocus,
  type SettingsSectionId,
} from "./sections";
import { SecuritySection } from "./SecuritySection";

export function SettingsPage({ status, focus = null }: { status: AppStatus; focus?: SettingsFocus | null }) {
  return (
    <SettingsDirtyProvider>
      <SettingsContent status={status} focus={focus} />
    </SettingsDirtyProvider>
  );
}

function SettingsContent({ status, focus }: { status: AppStatus; focus: SettingsFocus | null }) {
  const settings = useQuery({ queryKey: queryKeys.settings, queryFn: () => ipc.getSettings() });
  const plugins = usePlugins();
  const showPlugins = !plugins.isSuccess || pluginsWithSettings(plugins.data).length > 0 || Boolean(focus?.pluginId);
  const sections = useMemo(() => SETTINGS_SECTIONS.filter((section) => section.id !== "plugins" || showPlugins), [showPlugins]);
  const loaded = Boolean(settings.data);
  const contentRef = useRef<HTMLDivElement>(null);
  const [requested, setRequested] = useState<{ section: SettingsSectionId; nonce: number } | null>(null);
  // Sección pedida (clic o apertura desde otra pantalla) mientras el usuario no desplace a mano:
  // sigue marcada en la navegación y se realinea si el contenido de arriba cambia de alto al cargar.
  const pinned = useRef<{ section: SettingsSectionId; align: boolean } | null>(null);

  const request = (section: SettingsSectionId, align = true) => {
    pinned.current = { section, align };
    setRequested({ section, nonce: Date.now() });
    if (align) goTo(section, true);
  };

  // Abrir en una sección concreta (los ajustes de un plugin se enfocan desde su propio bloque).
  useEffect(() => {
    if (!focus || !loaded) return;
    request(focus.section, !focus.pluginId);
  }, [focus, loaded]);

  useEffect(() => {
    const content = contentRef.current;
    const scroller = content?.closest("main");
    if (!content || !scroller) return;
    const observer = new ResizeObserver(() => {
      if (pinned.current?.align) goTo(pinned.current.section, false);
    });
    observer.observe(content);
    const release = () => {
      pinned.current = null;
    };
    scroller.addEventListener("wheel", release, { passive: true });
    scroller.addEventListener("touchmove", release, { passive: true });
    scroller.addEventListener("keydown", release);
    return () => {
      observer.disconnect();
      scroller.removeEventListener("wheel", release);
      scroller.removeEventListener("touchmove", release);
      scroller.removeEventListener("keydown", release);
    };
  }, [loaded]);

  return (
    <>
      <PageHeader title="Ajustes" description="Carpeta de descarga, retención, seguridad de la bóveda, Google Drive y plugins." />
      {loaded ? <SectionNav sections={sections} requested={requested} pinned={pinned} onRequest={request} /> : null}
      <div ref={contentRef} className="mx-auto max-w-3xl space-y-5 px-page py-section">
        {settings.isPending ? <Spinner label="Cargando ajustes…" /> : null}
        {settings.isError ? <Alert tone="danger">{errorMessage(settings.error)}</Alert> : null}
        {settings.data ? (
          <>
            <GeneralSection settings={settings.data} />
            <SecuritySection status={status} />
            <DriveSection settings={settings.data} />
            <PluginsSettingsSection focusPluginId={focus?.pluginId ?? null} />
          </>
        ) : null}
      </div>
    </>
  );
}

/** Lleva una sección bajo la barra de navegación; con `focus`, le pasa el foco (teclado y lectores). */
function goTo(section: SettingsSectionId, focus: boolean) {
  const target = document.getElementById(settingsAnchor(section));
  if (!target) return;
  target.scrollIntoView({ block: "start" });
  if (focus) target.focus({ preventScroll: true });
}

/**
 * Navegación interna fija: un clic lleva a cada sección, marca la sección visible y avisa con un
 * punto de las secciones con cambios sin guardar.
 */
function SectionNav({
  sections,
  requested,
  pinned,
  onRequest,
}: {
  sections: { id: SettingsSectionId; label: string }[];
  requested: { section: SettingsSectionId; nonce: number } | null;
  pinned: RefObject<{ section: SettingsSectionId } | null>;
  onRequest: (section: SettingsSectionId) => void;
}) {
  const dirty = useDirtySections();
  const [active, setActive] = useState<SettingsSectionId>(sections[0]?.id ?? "backups");
  const navRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (requested) setActive(requested.section);
  }, [requested]);

  useEffect(() => {
    const scroller = navRef.current?.closest("main");
    if (!scroller) return;
    let frame = 0;
    const update = () => {
      frame = 0;
      // Las últimas secciones pueden no llegar arriba: la pedida sigue marcada hasta desplazar a mano.
      if (pinned.current) {
        setActive(pinned.current.section);
        return;
      }
      const navBottom = navRef.current?.getBoundingClientRect().bottom ?? 0;
      const atEnd = scroller.scrollTop + scroller.clientHeight >= scroller.scrollHeight - 2;
      let current = sections[0]?.id;
      for (const section of sections) {
        const element = document.getElementById(settingsAnchor(section.id));
        if (element && element.getBoundingClientRect().top <= navBottom + 24) current = section.id;
      }
      if (atEnd) current = sections.at(-1)?.id ?? current;
      if (current) setActive(current);
    };
    const onScroll = () => {
      if (!frame) frame = requestAnimationFrame(update);
    };
    update();
    scroller.addEventListener("scroll", onScroll, { passive: true });
    window.addEventListener("resize", onScroll);
    return () => {
      scroller.removeEventListener("scroll", onScroll);
      window.removeEventListener("resize", onScroll);
      if (frame) cancelAnimationFrame(frame);
    };
  }, [sections, pinned]);

  return (
    <nav ref={navRef} aria-label="Secciones de ajustes" className="sticky top-0 z-20 border-b border-border bg-bg/95 px-page backdrop-blur">
      <ul className="mx-auto flex max-w-3xl gap-1 overflow-x-auto overflow-y-hidden">
        {sections.map((section) => {
          const current = section.id === active;
          const changed = dirty.has(section.id);
          return (
            <li key={section.id} className="shrink-0">
              <a
                href={`#${settingsAnchor(section.id)}`}
                aria-current={current ? "location" : undefined}
                onClick={(event) => {
                  event.preventDefault();
                  onRequest(section.id);
                }}
                className={`flex items-center gap-1.5 border-b-2 px-3 py-2.5 text-[13px] whitespace-nowrap transition-colors ${
                  current ? "border-accent font-medium text-fg" : "border-transparent text-muted hover:text-fg"
                }`}
              >
                {section.label}
                {changed ? (
                  <>
                    <span className="h-1.5 w-1.5 rounded-full bg-warning" aria-hidden="true" />
                    <span className="sr-only">(cambios sin guardar)</span>
                  </>
                ) : null}
              </a>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
