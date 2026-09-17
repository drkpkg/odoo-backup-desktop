// Ventana del plugin de ejemplo: muestra la instancia recibida en `params.instanceId`.
import obd from "../../_sdk/obd-plugin.js";

const $ = (selector) => document.querySelector(selector);

function formatBytes(bytes) {
  if (typeof bytes !== "number") return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(unit === 0 ? 0 : 1)} ${units[unit]}`;
}

const STATUS = {
  running: ["En curso", "info"],
  success: ["Correcto", "success"],
  failed: ["Fallido", "danger"],
  cancelled: ["Cancelado", "neutral"],
};

function field(label, value) {
  const dt = document.createElement("dt");
  dt.textContent = label;
  const dd = document.createElement("dd");
  dd.textContent = value;
  $("#fields").append(dt, dd);
}

async function main() {
  const context = await obd.context();
  $("#settings").addEventListener("click", () =>
    obd.ui.openSettings().catch((error) => obd.ui.toast({ kind: "error", title: "No se pudieron abrir los ajustes", description: error.message })),
  );

  const instanceId = typeof context.params.instanceId === "string" ? context.params.instanceId : null;
  if (!instanceId) {
    $("#subtitle").textContent = `Abierta desde la página (superficie: ${context.surface}), sin instancia seleccionada.`;
    return;
  }

  const instances = await obd.instances.list();
  const instance = instances.find((item) => item.id === instanceId);
  if (!instance) {
    $("#subtitle").textContent = "La instancia ya no existe.";
    return;
  }

  $("#title").textContent = instance.name;
  $("#subtitle").textContent = `Superficie: ${context.surface}`;
  $("#details").hidden = false;
  field("URL", instance.url);
  field("Base de datos", instance.database);
  field("Versión de Odoo", instance.odooVersion ?? "Sin probar");

  const history = await obd.history.list({ instanceId, limit: 5 });
  const body = $("#history");
  body.replaceChildren();
  if (history.length === 0) {
    const cell = body.insertRow().insertCell();
    cell.colSpan = 3;
    cell.className = "obd-muted";
    cell.textContent = "Sin respaldos todavía.";
    return;
  }
  for (const entry of history) {
    const row = body.insertRow();
    row.insertCell().textContent = new Date(entry.startedAt).toLocaleString("es");
    const status = row.insertCell();
    const badge = document.createElement("span");
    const [label, tone] = STATUS[entry.status] ?? [entry.status, "neutral"];
    badge.className = `obd-badge obd-badge-${tone}`;
    badge.textContent = label;
    status.append(badge);
    row.insertCell().textContent = formatBytes(entry.sizeBytes);
  }
}

main().catch((error) => {
  $("#subtitle").textContent = `Error: ${error.message ?? error}`;
});
