// Runs Excalidraw's own SVG exporter (@excalidraw/utils exportScene) in plain Node, with
// linkedom standing in for the browser DOM. exportScene accepts a host `environment` for
// exactly this; the few globals below cover what it still reaches for outside a browser.

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { parseHTML } from "linkedom";

// Only used to register font descriptors; nothing is rendered with them in Node.
class FontFacePolyfill {
  constructor(family, source, descriptors = {}) {
    Object.assign(this, { family, source, style: "normal", weight: "400", display: "swap", unicodeRange: "U+0-10FFFF" }, descriptors);
    this.status = "unloaded";
  }
  load() {
    this.status = "loaded";
    return Promise.resolve(this);
  }
}

// Text width is only measured for frame labels and embed placeholders (element text
// already carries its laid-out size), so an average-advance estimate is enough.
const textMetrics = {
  getLineWidth(text, fontString) {
    const px = parseFloat(/([\d.]+)px/.exec(String(fontString))?.[1] ?? "16");
    return [...String(text)].length * px * 0.55;
  },
};

let loaded;

/** Load @excalidraw/utils once. Returns { exportScene, document, version }. */
export async function loadExporter() {
  if (loaded) return loaded;
  const require = createRequire(import.meta.url);
  const entry = require.resolve("@excalidraw/utils");
  // The bundled font subsetter (Emscripten/HarfBuzz) probes CommonJS globals to find itself.
  globalThis.__dirname ??= path.dirname(entry);
  globalThis.require ??= createRequire(entry);
  globalThis.FontFace ??= FontFacePolyfill;

  // Expected, harmless diagnostics from the exporter in this setting: Node has no
  // `document.fonts`, and library items often hold arrows bound to shapes that live
  // outside the item.
  const BENIGN = [/^Excalidraw: there is no font set/, /^Could not repair binding for element/];
  for (const level of ["warn", "error", "log"]) {
    const original = console[level];
    console[level] = (...args) => {
      if (typeof args[0] === "string" && BENIGN.some((re) => re.test(args[0]))) return;
      original(...args);
    };
  }

  const { exportScene } = await import("@excalidraw/utils");
  const pkg = JSON.parse(fs.readFileSync(path.resolve(path.dirname(entry), "../../package.json"), "utf8"));
  const { document } = parseHTML("<!doctype html><html><head></head><body></body></html>");
  loaded = { exportScene, document, version: pkg.version };
  return loaded;
}

/**
 * Export elements to SVG markup.
 * opts: { theme, padding, background (CSS color | null), frames (bool), embedFonts (bool) }
 */
export async function exportSvg(elements, files, opts) {
  const { exportScene, document } = await loadExporter();
  const config = {
    theme: opts.theme,
    padding: opts.padding,
    canvasBackgroundColor: opts.background ?? false,
    environment: { document, textMetrics, loadFonts: async () => {} },
  };
  if (!opts.frames) config.frames = { name: false, outline: false, clip: true };
  if (!opts.embedFonts) config.skipInliningFonts = true;
  const svg = await exportScene({ type: "svg", data: { elements, files: files ?? null }, config });
  return svg.outerHTML;
}
