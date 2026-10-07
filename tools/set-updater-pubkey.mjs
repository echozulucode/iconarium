#!/usr/bin/env node
/**
 * set-updater-pubkey.mjs — write an updater public key into tauri.conf.json.
 *
 *   node tools/set-updater-pubkey.mjs <path-to-.key.pub>     # set it
 *   node tools/set-updater-pubkey.mjs --check                # fail if still the placeholder
 *
 * Only the PUBLIC key (the `.pub` file) is ever read here. The private key never belongs in the
 * repo, a script argument that gets logged, or a chat transcript.
 */
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { dirname, join, resolve } from "node:path";

const conf = join(resolve(dirname(fileURLToPath(import.meta.url)), ".."), "src-tauri", "tauri.conf.json");
const PLACEHOLDER = "REPLACE_WITH_ICONARIUM_UPDATER_PUBKEY";
const pattern = /("pubkey"\s*:\s*)"([^"]*)"/;

const raw = await readFile(conf, "utf8");
const current = pattern.exec(raw)?.[2];
if (current === undefined) {
  console.error("No plugins.updater.pubkey in src-tauri/tauri.conf.json");
  process.exit(1);
}

const arg = process.argv[2];
if (!arg || arg === "--check") {
  if (current === PLACEHOLDER || current.length < 40) {
    console.error("The updater public key is not set (tauri.conf.json still has the placeholder).");
    console.error("Run `just updater-key` once (see README > Releasing).");
    process.exit(1);
  }
  console.log("Updater public key is set.");
  process.exit(0);
}

const pub = (await readFile(arg, "utf8")).trim();
if (/PRIVATE|secret key/i.test(Buffer.from(pub, "base64").toString("utf8"))) {
  console.error("That looks like a PRIVATE key. Pass the .pub file.");
  process.exit(1);
}
await writeFile(conf, raw.replace(pattern, `$1"${pub}"`));
console.log("Wrote the updater public key into src-tauri/tauri.conf.json. Commit that change.");
