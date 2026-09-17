import { useEffect, useState } from "react";

import { Dialog } from "../../components/Dialog";
import { MOCK_OPEN_WINDOW_EVENT, type MockOpenWindowDetail } from "../../lib/mock-backend";
import type { PluginView } from "../../lib/types";
import { PluginFrame } from "./PluginFrame";
import { usePlugins } from "./usePlugins";

/**
 * Solo en el navegador (`pnpm dev`): el mock no puede crear ventanas nativas, así que muestra
 * la ventana del plugin en un diálogo con el mismo puente (superficie "window").
 */
export function MockPluginWindowDialog({ appVersion, onOpenSettings }: { appVersion: string; onOpenSettings: (pluginId: string) => void }) {
  const plugins = usePlugins();
  const [detail, setDetail] = useState<MockOpenWindowDetail | null>(null);

  useEffect(() => {
    const onOpen = (event: Event) => setDetail((event as CustomEvent<MockOpenWindowDetail>).detail);
    window.addEventListener(MOCK_OPEN_WINDOW_EVENT, onOpen);
    return () => window.removeEventListener(MOCK_OPEN_WINDOW_EVENT, onOpen);
  }, []);

  const plugin: PluginView | undefined = detail ? plugins.data?.find((p) => p.id === detail.pluginId) : undefined;
  const pluginWindow = detail ? plugin?.windows.find((w) => w.id === detail.windowId) : undefined;

  return (
    <Dialog
      open={Boolean(plugin && pluginWindow)}
      onClose={() => setDetail(null)}
      title={pluginWindow && plugin ? `${pluginWindow.title} · ${plugin.name}` : "Ventana de plugin"}
      description="Vista previa en el navegador (en la app se abre una ventana nativa)."
      size="lg"
    >
      {plugin && pluginWindow && detail ? (
        <div className="-mx-5 -my-4 h-[60vh]">
          <PluginFrame
            plugin={plugin}
            path={pluginWindow.path}
            title={pluginWindow.title}
            surface="window"
            params={detail.params}
            appVersion={appVersion}
            onOpenSettings={() => {
              setDetail(null);
              onOpenSettings(plugin.id);
            }}
          />
        </div>
      ) : null}
    </Dialog>
  );
}
