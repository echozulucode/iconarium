#!/usr/bin/env node
/**
 * One-time setup of the auto-update signing key.
 *
 *   just updater-key            # generate ~/.tauri/iconarium-updater.key (+ .pub), write the pubkey
 *   just updater-key --check    # used by `just build`: is TAURI_SIGNING_PRIVATE_KEY set?
 *
 * The private key stays in ~/.tauri (outside the repo). Back it up with its passphrase: losing it
 * strands every installed copy on its current version.
 */
import { existsSync, mkdirSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { homedir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

if (process.argv[2] === "--check") {
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY) {
    console.error("TAURI_SIGNING_PRIVATE_KEY is not set, so the installer cannot be signed for the auto-updater.");
    console.error("Set it (README > Releasing), or run `just build-unsigned` for a local test build.");
    process.exit(1);
  }
  process.exit(0);
}

const dir = join(homedir(), ".tauri");
const key = join(dir, "iconarium-updater.key");
mkdirSync(dir, { recursive: true });
const shell = process.platform === "win32";
if (existsSync(key)) {
  console.log(`${key} already exists; reusing it (delete it first to generate a new key).`);
} else {
  const r = spawnSync(shell ? "npx.cmd" : "npx", ["tauri", "signer", "generate", "-w", key], { cwd: root, stdio: "inherit", shell });
  if (r.status !== 0) process.exit(r.status ?? 1);
}
const r = spawnSync(process.execPath, [join(root, "tools", "set-updater-pubkey.mjs"), `${key}.pub`], { stdio: "inherit" });
process.exit(r.status ?? 1);
