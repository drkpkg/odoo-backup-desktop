// Página principal del plugin de ejemplo. Todo pasa por el puente del SDK.
import obd from "../../_sdk/obd-plugin.js";

const $ = (selector) => document.querySelector(selector);

const STATUS_LABELS = { running: "En curso", success: "Completado", failed: "Fallido", cancelled: "Cancelado" };
const STATUS_TONES = { running: "info", success: "success", failed: "danger", cancelled: "neutral" };

function showError(error) {
  const message = error?.message ?? String(error);
  void obd.ui.toast({ kind: "error", title: "Error en Hola OBD", description: message }).catch(() => undefined);
}

function formatDate(iso) {
  if (!iso) return "—";
  return new Date(iso).toLocaleString("es", { dateStyle: "medium", timeStyle: "short" });
}

async function renderCounter() {
  const value = await obd.storage.get("counter");
  $("#counter").textContent = String(typeof value === "number" ? value : 0);
}

async function renderSettings() {
  const { values, secretsSet } = await obd.settings.get();
  const shown = { ...values, secretosDefinidos: secretsSet };
  $("#settings").textContent = JSON.stringify(shown, null, 2);
  return values;
}

async function renderInstances() {
  const instances = await obd.instances.list();
  const body = $("#instances");
  body.replaceChildren();
  if (instances.length === 0) {
    const row = body.insertRow();
    const cell = row.insertCell();
    cell.colSpan = 4;
    cell.className = "obd-muted";
    cell.textContent = "No hay instancias registradas.";
    return;
  }
  for (const instance of instances) {
    const row = body.insertRow();
    const name = row.insertCell();
    const strong = document.createElement("strong");
    strong.textContent = instance.name;
    const url = document.createElement("div");
    url.className = "obd-muted";
    url.textContent = `${instance.url} · ${instance.database}`;
    name.append(strong, url);

    row.insertCell().textContent = instance.odooVersion ?? "Sin probar";

    const last = row.insertCell();
    if (instance.lastBackup) {
      const badge = document.createElement("span");
      badge.className = `obd-badge obd-badge-${STATUS_TONES[instance.lastBackup.status] ?? "neutral"}`;
      badge.textContent = STATUS_LABELS[instance.lastBackup.status] ?? instance.lastBackup.status;
      const when = document.createElement("div");
      when.className = "obd-muted";
      when.textContent = formatDate(instance.lastBackup.finishedAt ?? instance.lastBackup.startedAt);
      last.append(badge, when);
    } else {
      last.textContent = "Nunca";
      last.className = "obd-muted";
    }

    const actions = row.insertCell();
    const button = document.createElement("button");
    button.type = "button";
    button.className = "obd-btn obd-btn-sm";
    button.textContent = "Detalle";
    button.addEventListener("click", () => obd.ui.openWindow("detail", { instanceId: instance.id }).catch(showError));
    actions.append(button);
  }
}

async function main() {
  const context = await obd.context();
  $("#context").textContent = `Plugin ${context.pluginId} v${context.pluginVersion} · app ${context.appVersion} · tema ${context.theme}`;
  $("#open-window").disabled = false;

  $("#increment").addEventListener("click", async () => {
    try {
      const current = await obd.storage.get("counter");
      await obd.storage.set("counter", (typeof current === "number" ? current : 0) + 1);
      await renderCounter();
    } catch (error) {
      showError(error);
    }
  });
  $("#reset").addEventListener("click", () => obd.storage.set("counter", null).then(renderCounter).catch(showError));
  $("#open-settings").addEventListener("click", () => obd.ui.openSettings().catch(showError));
  $("#open-window").addEventListener("click", () => obd.ui.openWindow("detail").catch(showError));
  for (const button of document.querySelectorAll("[data-toast]")) {
    button.addEventListener("click", () => {
      const kind = button.dataset.toast;
      obd.ui.toast({ kind, title: `Aviso de tipo ${kind}`, description: "Enviado desde el plugin de ejemplo." }).catch(showError);
    });
  }
  obd.on("plugins-changed", () => void renderSettings().catch(showError));

  const [, values] = await Promise.all([renderCounter(), renderSettings(), renderInstances()]);
  if (values?.notify === true) {
    await obd.ui.toast({ kind: "info", title: "Hola OBD está listo", description: "Desactiva «Notificar» en los ajustes." });
  }
}

main().catch(showError);
