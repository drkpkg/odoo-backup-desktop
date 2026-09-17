import obd from "../../_sdk/obd-plugin.js";

const context = await obd.context();
document.querySelector("#status").textContent = `${context.pluginId} v${context.pluginVersion}`;

const instances = await obd.instances.list();
const list = document.querySelector("#instances");
for (const instance of instances) {
  const item = document.createElement("li");
  item.textContent = `${instance.name} (${instance.database})`;
  list.append(item);
}

document.querySelector("#hello").addEventListener("click", async () => {
  const { values } = await obd.settings.get();
  await obd.ui.toast({ kind: "success", title: `${values.greeting ?? "Hola"} desde __PLUGIN_NAME__` });
});
document.querySelector("#settings").addEventListener("click", () => obd.ui.openSettings());
