// File and folder names that are safe on Windows, macOS and Linux.

const RESERVED = /^(con|prn|aux|nul|com[0-9]|lpt[0-9])$/i;

/** Lowercase kebab slug of letters and digits (any script), at most `max` characters. */
export function slugify(text, max = 60) {
  if (!text) return "";
  let s = String(text)
    .normalize("NFKD")
    .replace(/\p{M}+/gu, "")
    .toLowerCase()
    .replace(/[^\p{L}\p{N}]+/gu, "-")
    .replace(/^-+|-+$/g, "");
  if (s.length > max) {
    s = s.slice(0, max);
    const cut = s.lastIndexOf("-");
    if (cut > max / 2) s = s.slice(0, cut);
    s = s.replace(/-+$/g, "");
  }
  return RESERVED.test(s) ? `${s}-` : s;
}

/** A folder name kept close to the original (case and punctuation preserved). */
export function safeName(name) {
  let s = String(name)
    .replace(/[<>:"/\\|?*\u0000-\u001f]/g, "_")
    .replace(/[. ]+$/g, "")
    .trim();
  if (!s) s = "_";
  if (RESERVED.test(s.split(".")[0])) s = `_${s}`;
  return s;
}

/** Zero-pad width for `count` items (at least 3 digits: 000, 001, …). */
export function indexWidth(count) {
  return Math.max(3, String(Math.max(0, count - 1)).length);
}

/** "007-cloud-run.svg", or "007.svg" when there is no label. */
export function itemFileName(index, width, label) {
  const slug = slugify(label);
  return `${String(index).padStart(width, "0")}${slug ? `-${slug}` : ""}.svg`;
}
