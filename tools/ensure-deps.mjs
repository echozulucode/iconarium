#!/usr/bin/env node
// Install npm dependencies only when node_modules is missing or package(-lock).json changed since
// the last install. Cross-platform replacement for a shell-specific stamp check.
//
//   node tools/ensure-deps.mjs                            # the app (repo root)
//   node tools/ensure-deps.mjs tools/excalidraw-to-svg    # another package folder
import { existsSync, statSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const repo = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const root = process.argv[2] ? resolve(repo, process.argv[2]) : repo;
const stamp = join(root, "node_modules", ".deps-stamp");
const mtime = (p) => statSync(join(root, p)).mtimeMs;
const stale =
  !existsSync(stamp) || ["package.json", "package-lock.json"].some((f) => mtime(f) > statSync(stamp).mtimeMs);
if (stale) {
  const npm = process.platform === "win32" ? "npm.cmd" : "npm";
  const r = spawnSync(npm, ["install", "--no-audit", "--no-fund"], { cwd: root, stdio: "inherit", shell: process.platform === "win32" });
  if (r.status !== 0) process.exit(r.status ?? 1);
  writeFileSync(stamp, new Date().toISOString());
}
