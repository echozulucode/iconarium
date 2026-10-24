// Discovery, conversion and incremental bookkeeping. The CLI is a thin wrapper around run().

import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { baseNameOf, itemText, kindOf, parseSource } from "./source.mjs";
import { indexWidth, itemFileName, safeName } from "./naming.mjs";
import { exportSvg, loadExporter } from "./exporter.mjs";
import { annotate, outlineText } from "./svg.mjs";
import { loadMeta, lookupMeta } from "./meta.mjs";
import { Manifest, sourceKey } from "./manifest.mjs";

const PKG = JSON.parse(fs.readFileSync(fileURLToPath(new URL("../package.json", import.meta.url)), "utf8"));
export const TOOL_VERSION = PKG.version;
// Bump when output for the same input and options changes, so incremental runs redo it.
const OUTPUT_REVISION = 1;

export const DEFAULTS = Object.freeze({
  text: "outline",
  theme: "light",
  padding: 10,
  background: null,
  frames: true,
  scenes: false,
  force: false,
  dryRun: false,
  meta: null,
});

const toPosix = (p) => p.split(path.sep).join("/");
const SKIP_DIRS = new Set(["node_modules", ".git"]);

/**
 * Find Excalidraw files. Folders are walked recursively (dot-folders and node_modules are
 * skipped); scenes found in folders are included only with `scenes`. Files named
 * explicitly are always included.
 */
export function discover(inputs, { scenes = false } = {}) {
  const found = [];
  const roots = [];
  for (const input of inputs) {
    const abs = path.resolve(input);
    let st;
    try {
      st = fs.statSync(abs);
    } catch {
      throw new Error(`input not found: ${input}`);
    }
    if (st.isFile()) {
      const kind = kindOf(abs);
      if (!kind) throw new Error(`not an .excalidrawlib or .excalidraw file: ${input}`);
      roots.push({ abs, isDir: false });
      found.push({ abs, root: path.dirname(abs), relDir: "", kind });
      continue;
    }
    roots.push({ abs, isDir: true });
    const walk = (dir, rel) => {
      for (const ent of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
        if (ent.name.startsWith(".")) continue;
        const p = path.join(dir, ent.name);
        if (ent.isDirectory()) {
          if (!SKIP_DIRS.has(ent.name)) walk(p, rel ? `${rel}/${ent.name}` : ent.name);
        } else if (ent.isFile()) {
          const kind = kindOf(ent.name);
          if (kind === "library" || (kind === "scene" && scenes)) found.push({ abs: p, root: abs, relDir: rel, kind });
        }
      }
    };
    walk(abs, "");
  }
  // De-duplicate (overlapping inputs), keeping the first occurrence.
  const seen = new Set();
  return {
    roots,
    sources: found.filter((s) => (seen.has(sourceKey(s.abs)) ? false : seen.add(sourceKey(s.abs)))),
  };
}

function itemLabels(src, it, base, meta) {
  const texts = itemText(it.elements);
  const m = lookupMeta(meta, src.relDir, base, it.index);
  const firstText = texts[0] ? texts[0].slice(0, 48) : null;
  const label = it.name || m?.name || m?.summary || firstText;
  const title = label || `${base} #${it.index}`;
  const relSource = toPosix(path.relative(src.root, src.abs)) || path.basename(src.abs);
  const desc = [
    src.kind === "scene" ? `Excalidraw scene ${relSource}` : `Excalidraw library ${relSource}, item #${it.index}`,
    m?.summary && m.summary !== title ? m.summary : null,
    m?.keywords?.length ? `Keywords: ${m.keywords.join(", ")}` : null,
    texts.length ? `Text: ${texts.join(" | ")}`.slice(0, 600) : null,
  ]
    .filter(Boolean)
    .join("\n");
  return { label, title, desc, metaEntry: m };
}

function writeAtomic(file, data) {
  fs.mkdirSync(path.dirname(file), { recursive: true });
  const tmp = `${file}.${process.pid}.tmp`;
  fs.writeFileSync(tmp, data);
  fs.renameSync(tmp, file);
}

/**
 * Convert inputs into `out`. `log(event)` receives progress events:
 *   { type: "source", rel, status: "converted"|"unchanged"|"would-convert"|"failed", items, written, removed, ms, error? }
 *   { type: "item-error", rel, index, error }
 *   { type: "pruned", source, removed }
 * Returns a summary object.
 */
export async function run(inputs, out, options = {}, log = () => {}) {
  const opts = { ...DEFAULTS, ...options };
  const meta = loadMeta(opts.meta);
  const { roots, sources } = discover(inputs, opts);
  const manifest = new Manifest(out);
  const outDir = manifest.outDir;
  const exportOpts = { theme: opts.theme, padding: opts.padding, background: opts.background, frames: opts.frames, embedFonts: opts.text === "live" };
  const excalidrawVersion = opts.dryRun ? null : (await loadExporter()).version;

  const summary = { sources: sources.length, converted: 0, unchanged: 0, failed: 0, items: 0, emptySkipped: 0, written: 0, removed: 0, itemErrors: 0, pruned: 0 };
  const claimed = new Map(); // output rel path -> source, to catch collisions within a run

  for (const src of sources) {
    const started = performance.now();
    const rel = toPosix(path.relative(src.root, src.abs)) || path.basename(src.abs);
    const base = baseNameOf(src.abs);
    try {
      const bytes = fs.readFileSync(src.abs);
      const parsed = parseSource(bytes.toString("utf8"), src.abs);

      // Plan output names.
      const width = indexWidth(parsed.items.length);
      const plan = parsed.items.map((it) => {
        const labels = itemLabels(src, it, base, meta);
        const relOut =
          src.kind === "scene"
            ? toPosix(path.join(src.relDir, `${safeName(base)}.svg`))
            : toPosix(path.join(src.relDir, safeName(base), itemFileName(it.index, width, labels.label)));
        return { it, labels, relOut };
      });
      for (const p of plan) {
        const other = claimed.get(p.relOut.toLowerCase());
        if (other && other !== src.abs) throw new Error(`output ${p.relOut} also produced by ${other}`);
        claimed.set(p.relOut.toLowerCase(), src.abs);
      }

      const fingerprint = crypto
        .createHash("sha256")
        .update(JSON.stringify({ t: TOOL_VERSION, r: OUTPUT_REVISION, x: excalidrawVersion, o: exportOpts, m: plan.map((p) => p.labels.metaEntry) }))
        .update(bytes)
        .digest("hex");

      if (!opts.force && !opts.dryRun && manifest.isFresh(src.abs, fingerprint)) {
        summary.unchanged++;
        log({ type: "source", rel, status: "unchanged", items: plan.length, written: 0, removed: 0, ms: performance.now() - started });
        continue;
      }

      const previous = manifest.get(src.abs)?.outputs ?? [];
      if (opts.dryRun) {
        const next = new Set(plan.filter((p) => !p.it.empty).map((p) => p.relOut));
        log({ type: "source", rel, status: "would-convert", items: next.size, written: next.size, removed: previous.filter((o) => !next.has(o)).length, ms: 0 });
        summary.converted++;
        continue;
      }

      const written = [];
      for (const { it, labels, relOut } of plan) {
        if (it.empty) {
          summary.emptySkipped++;
          continue;
        }
        try {
          let svg = await exportSvg(it.elements, parsed.files, exportOpts);
          if (opts.text === "outline") svg = outlineText(svg);
          svg = annotate(svg, { title: labels.title, desc: labels.desc });
          writeAtomic(manifest.abs(relOut), svg);
          written.push(relOut);
          summary.items++;
        } catch (e) {
          summary.itemErrors++;
          log({ type: "item-error", rel, index: it.index, error: e });
        }
      }
      const removed = manifest.removeOutputs(previous, new Set(written));
      summary.written += written.length;
      summary.removed += removed;
      // An item that failed leaves the fingerprint unset, so the next run retries the source.
      const failedItems = plan.filter((p) => !p.it.empty).length - written.length;
      manifest.set(src.abs, failedItems ? `incomplete:${fingerprint}` : fingerprint, written);
      manifest.save();
      summary.converted++;
      log({ type: "source", rel, status: "converted", items: written.length, written: written.length, removed, ms: performance.now() - started });
    } catch (e) {
      summary.failed++;
      log({ type: "source", rel, status: "failed", items: 0, written: 0, removed: 0, ms: performance.now() - started, error: e });
    }
  }

  // Sources that were converted before but no longer exist under the inputs: remove their output.
  const under = (p) => {
    const k = sourceKey(p);
    return roots.some((r) => (r.isDir ? k.startsWith(sourceKey(r.abs) + path.sep) : k === sourceKey(r.abs)));
  };
  for (const entry of Object.values(manifest.entries)) {
    if (!under(entry.source) || fs.existsSync(entry.source)) continue;
    const removed = opts.dryRun ? entry.outputs.length : manifest.removeOutputs(entry.outputs);
    if (!opts.dryRun) manifest.delete(entry.source);
    summary.pruned++;
    summary.removed += removed;
    log({ type: "pruned", source: entry.source, removed });
  }
  if (!opts.dryRun && (summary.converted || summary.pruned)) manifest.save();
  return { ...summary, outDir, excalidrawVersion };
}
