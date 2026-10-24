// Reads Excalidraw files into one shape: a list of items, each a list of elements.
//
//   .excalidrawlib v1   { type: "excalidrawlib", version: 1, library: Element[][] }
//   .excalidrawlib v2   { type: "excalidrawlib", version: 2, libraryItems: [{ id, name?, elements }] }
//   .excalidraw         { type: "excalidraw", elements, appState?, files? }   (one item)
//
// Item indices are 0-based positions in the file, matching Excalidraw catalogues ("#0")
// and sidecar metadata keys. Deleted elements are dropped; an item left empty is kept in
// the list (so indices stay stable) but flagged `empty`.

import path from "node:path";

export const LIBRARY_EXT = ".excalidrawlib";
export const SCENE_EXT = ".excalidraw";

export class SourceError extends Error {}

/** "library" | "scene" | null, by file extension (case-insensitive). */
export function kindOf(fileName) {
  const lower = fileName.toLowerCase();
  if (lower.endsWith(LIBRARY_EXT)) return "library";
  if (lower.endsWith(SCENE_EXT)) return "scene";
  return null;
}

/** File name without the Excalidraw extension. */
export function baseNameOf(fileName) {
  const name = path.basename(fileName);
  const kind = kindOf(name);
  return kind ? name.slice(0, name.length - (kind === "library" ? LIBRARY_EXT : SCENE_EXT).length) : name;
}

function visible(elements) {
  return Array.isArray(elements) ? elements.filter((e) => e && typeof e === "object" && !e.isDeleted) : [];
}

function item(index, elements, extra = {}) {
  const els = visible(elements);
  return { index, id: null, name: null, ...extra, elements: els, empty: els.length === 0 };
}

/**
 * Parse file text. Returns { kind, version, source, files, items }.
 * `files` is the Excalidraw BinaryFiles map (image data URLs) or null.
 */
export function parseSource(text, fileName) {
  const kind = kindOf(fileName);
  if (!kind) throw new SourceError(`not an Excalidraw file: ${fileName}`);
  let json;
  try {
    json = JSON.parse(text.charCodeAt(0) === 0xfeff ? text.slice(1) : text);
  } catch (e) {
    throw new SourceError(`invalid JSON: ${e.message}`);
  }

  if (kind === "scene") {
    if (!json || !Array.isArray(json.elements)) throw new SourceError("scene has no elements array");
    return {
      kind,
      version: json.version ?? null,
      source: typeof json.source === "string" ? json.source : null,
      files: json.files && typeof json.files === "object" ? json.files : null,
      items: [item(0, json.elements, { name: baseNameOf(fileName) })],
    };
  }

  // Very early libraries were a bare array of items.
  const lib = Array.isArray(json) ? { version: 1, library: json } : json;
  if (!lib || typeof lib !== "object") throw new SourceError("library is not an object");
  let items;
  if (Array.isArray(lib.libraryItems)) {
    items = lib.libraryItems.map((it, i) =>
      item(i, it?.elements, {
        id: typeof it?.id === "string" ? it.id : null,
        name: typeof it?.name === "string" && it.name.trim() ? it.name.trim() : null,
      }),
    );
  } else if (Array.isArray(lib.library)) {
    items = lib.library.map((els, i) => item(i, els));
  } else {
    throw new SourceError("library has neither libraryItems (v2) nor library (v1)");
  }
  return {
    kind,
    version: lib.version ?? null,
    source: typeof lib.source === "string" ? lib.source : null,
    files: lib.files && typeof lib.files === "object" ? lib.files : null,
    items,
  };
}

/** Visible text of an item, one entry per text element (unwrapped original text). */
export function itemText(elements) {
  const out = [];
  for (const e of elements) {
    if (e.type !== "text") continue;
    const t = String(e.originalText ?? e.text ?? "").replace(/\s+/g, " ").trim();
    if (t) out.push(t);
  }
  return out;
}
