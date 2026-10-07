// Visual check + virtualization measurement against the mock backend.
//
//   npx vite --port 5173 &          (mock backend is auto-selected outside Tauri)
//   node scripts/screenshots.mjs    [--base http://localhost:5173] [--only light|dark]
//
// Writes docs/screenshots/*.png and prints the max number of mounted cards during a fast scroll.
import { chromium } from "playwright";
import { mkdir } from "node:fs/promises";
import { existsSync } from "node:fs";
import path from "node:path";

const args = process.argv.slice(2);
const arg = (name, def) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : def;
};
const BASE = arg("base", "http://localhost:5173");
const ONLY = arg("only", null);
const OUT = path.resolve("docs/screenshots");
const CANDIDATES = [
  process.env.CHROMIUM_PATH,
  "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
].filter(Boolean);
const executablePath = CANDIDATES.find((p) => existsSync(p));

await mkdir(OUT, { recursive: true });
const browser = await chromium.launch({ executablePath, args: ["--no-sandbox"] });
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

const errors = [];

async function newPage(scheme, query = "") {
  const ctx = await browser.newContext({ viewport: { width: 1280, height: 820 }, colorScheme: scheme, deviceScaleFactor: 1 });
  const page = await ctx.newPage();
  page.on("pageerror", (e) => errors.push(`[${scheme}] pageerror: ${e.message}`));
  page.on("console", (m) => {
    if (m.type() === "error") errors.push(`[${scheme}] console: ${m.text()}`);
  });
  await page.goto(`${BASE}/${query}`);
  return page;
}

async function waitCards(page) {
  await page.waitForSelector("[data-asset-id] img", { timeout: 15000 });
  await sleep(400);
}

const shot = async (page, name) => {
  await page.screenshot({ path: path.join(OUT, `${name}.png`) });
  console.log("saved", name);
};

async function card(page, n) {
  return page.locator("[data-asset-id]").nth(n);
}

async function run(scheme) {
  // 1. Empty state (no library)
  {
    const page = await newPage(scheme, "?mock=empty");
    await page.waitForSelector("text=Open an SVG library");
    await sleep(300);
    await shot(page, `empty-${scheme}`);
    await page.context().close();
  }

  // 2. Gallery mid-scan
  {
    const page = await newPage(scheme, "?mock=scan");
    await page.waitForSelector("[data-asset-id]", { timeout: 15000 });
    await sleep(900);
    await shot(page, `gallery-scan-${scheme}`);
    await page.context().close();
  }

  const page = await newPage(scheme);
  await waitCards(page);
  await sleep(2200); // let "Checking for changes…" finish
  await shot(page, `gallery-${scheme}`);

  // 3. Search with content matches
  await page.keyboard.press("Control+f");
  await page.keyboard.type("motor controller", { delay: 20 });
  await sleep(700);
  await shot(page, `search-content-${scheme}`);

  // 3b. Bad regex
  await page.keyboard.press("Control+a");
  await page.keyboard.type("re:pump([", { delay: 10 });
  await sleep(500);
  await shot(page, `search-error-${scheme}`);

  // 3c. No results
  await page.keyboard.press("Control+a");
  await page.keyboard.type("flux capacitor", { delay: 10 });
  await sleep(500);
  await shot(page, `search-empty-${scheme}`);

  // 4. Multi-selection + context menu
  await page.keyboard.press("Control+a");
  await page.keyboard.type("valve", { delay: 10 });
  await sleep(600);
  await (await card(page, 1)).click();
  await (await card(page, 5)).click({ modifiers: ["Shift"] });
  await (await card(page, 9)).click({ modifiers: ["Control"] });
  await sleep(200);
  const box = await (await card(page, 4)).boundingBox();
  await page.mouse.click(box.x + box.width / 2, box.y + box.height / 2, { button: "right" });
  await sleep(250);
  await shot(page, `multiselect-menu-${scheme}`);
  await page.keyboard.press("Escape");
  await page.keyboard.press("Escape");

  // 4b. Single item context menu
  await (await card(page, 2)).click();
  const b2 = await (await card(page, 2)).boundingBox();
  await page.mouse.click(b2.x + b2.width / 2, b2.y + 40, { button: "right" });
  await sleep(250);
  await shot(page, `context-menu-${scheme}`);
  // 5. Properties dialog from the menu
  await page.getByRole("menuitem", { name: /Properties/ }).click();
  await sleep(500);
  await shot(page, `properties-${scheme}`);
  await page.keyboard.press("Escape");
  await sleep(200);

  // 6. Viewer at fit
  await page.keyboard.press("Control+f");
  await page.keyboard.press("Control+a");
  await page.keyboard.type("system-overview", { delay: 10 });
  await sleep(700);
  await (await card(page, 0)).dblclick();
  await page.waitForSelector("[role=dialog] img", { timeout: 10000 });
  await sleep(700);
  await shot(page, `viewer-fit-${scheme}`);

  // 7. Viewer zoomed, region selected, region menu, minimap
  const canvas = await page.locator("[role=dialog] .touch-none").boundingBox();
  const cx = canvas.x + canvas.width / 2;
  const cy = canvas.y + canvas.height / 2;
  await page.mouse.move(cx - 120, cy - 60);
  for (let i = 0; i < 4; i++) {
    await page.mouse.wheel(0, -120);
    await sleep(40);
  }
  await sleep(200);
  await page.keyboard.press("s");
  await page.mouse.move(cx - 380, cy - 200);
  await page.mouse.down();
  await page.mouse.move(cx - 100, cy - 60, { steps: 5 });
  await page.mouse.move(cx + 120, cy + 60, { steps: 8 });
  await sleep(100);
  await shot(page, `viewer-region-drag-${scheme}`);
  await page.mouse.up();
  await sleep(150);
  await page.mouse.click(cx, cy, { button: "right" });
  await sleep(250);
  await shot(page, `viewer-region-menu-${scheme}`);
  await page.keyboard.press("Escape");

  // Region survives zoom
  await page.mouse.move(cx + 200, cy + 100);
  await page.mouse.wheel(0, 240);
  await sleep(300);
  await shot(page, `viewer-region-zoomed-out-${scheme}`);
  await page.keyboard.press("Escape"); // clear region
  await page.keyboard.press("Escape"); // close viewer
  await sleep(200);
  await page.context().close();
}

async function measureVirtualization() {
  const page = await newPage("light");
  await waitCards(page);
  await page.click("text=S", { strict: false }).catch(() => {});
  await sleep(300);
  const result = await page.evaluate(async () => {
    const grid = document.querySelector("[role=grid]");
    const total = grid.scrollHeight;
    let max = 0;
    let frames = 0;
    const t0 = performance.now();
    let maxFrame = 0;
    let last = t0;
    while (grid.scrollTop + grid.clientHeight < total - 10 && frames < 600) {
      grid.scrollTop += 900;
      await new Promise((r) => requestAnimationFrame(r));
      const now = performance.now();
      maxFrame = Math.max(maxFrame, now - last);
      last = now;
      max = Math.max(max, document.querySelectorAll("[data-asset-id]").length);
      frames++;
    }
    return { scrollHeight: total, frames, maxMounted: max, avgFrameMs: (performance.now() - t0) / frames, maxFrameMs: maxFrame };
  });
  console.log("virtualization (small cards, fast scroll):", JSON.stringify(result));
  await page.context().close();
  return result;
}

const schemes = ONLY ? [ONLY] : ["light", "dark"];
for (const s of schemes) await run(s);
await measureVirtualization();
await browser.close();
if (errors.length) {
  console.log("\nPage errors:\n" + errors.join("\n"));
  process.exitCode = 1;
}
