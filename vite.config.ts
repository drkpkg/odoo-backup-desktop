import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";
import { fileURLToPath } from "node:url";
import { defineConfig, type Plugin } from "vite";

const host = process.env.TAURI_DEV_HOST;
const root = path.dirname(fileURLToPath(import.meta.url));

const MIME: Record<string, string> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".ico": "image/x-icon",
  ".ts": "text/plain; charset=utf-8",
  ".md": "text/plain; charset=utf-8",
};

/**
 * Solo `pnpm dev` en el navegador: sirve los plugins de `examples/plugins/<id>/…` y el SDK de
 * `plugin-sdk/…` bajo `/__obd-plugins/`, imitando el protocolo `obd-plugin` de la app.
 */
function obdPluginsDevServer(): Plugin {
  const pluginsRoot = path.join(root, "examples", "plugins");
  const sdkRoot = path.join(root, "plugin-sdk");
  return {
    name: "obd-plugins-dev-server",
    apply: "serve",
    configureServer(server) {
      server.middlewares.use("/__obd-plugins", (req, res) => {
        const pathname = decodeURIComponent((req.url ?? "/").split(/[?#]/)[0] ?? "/");
        const [first, ...rest] = pathname.replace(/^\/+/, "").split("/");
        if (!first || rest.length === 0 || rest.some((segment) => segment === "" || segment.startsWith("."))) {
          res.statusCode = 404;
          res.end("not found");
          return;
        }
        const base = first === "_sdk" ? sdkRoot : path.join(pluginsRoot, first);
        const file = path.resolve(base, ...rest);
        if (!file.startsWith(base + path.sep) || !fs.existsSync(file) || !fs.statSync(file).isFile()) {
          res.statusCode = 404;
          res.end("not found");
          return;
        }
        res.setHeader("Content-Type", MIME[path.extname(file).toLowerCase()] ?? "application/octet-stream");
        // Los iframes con sandbox tienen origen opaco: los módulos ES requieren CORS.
        res.setHeader("Access-Control-Allow-Origin", "*");
        res.setHeader("Cache-Control", "no-store");
        fs.createReadStream(file).pipe(res);
      });
    },
  };
}

// https://vite.dev/config/
export default defineConfig(() => ({
  plugins: [react(), tailwindcss(), obdPluginsDevServer()],

  // Tauri: no ocultar errores de Rust y puerto fijo.
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: {
      ignored: ["**/src-tauri/**", "**/crates/**", "**/target/**"],
    },
  },
  build: {
    target: "es2022",
    sourcemap: false,
    // App de escritorio: el bundle se carga desde disco, no por red.
    chunkSizeWarningLimit: 800,
  },
}));
