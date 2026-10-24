// Synthetic Excalidraw fixtures, built in code so tests stay readable and self-contained.

import fs from "node:fs";
import os from "node:os";
import path from "node:path";

let seed = 1000;
function base(type, props) {
  seed += 7;
  return {
    id: `${type}-${seed}`,
    type,
    x: 0,
    y: 0,
    width: 100,
    height: 60,
    angle: 0,
    strokeColor: "#1e1e1e",
    backgroundColor: "transparent",
    fillStyle: "solid",
    strokeWidth: 2,
    strokeStyle: "solid",
    roughness: 1,
    opacity: 100,
    groupIds: [],
    frameId: null,
    roundness: null,
    seed,
    version: 1,
    versionNonce: seed,
    isDeleted: false,
    boundElements: null,
    updated: 1,
    link: null,
    locked: false,
    ...props,
  };
}

export const rect = (props = {}) => base("rectangle", props);

export const text = (value, props = {}) =>
  base("text", {
    width: value.length * 9,
    height: 25,
    text: value,
    originalText: value,
    fontSize: 20,
    fontFamily: 1,
    textAlign: "left",
    verticalAlign: "top",
    containerId: null,
    lineHeight: 1.25,
    autoResize: true,
    ...props,
  });

const TINY_SVG = Buffer.from(
  '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><circle cx="5" cy="5" r="4" fill="#e63946"/></svg>',
).toString("base64");

export function imageWithFile() {
  const fileId = "file-tiny-circle";
  return {
    element: base("image", { width: 40, height: 40, fileId, status: "saved", scale: [1, 1], crop: null }),
    files: { [fileId]: { id: fileId, mimeType: "image/svg+xml", dataURL: `data:image/svg+xml;base64,${TINY_SVG}`, created: 1 } },
  };
}

export const libraryV2 = (items, extra = {}) => ({
  type: "excalidrawlib",
  version: 2,
  source: "https://excalidraw.com",
  libraryItems: items.map((it, i) => ({ id: `item-${i}`, status: "published", created: 1, ...it })),
  ...extra,
});

export const libraryV1 = (itemElementLists) => ({ type: "excalidrawlib", version: 1, source: "https://excalidraw.com", library: itemElementLists });

export const scene = (elements) => ({ type: "excalidraw", version: 2, source: "https://excalidraw.com", elements, appState: {}, files: {} });

export function tmpDir(prefix = "x2s-") {
  return fs.mkdtempSync(path.join(os.tmpdir(), prefix));
}

export function writeJson(file, value) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  fs.writeFileSync(file, JSON.stringify(value));
  return file;
}

/** Relative '/'-separated paths of all files under dir (excluding the manifest). */
export function listFiles(dir) {
  const out = [];
  const walk = (d, rel) => {
    for (const e of fs.readdirSync(d, { withFileTypes: true })) {
      const r = rel ? `${rel}/${e.name}` : e.name;
      if (e.isDirectory()) walk(path.join(d, e.name), r);
      else if (!e.name.startsWith(".excalidraw-to-svg")) out.push(r);
    }
  };
  if (fs.existsSync(dir)) walk(dir, "");
  return out.sort();
}
