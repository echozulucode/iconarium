// In-browser search engine used by the mock backend. Mirrors the planned backend semantics
// (implementation-plan §2/§3): whitespace tokens are ANDed, "quoted phrases", -exclusions,
// globs with * ? (path-matched when the pattern contains '/'), re:regex, and ranking
// filename > path > title > text > desc > ids/classes with a match explanation.
import { AppError } from "../errors";
import type { MatchField, MatchInfo } from "../types";

export interface SearchDoc {
  id: number;
  filename: string;
  relPath: string;
  relDir: string;
  title: string;
  texts: string[];
  desc: string;
  idClass: string;
}

type Term =
  | { kind: "word"; value: string; negate: boolean }
  | { kind: "glob"; re: RegExp; path: boolean; negate: boolean }
  | { kind: "regex"; re: RegExp; negate: boolean };

export interface ParsedQuery {
  terms: Term[];
  empty: boolean;
}

/** Split into tokens, honouring "double quotes". Returns raw tokens with quotes stripped and a phrase flag. */
function tokenize(q: string): { text: string; quoted: boolean; negate: boolean }[] {
  const out: { text: string; quoted: boolean; negate: boolean }[] = [];
  let i = 0;
  while (i < q.length) {
    while (i < q.length && /\s/.test(q[i])) i++;
    if (i >= q.length) break;
    let negate = false;
    if (q[i] === "-" && i + 1 < q.length && !/\s/.test(q[i + 1])) {
      negate = true;
      i++;
    }
    let prefix = "";
    // allow re:"..." form
    if (q.startsWith("re:", i) && q[i + 3] === '"') {
      prefix = "re:";
      i += 3;
    }
    if (q[i] === '"') {
      const end = q.indexOf('"', i + 1);
      const text = end < 0 ? q.slice(i + 1) : q.slice(i + 1, end);
      i = end < 0 ? q.length : end + 1;
      out.push({ text: prefix + text, quoted: !prefix, negate });
    } else {
      let j = i;
      while (j < q.length && !/\s/.test(q[j])) j++;
      out.push({ text: prefix + q.slice(i, j), quoted: false, negate });
      i = j;
    }
  }
  return out;
}

function globToRegExp(glob: string): RegExp {
  let src = "";
  for (let i = 0; i < glob.length; i++) {
    const c = glob[i];
    if (c === "*") {
      if (glob[i + 1] === "*") {
        src += ".*";
        i++;
        if (glob[i + 1] === "/") i++;
      } else src += "[^/]*";
    } else if (c === "?") src += "[^/]";
    else src += c.replace(/[.+^${}()|[\]\\]/g, "\\$&");
  }
  return new RegExp(`^${src}$`, "i");
}

export function parseQuery(query: string): ParsedQuery {
  const q = query.trim();
  if (!q) return { terms: [], empty: true };
  // Whole-query regex: everything after a leading re: (allows spaces).
  if (/^re:/i.test(q) && !q.startsWith('re:"')) {
    return { terms: [{ kind: "regex", re: compileRegex(q.slice(3)), negate: false }], empty: false };
  }
  const terms: Term[] = [];
  for (const tok of tokenize(q)) {
    if (!tok.text) continue;
    if (tok.text.startsWith("re:")) {
      terms.push({ kind: "regex", re: compileRegex(tok.text.slice(3)), negate: tok.negate });
    } else if (!tok.quoted && /[*?]/.test(tok.text)) {
      const text = tok.text.replace(/\\/g, "/");
      terms.push({ kind: "glob", re: globToRegExp(text), path: text.includes("/"), negate: tok.negate });
    } else {
      terms.push({ kind: "word", value: tok.text.toLowerCase().replace(/\s+/g, " "), negate: tok.negate });
    }
  }
  return { terms, empty: terms.length === 0 };
}

function compileRegex(src: string): RegExp {
  if (!src) throw new AppError("invalid_query", "Regular expression is empty");
  try {
    return new RegExp(src, "i");
  } catch (e) {
    const msg = e instanceof Error ? e.message.replace(/^Invalid regular expression: /, "") : String(e);
    throw new AppError("invalid_query", `Invalid regular expression: ${msg}`);
  }
}

// Field weights (plan §17).
const W_EXACT = 1000;
const W_FILE_TOKEN = 700;
const W_FILE = 500;
const W_PATH = 300;
const W_TITLE = 200;
const W_TEXT = 120;
const W_DESC = 80;
const W_ID = 40;

const FIELD_RANK: Record<MatchField, number> = { filename: 0, path: 1, title: 2, text: 3, desc: 4, id_class: 5 };

interface Lowered {
  stem: string;
  filename: string;
  relDir: string;
  title: string;
  texts: string[];
  desc: string;
  idClass: string;
}

const lowerCache = new WeakMap<SearchDoc, Lowered>();
function lowered(d: SearchDoc): Lowered {
  let l = lowerCache.get(d);
  if (!l) {
    const filename = d.filename.toLowerCase();
    l = {
      filename,
      stem: filename.replace(/\.svg$/, ""),
      relDir: d.relDir.toLowerCase(),
      title: d.title.toLowerCase(),
      texts: d.texts.map((t) => t.toLowerCase().replace(/\s+/g, " ")),
      desc: d.desc.toLowerCase(),
      idClass: d.idClass.toLowerCase(),
    };
    lowerCache.set(d, l);
  }
  return l;
}

interface TermHit {
  field: MatchField;
  weight: number;
  /** For content snippets: original-case text and offset of the match. */
  source?: string;
  at?: number;
  len?: number;
}

function isTokenStart(s: string, idx: number): boolean {
  return idx === 0 || /[-_.\s/]/.test(s[idx - 1]);
}

function hitWord(d: SearchDoc, l: Lowered, v: string): TermHit | null {
  if (l.stem === v) return { field: "filename", weight: W_EXACT };
  const fi = l.filename.indexOf(v);
  if (fi >= 0) return { field: "filename", weight: isTokenStart(l.filename, fi) ? W_FILE_TOKEN : W_FILE };
  if (l.relDir.includes(v)) return { field: "path", weight: W_PATH };
  const ti = l.title.indexOf(v);
  if (ti >= 0) return { field: "title", weight: W_TITLE, source: d.title, at: ti, len: v.length };
  for (let k = 0; k < l.texts.length; k++) {
    const xi = l.texts[k].indexOf(v);
    if (xi >= 0) return { field: "text", weight: W_TEXT, source: d.texts[k], at: xi, len: v.length };
  }
  const di = l.desc.indexOf(v);
  if (di >= 0) return { field: "desc", weight: W_DESC, source: d.desc, at: di, len: v.length };
  const ii = l.idClass.indexOf(v);
  if (ii >= 0) return { field: "id_class", weight: W_ID, source: d.idClass, at: ii, len: v.length };
  return null;
}

function hitRegex(d: SearchDoc, re: RegExp): TermHit | null {
  const tryField = (s: string, field: MatchField, weight: number): TermHit | null => {
    const m = re.exec(s);
    return m ? { field, weight, source: s, at: m.index, len: m[0].length } : null;
  };
  return (
    tryField(d.filename, "filename", W_FILE) ??
    tryField(d.relPath, "path", W_PATH) ??
    tryField(d.title, "title", W_TITLE) ??
    d.texts.reduce<TermHit | null>((acc, t) => acc ?? tryField(t, "text", W_TEXT), null) ??
    tryField(d.desc, "desc", W_DESC) ??
    tryField(d.idClass, "id_class", W_ID)
  );
}

function hitTerm(d: SearchDoc, l: Lowered, t: Term): TermHit | null {
  switch (t.kind) {
    case "word":
      return hitWord(d, l, t.value);
    case "glob":
      return t.path
        ? t.re.test(d.relPath)
          ? { field: "path", weight: W_PATH }
          : null
        : t.re.test(d.filename)
          ? { field: "filename", weight: W_FILE }
          : null;
    case "regex":
      return hitRegex(d, t.re);
  }
}

export interface DocMatch {
  score: number;
  info: MatchInfo;
}

export function matchDoc(d: SearchDoc, pq: ParsedQuery): DocMatch | null {
  if (pq.empty) return { score: 0, info: { field: "filename", snippet: d.filename } };
  const l = lowered(d);
  let score = 0;
  let weakest: TermHit | null = null;
  let positives = 0;
  for (const t of pq.terms) {
    const h = hitTerm(d, l, t);
    if (t.negate) {
      if (h) return null;
      continue;
    }
    if (!h) return null;
    positives++;
    score += h.weight;
    if (!weakest || FIELD_RANK[h.field] > FIELD_RANK[weakest.field]) weakest = h;
  }
  if (positives === 0) return { score: 0, info: { field: "filename", snippet: d.filename } };
  return { score, info: explain(d, weakest!) };
}

function explain(d: SearchDoc, h: TermHit): MatchInfo {
  if (h.field === "filename") return { field: "filename", snippet: d.filename };
  if (h.field === "path") return { field: "path", snippet: d.relPath };
  return { field: h.field, snippet: snippet(h.source ?? "", h.at ?? 0, h.len ?? 0) };
}

export function snippet(src: string, at: number, len: number, max = 72): string {
  const s = src.replace(/\s+/g, " ").trim();
  if (s.length <= max) return s;
  const pad = Math.max(8, Math.floor((max - len) / 2));
  let start = Math.max(0, at - pad);
  const end = Math.min(s.length, start + max);
  start = Math.max(0, end - max);
  return (start > 0 ? "…" : "") + s.slice(start, end).trim() + (end < s.length ? "…" : "");
}

/** Ranked IDs. Empty query → path order. Throws AppError('invalid_query') on bad regex. */
export function searchDocs(docs: readonly SearchDoc[], query: string): number[] {
  const pq = parseQuery(query);
  if (pq.empty) {
    return [...docs].sort((a, b) => cmpPath(a, b)).map((d) => d.id);
  }
  const scored: { d: SearchDoc; s: number }[] = [];
  for (const d of docs) {
    const m = matchDoc(d, pq);
    if (m) scored.push({ d, s: m.score });
  }
  scored.sort((a, b) => b.s - a.s || a.d.filename.length - b.d.filename.length || cmpPath(a.d, b.d));
  return scored.map((x) => x.d.id);
}

function cmpPath(a: SearchDoc, b: SearchDoc): number {
  return a.relPath < b.relPath ? -1 : a.relPath > b.relPath ? 1 : 0;
}
