// Optional sidecar metadata: names, summaries and keywords for items (useful for the many
// library items that have no name). A JSON object keyed by
//
//   "<folder>__<subfolder>__<library>__<index>"   (path relative to the scanned folder)
//   "<library>__<index>"                          (fallback)
//
// with values like { "name"?, "summary"?, "kw"? | "keywords"? | "tags"? }.

import fs from "node:fs";

export function loadMeta(file) {
  if (!file) return null;
  const json = JSON.parse(fs.readFileSync(file, "utf8"));
  if (!json || typeof json !== "object" || Array.isArray(json)) {
    throw new Error(`${file}: expected a JSON object keyed by "<library>__<index>"`);
  }
  return json;
}

const asList = (v) =>
  Array.isArray(v) ? v.map(String) : typeof v === "string" ? v.split(/[,;]/) : [];

function normalize(v) {
  if (!v || typeof v !== "object") return null;
  const str = (x) => (typeof x === "string" && x.trim() ? x.trim() : null);
  const keywords = [...new Set(asList(v.kw ?? v.keywords ?? v.tags).map((s) => s.trim()).filter(Boolean))];
  const out = { name: str(v.name), summary: str(v.summary ?? v.description), keywords };
  return out.name || out.summary || keywords.length ? out : null;
}

/** Metadata for one item, or null. relDir is '/'-separated ("" at the root). */
export function lookupMeta(meta, relDir, libName, index) {
  if (!meta) return null;
  const parts = relDir ? relDir.split("/").filter(Boolean) : [];
  for (const key of [[...parts, libName, index].join("__"), `${libName}__${index}`]) {
    if (key in meta) return normalize(meta[key]);
  }
  return null;
}
