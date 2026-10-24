import { test } from "node:test";
import assert from "node:assert/strict";

import { baseNameOf, itemText, kindOf, parseSource, SourceError } from "../lib/source.mjs";
import { indexWidth, itemFileName, safeName, slugify } from "../lib/naming.mjs";
import { lookupMeta } from "../lib/meta.mjs";
import { annotate, escapeXml } from "../lib/svg.mjs";
import { libraryV1, libraryV2, rect, scene, text } from "./helpers.mjs";

test("kindOf / baseNameOf", () => {
  assert.equal(kindOf("a/B.EXCALIDRAWLIB"), "library");
  assert.equal(kindOf("x.excalidraw"), "scene");
  assert.equal(kindOf("x.svg"), null);
  assert.equal(baseNameOf("/p/aws-icons.excalidrawlib"), "aws-icons");
  assert.equal(baseNameOf("c:/p/cat.excalidraw"), "cat");
});

test("parse v2: names, ids, deleted elements, empty items", () => {
  const lib = libraryV2([
    { name: "  Server ", elements: [rect()] },
    { elements: [rect({ isDeleted: true })] },
    { name: "", elements: [rect(), text("Label")] },
  ]);
  const p = parseSource(JSON.stringify(lib), "x.excalidrawlib");
  assert.equal(p.kind, "library");
  assert.equal(p.version, 2);
  assert.deepEqual(p.items.map((i) => [i.index, i.id, i.name, i.empty]), [
    [0, "item-0", "Server", false],
    [1, "item-1", null, true],
    [2, "item-2", null, false],
  ]);
  assert.deepEqual(itemText(p.items[2].elements), ["Label"]);
});

test("parse v1 and bare-array libraries, BOM tolerated", () => {
  const p = parseSource("\ufeff" + JSON.stringify(libraryV1([[rect()], [rect(), rect()]])), "x.excalidrawlib");
  assert.equal(p.items.length, 2);
  assert.equal(p.items[1].elements.length, 2);
  assert.equal(parseSource(JSON.stringify([[rect()]]), "x.excalidrawlib").items.length, 1);
});

test("parse scene as one item named after the file", () => {
  const p = parseSource(JSON.stringify(scene([rect()])), "dir/catalogue.excalidraw");
  assert.equal(p.kind, "scene");
  assert.deepEqual(p.items.map((i) => i.name), ["catalogue"]);
});

test("parse errors", () => {
  assert.throws(() => parseSource("{", "x.excalidrawlib"), SourceError);
  assert.throws(() => parseSource('{"type":"excalidrawlib"}', "x.excalidrawlib"), /neither/);
  assert.throws(() => parseSource("{}", "x.excalidraw"), /elements/);
});

test("itemText prefers unwrapped originalText and collapses whitespace", () => {
  assert.deepEqual(itemText([text("a b", { text: "a\nb", originalText: "a   b" }), rect(), text("  ")]), ["a b"]);
});

test("slugify", () => {
  assert.equal(slugify("Cloud Run (v2)"), "cloud-run-v2");
  assert.equal(slugify("Café / Ünïcode"), "cafe-unicode");
  assert.equal(slugify("数据库 Server"), "数据库-server");
  assert.equal(slugify("***"), "");
  assert.equal(slugify("CON"), "con-");
  assert.ok(slugify("word ".repeat(40)).length <= 60);
  assert.ok(!slugify("word ".repeat(40)).endsWith("-"));
});

test("safeName and file names", () => {
  assert.equal(safeName('a<b>:c"d|e?f*g.'), "a_b__c_d_e_f_g");
  assert.equal(safeName("aux"), "_aux");
  assert.equal(indexWidth(9), 3);
  assert.equal(indexWidth(1000), 3);
  assert.equal(indexWidth(1001), 4);
  assert.equal(itemFileName(7, 3, "Cloud Run"), "007-cloud-run.svg");
  assert.equal(itemFileName(7, 3, null), "007.svg");
});

test("lookupMeta: path key first, then library key; normalizes keyword shapes", () => {
  const meta = {
    "gcp__gcp-icons__3": { summary: "Bank building", kw: ["bank", "finance", "bank"] },
    "gcp-icons__4": { name: "Shield", keywords: "security, password" },
    "gcp-icons__5": {},
  };
  assert.deepEqual(lookupMeta(meta, "gcp", "gcp-icons", 3), { name: null, summary: "Bank building", keywords: ["bank", "finance"] });
  assert.deepEqual(lookupMeta(meta, "other/dir", "gcp-icons", 4), { name: "Shield", summary: null, keywords: ["security", "password"] });
  assert.equal(lookupMeta(meta, "gcp", "gcp-icons", 5), null);
  assert.equal(lookupMeta(null, "", "x", 0), null);
});

test("annotate inserts title/desc after the root tag and escapes them", () => {
  const out = annotate('<svg a=">" xmlns="http://www.w3.org/2000/svg"><title>old</title><g/></svg>', { title: "A & <B>", desc: 'x "y"' });
  assert.equal(out, '<svg a=">" xmlns="http://www.w3.org/2000/svg"><title>A &amp; &lt;B&gt;</title><desc>x &quot;y&quot;</desc><g/></svg>');
  assert.equal(escapeXml("a\u0001b"), "ab");
});
