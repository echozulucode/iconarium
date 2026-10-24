#!/usr/bin/env node
// excalidraw-to-svg: convert Excalidraw libraries and scenes into folders of SVG files.
// Run with --help for usage.

import path from "node:path";
import { parseArgs } from "node:util";
import { DEFAULTS, TOOL_VERSION, run } from "./lib/convert.mjs";

const HELP = `excalidraw-to-svg ${TOOL_VERSION}
Convert Excalidraw libraries (.excalidrawlib) and scenes (.excalidraw) to SVG files.

Usage:
  excalidraw-to-svg <file-or-folder>... --out <folder> [options]

Each library becomes a folder of SVGs named <index>-<item-name>.svg (index is 0-based,
matching Excalidraw catalogues). Folders are scanned recursively and their layout is
mirrored in the output. Re-running only reconverts libraries that changed.

Options:
  -o, --out <folder>     Output folder (required)
      --text <mode>      outline  text becomes paths: renders identically everywhere,
                                  including Office and Iconarium (default)
                         live     text stays editable, with Excalidraw's fonts embedded
                                  (exact in browsers; Office/Inkscape substitute fonts)
      --theme <name>     light (default) or dark
      --padding <px>     Space around each item (default ${DEFAULTS.padding})
      --background <c>   CSS color behind each item (default: transparent)
      --hide-frames      Don't draw frame outlines and labels
      --scenes           Also convert .excalidraw scenes found in folders
                         (scene files named on the command line are always converted)
      --meta <file>      JSON with names/summaries/keywords for items, keyed
                         "<folder>__<library>__<index>" or "<library>__<index>"
      --force            Reconvert everything, even if unchanged
      --dry-run          Report what would change without writing anything
  -q, --quiet            Only print errors and the summary
  -h, --help             Show this help
  -v, --version          Show versions
`;

function fail(msg) {
  process.stderr.write(`excalidraw-to-svg: ${msg}\nRun with --help for usage.\n`);
  process.exit(2);
}

let args;
try {
  args = parseArgs({
    allowPositionals: true,
    options: {
      out: { type: "string", short: "o" },
      text: { type: "string", default: DEFAULTS.text },
      theme: { type: "string", default: DEFAULTS.theme },
      padding: { type: "string", default: String(DEFAULTS.padding) },
      background: { type: "string" },
      "hide-frames": { type: "boolean", default: false },
      scenes: { type: "boolean", default: false },
      meta: { type: "string" },
      force: { type: "boolean", default: false },
      "dry-run": { type: "boolean", default: false },
      quiet: { type: "boolean", short: "q", default: false },
      help: { type: "boolean", short: "h", default: false },
      version: { type: "boolean", short: "v", default: false },
    },
  });
} catch (e) {
  fail(e.message);
}
const { values: v, positionals } = args;

if (v.help) {
  process.stdout.write(HELP);
  process.exit(0);
}
if (v.version) {
  const { loadExporter } = await import("./lib/exporter.mjs");
  process.stdout.write(`excalidraw-to-svg ${TOOL_VERSION}\n@excalidraw/utils ${(await loadExporter()).version}\nnode ${process.versions.node}\n`);
  process.exit(0);
}
if (!positionals.length) fail("no input files or folders given");
if (!v.out) fail("--out <folder> is required");
if (!["outline", "live"].includes(v.text)) fail(`--text must be "outline" or "live", not "${v.text}"`);
if (!["light", "dark"].includes(v.theme)) fail(`--theme must be "light" or "dark", not "${v.theme}"`);
const padding = Number(v.padding);
if (!Number.isFinite(padding) || padding < 0) fail(`--padding must be a non-negative number, not "${v.padding}"`);

const fmtMs = (ms) => (ms >= 1000 ? `${(ms / 1000).toFixed(1)}s` : `${Math.round(ms)}ms`);
const say = (s) => process.stdout.write(`${s}\n`);

function log(ev) {
  if (ev.type === "item-error") {
    process.stderr.write(`  error: ${ev.rel} item #${ev.index}: ${ev.error?.message ?? ev.error}\n`);
  } else if (ev.type === "pruned") {
    if (!v.quiet) say(`removed   ${ev.source} (source deleted; ${ev.removed} file(s))`);
  } else if (ev.status === "failed") {
    process.stderr.write(`FAILED    ${ev.rel}: ${ev.error?.message ?? ev.error}\n`);
  } else if (!v.quiet) {
    const extra = ev.removed ? `, ${ev.removed} stale removed` : "";
    const label = { converted: "converted", unchanged: "unchanged", "would-convert": "would convert" }[ev.status];
    say(`${label.padEnd(9)} ${ev.rel}  ${ev.items} item(s)${extra}${ev.ms ? `  ${fmtMs(ev.ms)}` : ""}`);
  }
}

const started = performance.now();
let s;
try {
  s = await run(positionals, v.out, {
    text: v.text,
    theme: v.theme,
    padding,
    background: v.background ?? null,
    frames: !v["hide-frames"],
    scenes: v.scenes,
    meta: v.meta ? path.resolve(v.meta) : null,
    force: v.force,
    dryRun: v["dry-run"],
  }, log);
} catch (e) {
  process.stderr.write(`excalidraw-to-svg: ${e.message}\n`);
  process.exit(1);
}

const parts = [
  `${s.sources} source(s)`,
  `${s.converted} ${v["dry-run"] ? "to convert" : "converted"}`,
  `${s.unchanged} unchanged`,
  s.failed ? `${s.failed} failed` : null,
  `${s.items} SVG(s) written`,
  s.removed ? `${s.removed} removed` : null,
  s.emptySkipped ? `${s.emptySkipped} empty item(s) skipped` : null,
  s.itemErrors ? `${s.itemErrors} item error(s)` : null,
].filter(Boolean);
say(`${v["dry-run"] ? "[dry run] " : ""}${parts.join(", ")} in ${fmtMs(performance.now() - started)} -> ${s.outDir}`);
process.exit(s.failed || s.itemErrors ? 1 : 0);
