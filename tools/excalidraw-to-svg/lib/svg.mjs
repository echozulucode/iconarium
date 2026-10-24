// SVG post-processing: outline text with resvg, and add <title>/<desc> for search tools.

import { Resvg } from "@resvg/resvg-js";
import { prepareFonts, RESVG_FAMILIES } from "./fonts.mjs";

/**
 * Convert every <text> to paths. SVGs without text are returned unchanged, so purely
 * geometric items keep Excalidraw's original markup.
 */
export function outlineText(svg) {
  if (!/<text\b/.test(svg)) return svg;
  const { svg: prepared, fontFiles, needsSystemFonts } = prepareFonts(svg);
  const resvg = new Resvg(prepared, {
    font: { fontFiles, loadSystemFonts: needsSystemFonts, ...RESVG_FAMILIES },
    logLevel: "off",
  });
  return resvg.toString();
}

export function escapeXml(s) {
  return String(s)
    .replace(/&/g, "&amp;")
    .replace(/</g, "&lt;")
    .replace(/>/g, "&gt;")
    .replace(/"/g, "&quot;")
    // Drop characters XML 1.0 forbids.
    .replace(/[\u0000-\u0008\u000b\u000c\u000e-\u001f￾￿]/g, "");
}

/** Index just past the root <svg …> start tag (quote-aware), or -1. */
function rootTagEnd(svg) {
  const start = svg.search(/<svg[\s>]/);
  if (start < 0) return -1;
  let quote = null;
  for (let i = start + 4; i < svg.length; i++) {
    const c = svg[i];
    if (quote) {
      if (c === quote) quote = null;
    } else if (c === '"' || c === "'") quote = c;
    else if (c === ">") return svg[i - 1] === "/" ? -1 : i + 1;
  }
  return -1;
}

/** Insert <title> and <desc> as the root's first children (replacing existing ones). */
export function annotate(svg, { title, desc }) {
  let out = svg.replace(/<title>[\s\S]*?<\/title>|<desc>[\s\S]*?<\/desc>/g, "");
  const at = rootTagEnd(out);
  if (at < 0) return out;
  const parts = [];
  if (title) parts.push(`<title>${escapeXml(title)}</title>`);
  if (desc) parts.push(`<desc>${escapeXml(desc)}</desc>`);
  return out.slice(0, at) + parts.join("") + out.slice(at);
}
