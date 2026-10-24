import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";

import { run } from "../lib/convert.mjs";
import { MANIFEST_NAME } from "../lib/manifest.mjs";
import { imageWithFile, libraryV1, libraryV2, listFiles, rect, scene, text, tmpDir, writeJson } from "./helpers.mjs";

function fixtureTree() {
  const src = tmpDir("x2s-src-");
  const img = imageWithFile();
  writeJson(path.join(src, "net", "devices.excalidrawlib"), libraryV2([
    { name: "Router", elements: [rect({ backgroundColor: "#a5d8ff", fillStyle: "hachure" })] },
    { elements: [rect(), text("Load Balancer", { y: 70 })] },
    { name: "Gone", elements: [rect({ isDeleted: true })] },
    { name: "Logo", elements: [img.element] },
  ], { files: img.files }));
  writeJson(path.join(src, "old.excalidrawlib"), libraryV1([[rect()], [rect(), text("Nunito text", { fontFamily: 6 })]]));
  writeJson(path.join(src, "catalogues", "grid.excalidraw"), scene([rect(), text("#0 Router")]));
  fs.writeFileSync(path.join(src, "notes.txt"), "ignored");
  return src;
}

const read = (dir, rel) => fs.readFileSync(path.join(dir, rel), "utf8");

test("converts a folder tree: names, layout, annotations, outlined text, images", async () => {
  const src = fixtureTree();
  const out = tmpDir("x2s-out-");
  const events = [];
  const s = await run([src], out, {}, (e) => events.push(e));

  assert.deepEqual(listFiles(out), [
    "net/devices/000-router.svg",
    "net/devices/001-load-balancer.svg",
    "net/devices/003-logo.svg",
    "old/000.svg",
    "old/001-nunito-text.svg",
  ]);
  assert.equal(s.sources, 2, "scenes in folders are skipped without --scenes");
  assert.equal(s.items, 5);
  assert.equal(s.emptySkipped, 1);
  assert.equal(s.itemErrors, 0);
  assert.ok(fs.existsSync(path.join(out, MANIFEST_NAME)));

  const lb = read(out, "net/devices/001-load-balancer.svg");
  assert.match(lb, /^<svg[^>]*xmlns="http:\/\/www\.w3\.org\/2000\/svg"[^>]*><title>Load Balancer<\/title><desc>Excalidraw library net\/devices\.excalidrawlib, item #1\nText: Load Balancer<\/desc>/);
  assert.ok(!lb.includes("<text"), "text is outlined by default");
  assert.ok(!lb.includes("@font-face"));

  assert.match(read(out, "net/devices/000-router.svg"), /<title>Router<\/title>/);
  assert.match(read(out, "net/devices/003-logo.svg"), /<image[^>]+data:image\/svg\+xml;base64,/);
  assert.match(read(out, "old/000.svg"), /<title>old #0<\/title>/);
});

test("incremental: unchanged sources are skipped; changes and deletions clean up after themselves", async () => {
  const src = fixtureTree();
  const out = tmpDir("x2s-out-");
  await run([src], out);

  let s = await run([src], out);
  assert.equal(s.unchanged, 2);
  assert.equal(s.converted, 0);

  // A file the tool did not write is never touched.
  fs.writeFileSync(path.join(out, "net", "devices", "mine.svg"), "<svg/>");

  // Rename an item and drop another: the stale files go, the new name appears.
  const lib = path.join(src, "net", "devices.excalidrawlib");
  const json = JSON.parse(fs.readFileSync(lib, "utf8"));
  json.libraryItems[0].name = "Edge Router";
  json.libraryItems.pop();
  writeJson(lib, json);
  s = await run([src], out);
  assert.equal(s.converted, 1);
  assert.equal(s.unchanged, 1);
  assert.deepEqual(listFiles(path.join(out, "net")), ["devices/000-edge-router.svg", "devices/001-load-balancer.svg", "devices/mine.svg"]);

  // Delete a source: its output folder is pruned.
  fs.unlinkSync(path.join(src, "old.excalidrawlib"));
  s = await run([src], out);
  assert.equal(s.pruned, 1);
  assert.ok(!fs.existsSync(path.join(out, "old")));

  // Options are part of the fingerprint.
  s = await run([src], out, { theme: "dark" });
  assert.equal(s.converted, 1);
});

test("explicit scene files, live text, dry run", async () => {
  const src = fixtureTree();
  const out = tmpDir("x2s-out-");

  const dry = await run([src], out, { dryRun: true, scenes: true });
  assert.equal(dry.converted, 3);
  assert.deepEqual(listFiles(out), []);
  assert.ok(!fs.existsSync(path.join(out, MANIFEST_NAME)));

  const s = await run([path.join(src, "catalogues", "grid.excalidraw")], out, { text: "live" });
  assert.equal(s.items, 1);
  assert.deepEqual(listFiles(out), ["grid.svg"]);
  const svg = read(out, "grid.svg");
  assert.match(svg, /<title>grid<\/title>/);
  assert.match(svg, /<text[^>]*font-family="Virgil/);
  assert.match(svg, /@font-face\s*\{\s*font-family: Virgil/, "live mode embeds the font subset");
});

test("a broken source is reported and does not stop the run", async () => {
  const src = fixtureTree();
  fs.writeFileSync(path.join(src, "broken.excalidrawlib"), "{ not json");
  const out = tmpDir("x2s-out-");
  const events = [];
  const s = await run([src], out, {}, (e) => events.push(e));
  assert.equal(s.failed, 1);
  assert.equal(s.converted, 2);
  assert.match(events.find((e) => e.status === "failed").error.message, /invalid JSON/);
});

test("rejects inputs that do not exist or are not Excalidraw files", async () => {
  const out = tmpDir("x2s-out-");
  await assert.rejects(run(["/no/such/path"], out), /input not found/);
  const f = path.join(tmpDir(), "x.svg");
  fs.writeFileSync(f, "<svg/>");
  await assert.rejects(run([f], out), /not an \.excalidrawlib/);
});
