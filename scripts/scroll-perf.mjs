// Fast-scroll measurement of the virtualized grid (mock backend).
//
//   npm run build && npx vite preview --port 4173 &
//   node scripts/scroll-perf.mjs [--base http://localhost:4173] [--size small|medium|large]
//
// Reports max mounted cards and frame timings, with and without thumbnail images
// (to separate React/virtualization cost from SVG rasterization of the mock thumbnails).
import { chromium } from "playwright";
import { existsSync } from "node:fs";

const args = process.argv.slice(2);
const arg = (n, d) => (args.includes(`--${n}`) ? args[args.indexOf(`--${n}`) + 1] : d);
const BASE = arg("base", "http://localhost:4173");
const SIZE = arg("size", "small");
const exe = [process.env.CHROMIUM_PATH, "/opt/pw-browsers/chromium-1194/chrome-linux/chrome"].filter(Boolean).find((p) => existsSync(p));
const browser = await chromium.launch({ executablePath: exe, args: ["--no-sandbox"] });

async function measure(hideImages, stepPx) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 820 } });
  await page.goto(BASE + "/");
  await page.waitForSelector("[data-asset-id] img", { timeout: 20000 });
  await page.getByRole("radio", { name: new RegExp(SIZE, "i") }).click();
  if (hideImages) await page.addStyleTag({ content: "[data-asset-id] img{display:none!important}" });
  await page.waitForTimeout(2500);
  const r = await page.evaluate(async (step) => {
    const grid = document.querySelector("[role=grid]");
    const frames = [];
    let max = 0;
    let last = performance.now();
    const t0 = last;
    while (grid.scrollTop + grid.clientHeight < grid.scrollHeight - 10 && frames.length < 2000) {
      grid.scrollTop += step;
      await new Promise((res) => requestAnimationFrame(res));
      const now = performance.now();
      frames.push(now - last);
      last = now;
      max = Math.max(max, document.querySelectorAll("[data-asset-id]").length);
    }
    frames.sort((a, b) => a - b);
    const pct = (p) => frames[Math.min(frames.length - 1, Math.floor(frames.length * p))];
    return {
      frames: frames.length,
      scrollHeight: grid.scrollHeight,
      maxMounted: max,
      p50: +pct(0.5).toFixed(1),
      p95: +pct(0.95).toFixed(1),
      max: +frames[frames.length - 1].toFixed(1),
      totalMs: Math.round(performance.now() - t0),
    };
  }, stepPx);
  await page.close();
  return r;
}

for (const [hide, step] of [
  [true, 900],
  [false, 900],
  [false, 120],
]) {
  const r = await measure(hide, step);
  console.log(`${SIZE} cards, ${hide ? "no images " : "with images"}, ${step}px/frame:`, JSON.stringify(r));
}
await browser.close();
