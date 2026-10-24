// Fonts for outlining text. They are the TTFs that ship inside @excalidraw/utils, loaded in
// place (nothing is copied or modified). Excalidraw's SVGs name some families differently
// from the TTFs' own family names, so font-family lists are rewritten before outlining.

import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";

// Excalidraw font-family name (lowercase) -> TTF file and the family name the TTF declares.
// Verified by test/fonts.test.mjs: each mapping must change the outlined result vs. fallback.
export const FAMILY_MAP = {
  virgil: { file: "Virgil.ttf", family: "Virgil" },
  excalifont: { file: "Excalifont.ttf", family: "Excalifont" },
  // Excalifont.ttf also carries the Xiaolai CJK glyphs Excalidraw lists as a fallback.
  xiaolai: { file: "Excalifont.ttf", family: "Excalifont" },
  cascadia: { file: "Cascadia Code.ttf", family: "Cascadia Code" },
  nunito: { file: "Nunito ExtraLight Medium.ttf", family: "Nunito ExtraLight" },
  "comic shanns": { file: "Comic Shanns Regular.ttf", family: "Comic Shanns Regular" },
  "lilita one": { file: "Lilita One.ttf", family: "Lilita One" },
  // Excalidraw's "Helvetica" option; Liberation Sans is metric-compatible and always loaded.
  helvetica: { file: "Liberation Sans.ttf", family: "Liberation Sans" },
};
export const FALLBACK = { file: "Liberation Sans.ttf", family: "Liberation Sans" };
const MONOSPACE = FAMILY_MAP.cascadia;

let dir;
/** Folder holding Excalidraw's TTFs. */
export function fontsDir() {
  if (!dir) {
    const entry = createRequire(import.meta.url).resolve("@excalidraw/utils");
    dir = path.join(path.dirname(entry), "assets");
    if (!fs.existsSync(path.join(dir, FALLBACK.file))) {
      throw new Error(`Excalidraw fonts not found in ${dir} (has @excalidraw/utils changed layout?)`);
    }
  }
  return dir;
}

const unquote = (s) => s.trim().replace(/^['"]|['"]$/g, "");

/** Rewrite one font-family list; records the TTFs it needs in `files`. */
export function mapFamilyList(list, files) {
  return list
    .split(",")
    .map((raw) => {
      const name = unquote(raw);
      const hit = FAMILY_MAP[name.toLowerCase()];
      if (hit) {
        files.add(hit.file);
        return hit.family.includes(" ") ? `'${hit.family}'` : hit.family;
      }
      if (name.toLowerCase() === "monospace") files.add(MONOSPACE.file);
      return raw.trim();
    })
    .join(", ");
}

// Code points the Excalidraw TTFs cover (each has Liberation's Latin/Greek/Cyrillic
// glyphs merged in, plus common punctuation, arrows and symbols).
function coveredByBundled(cp, hasExcalifont) {
  if (cp < 0x0530) return true;
  if (cp >= 0x2000 && cp <= 0x22ff) return true;
  if (hasExcalifont) {
    // CJK in Excalifont (Xiaolai): radicals, kana, ideographs, Hangul, full-width forms.
    if ((cp >= 0x2e80 && cp <= 0x9fff) || (cp >= 0xac00 && cp <= 0xd7af) ||
        (cp >= 0xf900 && cp <= 0xfaff) || (cp >= 0xff00 && cp <= 0xffef)) return true;
  }
  return false;
}

const decode = (s) =>
  s.replace(/&(#x[0-9a-f]+|#\d+|amp|lt|gt|quot|apos);/gi, (m, e) => {
    const k = e.toLowerCase();
    if (k[0] === "#") return String.fromCodePoint(k[1] === "x" ? parseInt(k.slice(2), 16) : parseInt(k.slice(1), 10));
    return { amp: "&", lt: "<", gt: ">", quot: '"', apos: "'" }[k];
  });

/**
 * Prepare an Excalidraw SVG for resvg: rewrite font families and work out which font files
 * to load. System fonts are loaded only for characters the bundled fonts lack (emoji,
 * scripts beyond Latin/Greek/Cyrillic, CJK outside Excalifont), because scanning them
 * costs ~50–200 ms per SVG.
 */
export function prepareFonts(svg) {
  const files = new Set([FALLBACK.file]);
  const rewritten = svg.replace(/font-family="([^"]*)"/g, (_, list) => `font-family="${mapFamilyList(list, files)}"`);
  const hasExcalifont = files.has(FAMILY_MAP.excalifont.file);
  let needsSystemFonts = false;
  for (const m of rewritten.matchAll(/<text\b[^>]*>([\s\S]*?)<\/text>/g)) {
    for (const ch of decode(m[1].replace(/<[^>]+>/g, ""))) {
      if (!coveredByBundled(ch.codePointAt(0), hasExcalifont)) {
        needsSystemFonts = true;
        break;
      }
    }
    if (needsSystemFonts) break;
  }
  const d = fontsDir();
  return {
    svg: rewritten,
    fontFiles: [...files].sort().map((f) => path.join(d, f)),
    needsSystemFonts,
  };
}

export const RESVG_FAMILIES = {
  defaultFontFamily: FALLBACK.family,
  sansSerifFamily: FALLBACK.family,
  serifFamily: FALLBACK.family,
  monospaceFamily: MONOSPACE.family,
};
