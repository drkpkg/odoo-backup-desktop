import { QueryClientProvider } from "@tanstack/react-query";
import React from "react";
import ReactDOM from "react-dom/client";

import App from "./App";
import { ToastProvider } from "./components/Toast";
import { isPluginWindowLabel } from "./features/layout/navigation";
import { PluginWindowApp } from "./features/plugins/PluginWindowApp";
import { ipc } from "./lib/ipc";
import { createQueryClient } from "./lib/query";
import "./styles.css";

const queryClient = createQueryClient();

// Las ventanas de plugin (`plugin--<id>--<windowId>`) reutilizan el mismo bundle con un shell mínimo.
const pluginWindow = ipc.kind === "tauri" && isPluginWindowLabel(ipc.windowLabel());

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <QueryClientProvider client={queryClient}>
      <ToastProvider>{pluginWindow ? <PluginWindowApp /> : <App />}</ToastProvider>
    </QueryClientProvider>
  </React.StrictMode>,
);
