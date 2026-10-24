#!/usr/bin/env node
// Build iconarium-cli in release mode and stage it where Tauri's `bundle.externalBin`
// expects it: src-tauri/binaries/iconarium-cli-<target-triple>[.exe]. The installer then
// ships it next to Iconarium.exe as iconarium-cli.exe.
//
// Run by `beforeBuildCommand` in src-tauri/tauri.bundle.conf.json, which installer builds
// (`just build`, `just build-unsigned`, the release workflow) layer on with --config. Plain
// `tauri dev` / `cargo test` don't use that config, so they never need the staged binary.
import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const host = execFileSync("rustc", ["-vV"], { encoding: "utf8" }).match(/^host: (.+)$/m)?.[1]?.trim();
const triple = process.env.TAURI_ENV_TARGET_TRIPLE || host;
if (!triple) {
  console.error("stage-cli: cannot determine the Rust target triple");
  process.exit(1);
}
const crossTarget = triple !== host;
const args = ["build", "--release", "--locked", "-p", "iconarium-cli"];
if (crossTarget) args.push("--target", triple);
console.log(`stage-cli: cargo ${args.join(" ")}`);
execFileSync("cargo", args, { cwd: root, stdio: "inherit" });

const exe = triple.includes("windows") ? ".exe" : "";
const targetDir = process.env.CARGO_TARGET_DIR ? resolve(root, process.env.CARGO_TARGET_DIR) : join(root, "target");
const built = join(targetDir, ...(crossTarget ? [triple] : []), "release", `iconarium-cli${exe}`);
const staged = join(root, "src-tauri", "binaries", `iconarium-cli-${triple}${exe}`);
mkdirSync(dirname(staged), { recursive: true });
copyFileSync(built, staged);
console.log(`stage-cli: ${staged}`);
