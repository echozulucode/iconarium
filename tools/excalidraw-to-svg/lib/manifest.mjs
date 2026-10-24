// Incremental runs: a manifest in the output folder records, per source file, a fingerprint
// and the files written for it. Unchanged sources are skipped; when a source changes or
// disappears, only files this tool wrote for it are removed.

import fs from "node:fs";
import path from "node:path";

export const MANIFEST_NAME = ".excalidraw-to-svg.json";

export function sourceKey(absPath) {
  const p = path.resolve(absPath);
  return process.platform === "win32" ? p.toLowerCase() : p;
}

export class Manifest {
  constructor(outDir) {
    this.outDir = path.resolve(outDir);
    this.file = path.join(this.outDir, MANIFEST_NAME);
    this.entries = {};
    try {
      const json = JSON.parse(fs.readFileSync(this.file, "utf8"));
      if (json && json.formatVersion === 1 && json.entries) this.entries = json.entries;
    } catch {
      /* first run or unreadable: start empty */
    }
  }

  get(absPath) {
    return this.entries[sourceKey(absPath)] ?? null;
  }

  /** True if the entry matches `fingerprint` and all its outputs still exist. */
  isFresh(absPath, fingerprint) {
    const e = this.get(absPath);
    return !!e && e.fingerprint === fingerprint && e.outputs.every((o) => fs.existsSync(this.abs(o)));
  }

  set(absPath, fingerprint, outputs) {
    this.entries[sourceKey(absPath)] = { source: path.resolve(absPath), fingerprint, outputs: [...outputs].sort() };
  }

  delete(absPath) {
    delete this.entries[sourceKey(absPath)];
  }

  abs(rel) {
    const p = path.resolve(this.outDir, rel);
    if (p !== this.outDir && !p.startsWith(this.outDir + path.sep)) throw new Error(`refusing path outside output: ${rel}`);
    return p;
  }

  /** Remove previously written outputs not in `keep`; prune folders left empty. */
  removeOutputs(previous, keep = new Set()) {
    let removed = 0;
    const dirs = new Set();
    for (const rel of previous) {
      if (keep.has(rel)) continue;
      const p = this.abs(rel);
      try {
        fs.unlinkSync(p);
        removed++;
      } catch (e) {
        if (e.code !== "ENOENT") throw e;
      }
      dirs.add(path.dirname(p));
    }
    for (let d of [...dirs].sort((a, b) => b.length - a.length)) {
      while (d.startsWith(this.outDir + path.sep)) {
        try {
          fs.rmdirSync(d);
        } catch {
          break; // not empty (or gone)
        }
        d = path.dirname(d);
      }
    }
    return removed;
  }

  save() {
    fs.mkdirSync(this.outDir, { recursive: true });
    const tmp = `${this.file}.tmp`;
    const sorted = Object.fromEntries(Object.entries(this.entries).sort(([a], [b]) => a.localeCompare(b)));
    fs.writeFileSync(tmp, JSON.stringify({ formatVersion: 1, tool: "excalidraw-to-svg", entries: sorted }, null, 1) + "\n");
    fs.renameSync(tmp, this.file);
  }
}
