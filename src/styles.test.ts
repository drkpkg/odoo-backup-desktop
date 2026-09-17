import { describe, expect, it } from "vitest";

import sdkCss from "../plugin-sdk/obd-plugin.css?raw";
import appCss from "./styles.css?raw";

type Tokens = Record<string, string>;

/** Variables `--<prefix>-*` declaradas dentro del primer bloque que abre `selector`. */
function tokens(css: string, selector: string, prefix: string): Tokens {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`selector not found: ${selector}`);
  let depth = 0;
  let body = "";
  for (let i = css.indexOf("{", start); i < css.length; i += 1) {
    const char = css[i];
    if (char === "{") depth += 1;
    if (char === "}") depth -= 1;
    body += char;
    if (depth === 0) break;
  }
  const pattern = new RegExp(`--${prefix}-([\\w-]+):\\s*([^;]+);`, "g");
  return Object.fromEntries([...body.matchAll(pattern)].map((match) => [match[1]!, match[2]!.trim()]));
}

function luminance(hex: string): number {
  const channels = [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255);
  const [r, g, b] = channels.map((v) => (v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4));
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}

function contrast(a: string, b: string): number {
  const [light, dark] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (light! + 0.05) / (dark! + 0.05);
}

const appLight = tokens(appCss, ":root {", "app");
const appDark = { ...appLight, ...tokens(appCss, "@media (prefers-color-scheme: dark)", "app") };

const TEXT = ["text", "muted", "subtle", "accent", "success", "warning", "danger", "info"];
const BACKGROUNDS = ["bg", "surface", "surface-2", "sidebar"];
const SOFT_PAIRS: [string, string][] = [
  ["accent", "accent-soft"],
  ["success", "success-soft"],
  ["warning", "warning-soft"],
  ["danger", "danger-soft"],
  ["info", "info-soft"],
  ["accent-fg", "accent"],
];

describe("theme contrast (WCAG AA, 4.5:1 for text)", () => {
  for (const [name, theme] of [
    ["light", appLight],
    ["dark", appDark],
  ] as const) {
    it(`${name}: text tokens on every background`, () => {
      const failures = TEXT.flatMap((fg) =>
        BACKGROUNDS.map((bg) => ({ pair: `${fg}/${bg}`, ratio: contrast(theme[fg]!, theme[bg]!) })),
      ).filter((result) => result.ratio < 4.5);
      expect(failures).toEqual([]);
    });

    it(`${name}: badges and primary buttons`, () => {
      const failures = SOFT_PAIRS.map(([fg, bg]) => ({ pair: `${fg}/${bg}`, ratio: contrast(theme[fg]!, theme[bg]!) })).filter(
        (result) => result.ratio < 4.5,
      );
      expect(failures).toEqual([]);
    });
  }
});

describe("plugin SDK stays in sync with the app", () => {
  /** Tokens que solo usa la ventana principal. */
  const APP_ONLY = new Set(["sidebar"]);
  const colorKeys = Object.keys(appLight).filter((key) => appLight[key]!.startsWith("#") && !APP_ONLY.has(key));

  it("uses the same light palette", () => {
    const sdk = tokens(sdkCss, ":root {", "obd");
    for (const key of colorKeys) expect(sdk[key], key).toBe(appLight[key]);
  });

  it("uses the same dark palette (system preference and data-theme)", () => {
    const system = tokens(sdkCss, "@media (prefers-color-scheme: dark)", "obd");
    const forced = tokens(sdkCss, ':root[data-theme="dark"]', "obd");
    const darkKeys = Object.keys(tokens(appCss, "@media (prefers-color-scheme: dark)", "app")).filter(
      (key) => appDark[key]!.startsWith("#") && !APP_ONLY.has(key),
    );
    for (const key of darkKeys) {
      expect(system[key], `system ${key}`).toBe(appDark[key]);
      expect(forced[key], `data-theme ${key}`).toBe(appDark[key]);
    }
  });

  it("shares density and shape tokens", () => {
    const sdk = tokens(sdkCss, ":root {", "obd");
    const pairs: [string, string][] = [
      ["space-page", "space-page"],
      ["space-section", "space-section"],
      ["control-height", "control-height"],
      ["control-height-sm", "control-height-sm"],
      ["radius-card", "radius-card"],
      ["radius-overlay", "radius-overlay"],
      ["shadow-lg", "shadow-overlay"],
    ];
    // La app usa rem sobre 16 px; el SDK usa px porque las páginas de plugin fijan `html` en 14 px.
    const toPx = (value: string) => value.replace(/(\d*\.?\d+)rem\b/g, (_, n: string) => `${Number(n) * 16}px`);
    for (const [app, plugin] of pairs) expect(sdk[plugin], plugin).toBe(toPx(appLight[app]!));
  });
});
