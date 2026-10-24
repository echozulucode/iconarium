import { test } from "node:test";
import assert from "node:assert/strict";

import { FAMILY_MAP, prepareFonts } from "../lib/fonts.mjs";
import { outlineText } from "../lib/svg.mjs";

const sample = (family, content = "Hamburgefonstiv 0123") =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="400" height="60"><text x="0" y="40" font-family="${family}, sans-serif, Segoe UI Emoji" font-size="24">${content}</text></svg>`;

// Guards the family-name mapping against changes in the TTFs @excalidraw/utils ships:
// each mapped family must draw differently from the sans-serif fallback.
test("every Excalidraw font family resolves to its bundled TTF", () => {
  const fallback = outlineText(sample("NoSuchFamily"));
  const names = { virgil: "Virgil", excalifont: "Excalifont", cascadia: "Cascadia", nunito: "Nunito", "comic shanns": "Comic Shanns", "lilita one": "Lilita One" };
  for (const [key, svgName] of Object.entries(names)) {
    assert.ok(FAMILY_MAP[key], key);
    const out = outlineText(sample(svgName));
    assert.ok(!out.includes("<text"), `${svgName}: text left in output`);
    assert.notEqual(out, fallback, `${svgName} fell back to the default font; check FAMILY_MAP`);
  }
  // Helvetica intentionally maps to the fallback (Liberation Sans).
  assert.equal(outlineText(sample("Helvetica")), fallback);
});

test("font lists are rewritten and only needed files are loaded", () => {
  const p = prepareFonts(sample("Cascadia"));
  assert.match(p.svg, /font-family="'Cascadia Code', sans-serif, Segoe UI Emoji"/);
  assert.deepEqual(p.fontFiles.map((f) => f.split(/[\\/]/).pop()), ["Cascadia Code.ttf", "Liberation Sans.ttf"]);
  assert.equal(p.needsSystemFonts, false);
});

test("system fonts are loaded only for characters the bundled fonts lack", () => {
  assert.equal(prepareFonts(sample("Virgil", "Plain → arrows &amp; Ünïcode")).needsSystemFonts, false);
  assert.equal(prepareFonts(sample("Virgil", "数据库")).needsSystemFonts, true);
  assert.equal(prepareFonts(sample("Excalifont", "数据库")).needsSystemFonts, false);
  assert.equal(prepareFonts(sample("Virgil", "ok &#x1F44D;")).needsSystemFonts, true);
  assert.equal(prepareFonts(sample("Virgil", "שלום")).needsSystemFonts, true);
});

test("SVGs without text pass through unchanged", () => {
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><path d="M0 0L1 1"/></svg>';
  assert.equal(outlineText(svg), svg);
});
