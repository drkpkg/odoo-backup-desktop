import { useEffect, useRef } from "react";

import { useToast } from "../../components/Toast";
import { ipc } from "../../lib/ipc";
import type { PluginView } from "../../lib/types";
import { bridgeEvent, handleBridgeMessage, pluginAssetUrl, type BridgeHost, type BridgeSurface, type BridgeTheme } from "./bridge";

function currentTheme(): BridgeTheme {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export type PluginFrameProps = {
  plugin: PluginView;
  path: string;
  title: string;
  surface: BridgeSurface;
  params: Record<string, unknown>;
  appVersion: string;
  onNavigate?: (pageId: string, params: Record<string, unknown>) => void;
  onOpenSettings: () => void | Promise<void>;
  className?: string;
};

/**
 * Página o ventana de un plugin en un iframe aislado (origen opaco, sin IPC de Tauri).
 * Solo se atienden mensajes cuyo `source` es este iframe.
 */
export function PluginFrame({ plugin, path, title, surface, params, appVersion, onNavigate, onOpenSettings, className = "" }: PluginFrameProps) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const toast = useToast();

  // El host se lee desde una ref para no re-suscribir el listener en cada render.
  const hostRef = useRef<BridgeHost | null>(null);
  hostRef.current = {
    plugin,
    appVersion,
    surface,
    params,
    theme: currentTheme,
    backend: ipc,
    toast: (kind, toastTitle, description) => {
      const prefixed = `${plugin.name}: ${toastTitle}`;
      if (kind === "success") toast.success(prefixed, description);
      else if (kind === "error") toast.error(prefixed, description);
      else toast.info(prefixed, description);
    },
    navigate: surface === "page" ? onNavigate : undefined,
    openSettings: onOpenSettings,
  };

  useEffect(() => {
    const post = (message: unknown) => frameRef.current?.contentWindow?.postMessage(message, "*");

    const onMessage = (event: MessageEvent) => {
      const frame = frameRef.current;
      if (!frame || !frame.contentWindow || event.source !== frame.contentWindow) return;
      const host = hostRef.current;
      if (!host) return;
      const target = frame.contentWindow;
      void handleBridgeMessage(event.data, host).then((response) => {
        // Si el iframe se recargó mientras tanto, la respuesta ya no le corresponde.
        if (response && frameRef.current?.contentWindow === target) target.postMessage(response, "*");
      });
    };
    window.addEventListener("message", onMessage);

    const media = typeof window.matchMedia === "function" ? window.matchMedia("(prefers-color-scheme: dark)") : null;
    const onTheme = () => post(bridgeEvent("theme", { theme: currentTheme() }));
    media?.addEventListener("change", onTheme);

    let unlisten: (() => void) | undefined;
    let cancelled = false;
    void ipc
      .onPluginsChanged(() => post(bridgeEvent("plugins-changed", {})))
      .then((fn) => {
        if (cancelled) fn();
        else unlisten = fn;
      })
      .catch(() => undefined);

    return () => {
      cancelled = true;
      window.removeEventListener("message", onMessage);
      media?.removeEventListener("change", onTheme);
      unlisten?.();
    };
  }, []);

  const src = pluginAssetUrl(plugin, path);

  return (
    <iframe
      key={`${plugin.id}:${plugin.revision}:${path}`}
      ref={frameRef}
      title={title}
      src={src}
      sandbox="allow-scripts allow-forms allow-popups"
      referrerPolicy="no-referrer"
      onLoad={() => frameRef.current?.contentWindow?.postMessage(bridgeEvent("theme", { theme: currentTheme() }), "*")}
      className={`block h-full w-full border-0 bg-bg ${className}`}
    />
  );
}
