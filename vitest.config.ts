import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
    environment: "node",
    restoreMocks: true,
    // styles.test.ts lee las hojas con `?raw` para verificar contraste y la paleta del SDK.
    css: { include: [/styles\.css/, /obd-plugin\.css/] },
  },
});
