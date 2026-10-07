// Functional smoke test of the UI against the mock backend (no screenshots, just assertions).
//
//   npx vite --port 5173 &
//   node scripts/smoke.mjs [--base http://localhost:5173]
import { chromium } from "playwright";
import { existsSync } from "node:fs";
import assert from "node:assert/strict";

const args = process.argv.slice(2);
const BASE = args.includes("--base") ? args[args.indexOf("--base") + 1] : "http://localhost:5173";
const exe = [process.env.CHROMIUM_PATH, "/opt/pw-browsers/chromium-1194/chrome-linux/chrome"].filter(Boolean).find((p) => existsSync(p));
const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox"] });
const page = await browser.newPage({ viewport: { width: 1280, height: 820 } });
const infos = [];
const errors = [];
page.on("console", (m) => {
  if (m.type() === "info") infos.push(m.text());
  if (m.type() === "error") errors.push(m.text());
});
page.on("pageerror", (e) => errors.push(e.message));
const wait = (ms) => page.waitForTimeout(ms);
const selectedCount = () => page.evaluate(() => document.querySelectorAll("[aria-selected=true]").length);
const card = (n) => page.locator("[data-asset-id]").nth(n);
const cardId = async (n) => Number(await card(n).getAttribute("data-asset-id"));
const lastInfo = () => infos[infos.length - 1] ?? "";
let passed = 0;
async function step(name, fn) {
  await fn();
  passed++;
  console.log("ok  ", name);
}

await page.goto(BASE + "/");
await page.waitForSelector("[data-asset-id] img");
await wait(2200);

await step("click / ctrl / shift selection", async () => {
  await card(0).click();
  assert.equal(await selectedCount(), 1);
  await card(3).click({ modifiers: ["Shift"] });
  assert.equal(await selectedCount(), 4);
  await card(1).click({ modifiers: ["Control"] });
  assert.equal(await selectedCount(), 3);
  assert.match(await page.locator("footer").first().textContent(), /3 selected/);
});

await step("Esc clears, Ctrl+A selects all results", async () => {
  await page.keyboard.press("Escape");
  assert.equal(await selectedCount(), 0);
  await page.keyboard.press("Control+a");
  const footer = await page.locator("footer").first().textContent();
  assert.match(footer, /6,204 selected/);
  await page.keyboard.press("Escape");
});

await step("arrow keys move, shift extends, scroll into view", async () => {
  await card(0).click();
  await page.keyboard.press("ArrowRight");
  assert.equal(await page.locator("[aria-selected=true]").getAttribute("data-asset-id"), String(await cardId(1)));
  await page.keyboard.press("Shift+ArrowDown");
  assert.ok((await selectedCount()) > 2);
  for (let i = 0; i < 12; i++) await page.keyboard.press("ArrowDown");
  await wait(200);
  const visible = await page.evaluate(() => {
    const el = document.querySelector("[aria-selected=true]");
    const g = document.querySelector("[role=grid]").getBoundingClientRect();
    const r = el?.getBoundingClientRect();
    return !!r && r.top >= g.top - 1 && r.bottom <= g.bottom + 1;
  });
  assert.ok(visible, "focused card scrolled into view");
});

await step("Ctrl+C single → svg, multi → paths (toast)", async () => {
  await page.keyboard.press("Home");
  await page.keyboard.press("Control+c");
  await wait(150);
  assert.match(lastInfo(), /copyAssets .* svg/);
  await page.keyboard.press("Shift+ArrowRight");
  await page.keyboard.press("Control+c");
  await wait(150);
  assert.match(lastInfo(), /copyAssets .* path/);
  await page.getByText("Copied 2 paths").waitFor();
  await page.keyboard.press("Control+Shift+C");
  await wait(150);
  assert.match(lastInfo(), /copyAssets .* path/);
});

await step("drag initiation uses the whole selection when dragging a selected card", async () => {
  const b = await card(1).boundingBox();
  await page.mouse.move(b.x + 40, b.y + 40);
  await page.mouse.down();
  await page.mouse.move(b.x + 60, b.y + 60, { steps: 3 });
  await page.mouse.up();
  await wait(100);
  assert.match(lastInfo(), /startDragAssets/);
  assert.match(lastInfo(), /\[\s*\d+,\s*\d+\s*\]|Array\(2\)/);
  // dragging an unselected card selects it and drags only it
  const c = await card(5).boundingBox();
  await page.mouse.move(c.x + 40, c.y + 40);
  await page.mouse.down();
  await page.mouse.move(c.x + 70, c.y + 50, { steps: 3 });
  await page.mouse.up();
  await wait(100);
  assert.equal(await selectedCount(), 1);
  assert.match(lastInfo(), /startDragAssets/);
});

await step("search with content match, selection survives re-search", async () => {
  await page.keyboard.press("Control+f");
  await page.keyboard.type("motor controller");
  await wait(600);
  const n = await page.locator("[data-asset-id]").count();
  assert.ok(n > 0);
  assert.ok((await page.locator("[aria-label^='Matched text']").count()) > 0, "matched text shown");
  await card(2).click();
  const id = await cardId(2);
  await page.keyboard.press("Control+f");
  await page.keyboard.press("End");
  await page.keyboard.type(" -zzz");
  await wait(600);
  assert.equal(await page.locator("[aria-selected=true]").getAttribute("data-asset-id"), String(id));
  await page.keyboard.press("Control+a");
  await page.keyboard.type("re:pump([");
  await wait(500);
  await page.getByRole("alert").filter({ hasText: "Invalid regular expression" }).waitFor();
  await page.keyboard.press("Escape"); // clears query
  await wait(500);
});

await step("limit_exceeded / parse_error cards", async () => {
  await page.keyboard.press("Control+f");
  await page.keyboard.type("scans");
  await wait(600);
  await page.getByText("Preview unavailable").first().waitFor();
  await page.getByText("File exceeds rendering limit").first().waitFor();
  await page.keyboard.press("Control+a");
  await page.keyboard.type("broken-export");
  await wait(600);
  await page.getByText("Can't read SVG").first().waitFor();
});

await step("viewer: open with Enter, zoom keys, fit, region create/resize/move, Esc sequence", async () => {
  await page.keyboard.press("Control+a");
  await page.keyboard.type("system-overview");
  await wait(600);
  await card(0).click();
  await page.keyboard.press("Enter");
  await page.waitForSelector("[role=dialog] img");
  await wait(400);
  const zoomText = () => page.locator("[role=dialog] button[title='Actual size (0)']").first().textContent();
  const z0 = await zoomText();
  await page.keyboard.press("+");
  assert.notEqual(await zoomText(), z0);
  await page.keyboard.press("0");
  assert.equal(await zoomText(), "100%");
  await page.keyboard.press("f");
  assert.equal(await zoomText(), z0);
  await page.keyboard.press("s");
  const cv = await page.locator("[role=dialog] .touch-none").boundingBox();
  await page.mouse.move(cv.x + 300, cv.y + 200);
  await page.mouse.down();
  await page.mouse.move(cv.x + 600, cv.y + 400, { steps: 6 });
  await page.mouse.up();
  const label = page.getByText(/SVG units$/);
  const l1 = await label.textContent();
  // resize with SE handle
  const se = await page.locator("[data-handle=se]").boundingBox();
  await page.mouse.move(se.x + 4, se.y + 4);
  await page.mouse.down();
  await page.mouse.move(se.x + 104, se.y + 54, { steps: 5 });
  await page.mouse.up();
  const l2 = await label.textContent();
  assert.notEqual(l1, l2);
  // move keeps size
  const body = await page.locator("[data-region-body]").boundingBox();
  await page.mouse.move(body.x + 20, body.y + 20);
  await page.mouse.down();
  await page.mouse.move(body.x + 80, body.y + 50, { steps: 4 });
  await page.mouse.up();
  assert.equal(await label.textContent(), l2);
  // zoom keeps svg size
  await page.mouse.move(cv.x + 500, cv.y + 300);
  await page.mouse.wheel(0, -300);
  await wait(150);
  assert.equal(await label.textContent(), l2);
  // Ctrl+C copies region
  await page.keyboard.press("Control+c");
  await wait(150);
  assert.match(lastInfo(), /copyRegion .* svg/);
  // grip drag
  const g = await page.locator("[data-grip]").boundingBox();
  await page.mouse.move(g.x + 5, g.y + 5);
  await page.mouse.down();
  await page.mouse.move(g.x + 30, g.y + 30, { steps: 3 });
  await page.mouse.up();
  await wait(100);
  assert.match(lastInfo(), /startDragRegion/);
  // minimap click recentres
  await page.keyboard.press("+");
  await page.keyboard.press("+");
  const mm = page.getByRole("navigation", { name: "Minimap" });
  await mm.waitFor();
  const mb = await mm.boundingBox();
  await page.mouse.click(mb.x + 5, mb.y + 5);
  await wait(100);
  // Esc: clear region, then close
  await page.keyboard.press("Escape");
  assert.equal(await page.locator("[data-handle]").count(), 0);
  await page.keyboard.press("Control+c");
  await wait(150);
  assert.match(lastInfo(), /copyAssets .* svg/);
  await page.keyboard.press("Escape");
  await wait(200);
  assert.equal(await page.locator("[role=dialog]").count(), 0);
  assert.equal(await selectedCount(), 1, "selection kept after closing viewer");
});

await step("context menu keyboard + properties", async () => {
  await page.keyboard.press("Shift+F10");
  await page.getByRole("menu").waitFor();
  await page.keyboard.press("End");
  await page.keyboard.press("Enter");
  await page.getByRole("dialog", { name: /system-overview/ }).waitFor();
  await page.getByText("Content hash").waitFor();
  await page.keyboard.press("Escape");
  await wait(150);
  assert.equal(await page.getByRole("dialog").count(), 0);
});

await step("settings: hide folder line", async () => {
  await page.getByRole("button", { name: "Settings" }).click();
  await page.getByRole("menuitemcheckbox", { name: /Show folder/ }).click();
  await page.keyboard.press("Escape");
  await page.keyboard.press("Control+f");
  await page.keyboard.press("Escape");
  await wait(600);
  assert.equal(await page.getByText("electrical/power", { exact: true }).count(), 0);
});

await step("library switch → progressive scan", async () => {
  await page.getByRole("button", { name: /Engineering Assets/ }).click();
  await page.getByRole("menuitem", { name: /network-diagrams/ }).click();
  await page.getByText(/Indexing|Looking for SVG/).first().waitFor();
  await page.getByText("Indexed", { exact: true }).waitFor({ timeout: 10000 });
  assert.ok((await page.locator("[data-asset-id]").count()) > 0);
});

await browser.close();
if (errors.length) {
  console.log("page errors:\n" + errors.join("\n"));
  process.exit(1);
}
console.log(`\n${passed} smoke steps passed`);
