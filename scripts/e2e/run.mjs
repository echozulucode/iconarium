// End-to-end test of the REAL app (Rust backend + WebKitGTK WebView) driven through
// tauri-driver (W3C WebDriver). Linux only (WebKitWebDriver); see scripts/e2e/README.md.
//
//   xvfb-run -a -s "-screen 0 1280x900x24" node scripts/e2e/run.mjs [options]
//
// Options:
//   --bin <path>        app binary (default target/release/svg-library-browser, else target/debug/…)
//   --home <dir>        isolated HOME for the app (default /tmp/e2e-home; wiped at start)
//   --work <dir>        scratch dir for the mutable copy of dataset A (default /tmp/e2e-work)
//   --skip <ids>        comma-separated scenario ids to skip (e.g. 9 to skip the 50k dataset)
//   --only <ids>        run only these scenario ids (dependencies are NOT run automatically)
//   --drag              also try start_drag_assets (may block on Linux; off by default)
//   --full-d            in 3b, also wait until dataset D is fully indexed (slow)
//   --shots <dir>       screenshot directory (default docs/screenshots)
//   --json <file>       write machine-readable results
import { remote } from "webdriverio";
import { spawn, execFileSync } from "node:child_process";
import { existsSync, mkdirSync, rmSync, cpSync, readdirSync, readFileSync, writeFileSync, unlinkSync, statSync } from "node:fs";
import { join, resolve, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import net from "node:net";

const ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "../..");
const argv = process.argv.slice(2);
const arg = (name, def) => (argv.includes(name) ? argv[argv.indexOf(name) + 1] : def);
const flag = (name) => argv.includes(name);
const idList = (s) => new Set((s ?? "").split(",").map((x) => x.trim()).filter(Boolean));

const BIN = resolve(
  arg(
    "--bin",
    [join(ROOT, "target/release/svg-library-browser"), join(ROOT, "target/debug/svg-library-browser")].find((p) => existsSync(p)) ?? "",
  ),
);
const HOME = resolve(arg("--home", "/tmp/e2e-home"));
const WORK = resolve(arg("--work", "/tmp/e2e-work"));
const SHOTS = resolve(arg("--shots", join(ROOT, "docs/screenshots")));
const JSON_OUT = arg("--json", null);
const SKIP = idList(arg("--skip", ""));
const ONLY = idList(arg("--only", ""));
const DATA = join(ROOT, "datasets");
const IDENT = "com.svglibrary.browser";
const PORT = 4444;

if (!existsSync(BIN)) throw new Error(`app binary not found: ${BIN} (build it first, see README)`);
if (!process.env.DISPLAY) throw new Error("DISPLAY not set — run under xvfb-run");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const now = () => performance.now();
const log = (...a) => console.log(new Date().toISOString().slice(11, 23), ...a);

// ---------------------------------------------------------------------------------------
// Environment: fresh HOME + scratch copy of dataset A, tauri-driver.

rmSync(HOME, { recursive: true, force: true });
mkdirSync(HOME, { recursive: true });
rmSync(WORK, { recursive: true, force: true });
mkdirSync(WORK, { recursive: true });
const LIB_A = join(WORK, "A");
cpSync(join(DATA, "A"), LIB_A, { recursive: true });
mkdirSync(SHOTS, { recursive: true });

const appEnv = { ...process.env, HOME };
for (const k of ["XDG_DATA_HOME", "XDG_CACHE_HOME", "XDG_CONFIG_HOME", "XDG_STATE_HOME"]) delete appEnv[k];
const appLogDir = join(HOME, ".local/share", IDENT, "logs");
const driverLog = join(WORK, "tauri-driver.log");

async function portOpen(port) {
  return new Promise((res) => {
    const s = net.connect(port, "127.0.0.1", () => (s.destroy(), res(true)));
    s.on("error", () => res(false));
  });
}
if (await portOpen(PORT)) throw new Error(`port ${PORT} busy — is another tauri-driver running?`);
const { openSync } = await import("node:fs");
const driverFd = openSync(driverLog, "w");
const driver = spawn("tauri-driver", ["--port", String(PORT)], { env: appEnv, stdio: ["ignore", driverFd, driverFd] });
for (let i = 0; i < 100 && !(await portOpen(PORT)); i++) await sleep(100);

let browser = null;
let lastSessionMs = null;
async function startApp() {
  const t0 = now();
  browser = await remote({
    hostname: "127.0.0.1",
    port: PORT,
    logLevel: "error",
    connectionRetryTimeout: 120_000,
    capabilities: { "tauri:options": { application: BIN } },
  });
  lastSessionMs = now() - t0;
  await browser.setTimeout({ script: 120_000 });
  return lastSessionMs;
}
async function stopApp() {
  if (!browser) return;
  try {
    await browser.deleteSession();
  } catch {}
  browser = null;
  await sleep(500);
}

// ---------------------------------------------------------------------------------------
// In-page helpers.

/** Call a backend command directly. Binary responses (search) are decoded to number[]. */
async function invoke(cmd, args = {}) {
  const r = await browser.execute(
    function (cmd, args) {
      const t0 = performance.now();
      return window.__TAURI_INTERNALS__.invoke(cmd, args).then(
        (v) => {
          const ms = performance.now() - t0;
          if (v instanceof ArrayBuffer) v = Array.from(new Uint32Array(v));
          else if (ArrayBuffer.isView(v)) v = Array.from(new Uint32Array(v.buffer, v.byteOffset, v.byteLength >> 2));
          return { ok: true, v, ms };
        },
        (e) => ({ ok: false, e: typeof e === "string" ? e : JSON.stringify(e), ms: performance.now() - t0 }),
      );
    },
    cmd,
    args,
  );
  if (!r.ok) {
    const err = new Error(`${cmd} failed: ${r.e}`);
    err.backend = r.e;
    throw err;
  }
  invoke.lastMs = r.ms;
  return r.v;
}

/** Snapshot of what the user sees. */
function pageState() {
  return browser.execute(function () {
    const grid = document.querySelector('[role="grid"]');
    const gr = grid ? grid.getBoundingClientRect() : null;
    const cards = Array.from(document.querySelectorAll("[data-asset-id]"));
    let visible = 0,
      loaded = 0,
      states = { unavailable: 0, cantRead: 0, missing: 0, noThumb: 0 };
    for (const c of cards) {
      const r = c.getBoundingClientRect();
      if (!gr || r.bottom <= gr.top || r.top >= gr.bottom) continue;
      visible++;
      const img = c.querySelector("img");
      if (img && img.complete && img.naturalWidth > 0 && img.style.opacity !== "0") loaded++;
      const t = c.textContent || "";
      if (t.includes("Preview unavailable")) states.unavailable++;
      if (t.includes("Can't read SVG")) states.cantRead++;
      if (t.includes("File missing")) states.missing++;
      if (t.includes("No thumbnail")) states.noThumb++;
    }
    const footer = document.querySelector("footer");
    const status = document.querySelector('footer [role="status"]');
    const toasts = Array.from(document.querySelectorAll('[role="alert"],[role="status"]'))
      .filter((e) => !e.closest("footer"))
      .map((e) => e.textContent.trim())
      .filter(Boolean);
    return {
      cards: cards.length,
      visible,
      loaded,
      states,
      status: status ? status.textContent.trim() : null,
      footer: footer ? footer.textContent.replace(/\s+/g, " ").trim() : null,
      selectedText: footer ? (footer.textContent.match(/([\d,]+) selected/) || [])[1] || "0" : "0",
      body: document.body.innerText.slice(0, 400),
      toasts,
      searchError: (document.querySelector("#search-error") || {}).textContent || null,
      scrollTop: grid ? grid.scrollTop : null,
      scrollHeight: grid ? grid.scrollHeight : null,
      viewer: !!document.querySelector('[role="dialog"][aria-label^="Viewer"]'),
    };
  });
}

/** Install a high-frequency monitor recording when milestones first appear (page clock). */
function startMonitor() {
  return browser.execute(function () {
    if (window.__e2eMon) clearInterval(window.__e2eMon.timer);
    const initial = new Set(Array.from(document.querySelectorAll("[data-asset-id]")).map((c) => c.getAttribute("data-asset-id")));
    const m = { t0: performance.now(), firstCard: null, firstThumb: null, allVisible: null, indexed: null, statuses: [], gaps: [] };
    let last = performance.now();
    m.timer = setInterval(() => {
      const t = performance.now();
      m.gaps.push(t - last);
      last = t;
      const rel = t - m.t0;
      const grid = document.querySelector('[role="grid"]');
      const gr = grid && grid.getBoundingClientRect();
      // Asset IDs are unique across libraries: cards still showing the previous library don't count.
      const cards = Array.from(document.querySelectorAll("[data-asset-id]")).filter((c) => !initial.has(c.getAttribute("data-asset-id")));
      if (m.firstCard === null && cards.length) m.firstCard = rel;
      if (gr && cards.length) {
        let vis = 0,
          ok = 0;
        for (const c of cards) {
          const r = c.getBoundingClientRect();
          if (r.bottom <= gr.top || r.top >= gr.bottom) continue;
          vis++;
          const img = c.querySelector("img");
          if ((img && img.complete && img.naturalWidth > 0) || /Preview unavailable|Can't read SVG|No thumbnail/.test(c.textContent)) ok++;
        }
        if (m.firstThumb === null && ok > 0) m.firstThumb = rel;
        if (m.allVisible === null && vis > 0 && ok === vis) m.allVisible = rel;
      }
      const s = document.querySelector('footer [role="status"]');
      const st = s ? s.textContent.trim() : "";
      const prev = m.statuses[m.statuses.length - 1];
      if (st && (!prev || prev.s !== st.replace(/[\d,]+/g, "#"))) m.statuses.push({ t: Math.round(rel), s: st.replace(/[\d,]+/g, "#"), raw: st });
      if (m.indexed === null && st === "Indexed" && m.firstCard !== null) m.indexed = rel;
    }, 20);
    window.__e2eMon = m;
  });
}
function readMonitor() {
  return browser.execute(function () {
    const m = window.__e2eMon;
    if (!m) return null;
    const g = m.gaps.slice().sort((a, b) => a - b);
    const p = (q) => (g.length ? g[Math.min(g.length - 1, Math.floor(q * g.length))] : null);
    return {
      firstCard: m.firstCard,
      firstThumb: m.firstThumb,
      allVisible: m.allVisible,
      indexed: m.indexed,
      statuses: m.statuses.map((x) => `${x.t}ms ${x.raw}`),
      timerGapP50: p(0.5),
      timerGapP99: p(0.99),
      timerGapMax: g.length ? g[g.length - 1] : null,
    };
  });
}

async function poll(fn, { timeout = 30_000, interval = 50, what = "condition" } = {}) {
  const t0 = now();
  let last;
  while (now() - t0 < timeout) {
    last = await fn();
    if (last) return { ms: now() - t0, value: last };
    await sleep(interval);
  }
  const e = new Error(`timed out after ${timeout} ms waiting for ${what}`);
  e.last = last;
  throw e;
}

async function shot(name) {
  const p = join(SHOTS, `e2e-${name}.png`);
  // WebKitGTK under Xvfb (software compositing) can hand out a frame that lags the DOM by a
  // few hundred ms; let it settle so the picture matches what was just asserted.
  await sleep(600);
  await browser.saveScreenshot(p);
  shots.push(p);
  return p;
}

const KEY = { Ctrl: "", Shift: "", Enter: "", Escape: "" };

/** Viewport coordinates of an element's center (+ offset). */
async function centerOf(el, dx = 0, dy = 0) {
  const r = await browser.execute((e) => {
    e.scrollIntoView({ block: "nearest" });
    const b = e.getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2 };
  }, el);
  return { x: Math.round(r.x + dx), y: Math.round(r.y + dy) };
}
const pad = (n) => Array.from({ length: n }, () => ({ type: "pause", duration: 0 }));

/** Click with modifiers held, as one W3C action sequence (key + pointer sources). */
async function clickWith(el, mods = [], { button = 0 } = {}) {
  const downs = mods.map((m) => ({ type: "keyDown", value: KEY[m] }));
  const ups = mods.map((m) => ({ type: "keyUp", value: KEY[m] }));
  const p = await centerOf(el);
  await browser.performActions([
    { type: "key", id: "kbd", actions: [...downs, ...pad(3), ...ups] },
    {
      type: "pointer",
      id: "mouse",
      parameters: { pointerType: "mouse" },
      actions: [
        ...pad(downs.length),
        { type: "pointerMove", duration: 0, origin: "viewport", x: p.x, y: p.y },
        { type: "pointerDown", button },
        { type: "pointerUp", button },
        ...pad(ups.length),
      ],
    },
  ]);
  await browser.releaseActions();
  await sleep(60);
}
async function doubleClick(el) {
  const p = await centerOf(el);
  await browser.performActions([
    {
      type: "pointer",
      id: "mouse",
      parameters: { pointerType: "mouse" },
      actions: [
        { type: "pointerMove", duration: 0, origin: "viewport", x: p.x, y: p.y },
        { type: "pointerDown", button: 0 },
        { type: "pointerUp", button: 0 },
        { type: "pointerDown", button: 0 },
        { type: "pointerUp", button: 0 },
      ],
    },
  ]);
  await browser.releaseActions();
}
async function keys(...ks) {
  const seq = ks.map((k) => KEY[k] ?? k);
  await browser.performActions([
    { type: "key", id: "kbd", actions: [...seq.map((v) => ({ type: "keyDown", value: v })), ...seq.reverse().map((v) => ({ type: "keyUp", value: v }))] },
  ]);
  await browser.releaseActions();
  await sleep(60);
}
/** Mouse drag between two viewport points. */
async function dragPoints(a, b, steps = 8) {
  const moves = [];
  for (let i = 1; i <= steps; i++) {
    moves.push({ type: "pointerMove", duration: 16, origin: "viewport", x: Math.round(a.x + ((b.x - a.x) * i) / steps), y: Math.round(a.y + ((b.y - a.y) * i) / steps) });
  }
  await browser.performActions([
    {
      type: "pointer",
      id: "mouse",
      parameters: { pointerType: "mouse" },
      actions: [
        { type: "pointerMove", duration: 0, origin: "viewport", x: Math.round(a.x), y: Math.round(a.y) },
        { type: "pause", duration: 50 },
        { type: "pointerDown", button: 0 },
        { type: "pause", duration: 50 },
        ...moves,
        { type: "pause", duration: 50 },
        { type: "pointerUp", button: 0 },
      ],
    },
  ]);
  await browser.releaseActions();
}

// Clipboard (X11 CLIPBOARD selection) via xclip.
function xclip(target, { binary = false } = {}) {
  const t0 = now();
  try {
    return xclipRaw(target, binary);
  } finally {
    if (process.env.E2E_VERBOSE) log(`xclip ${target} ${Math.round(now() - t0)} ms`);
  }
}
function xclipRaw(target, binary) {
  const a = ["-selection", "clipboard", "-o"];
  if (target) a.splice(2, 0, "-t", target);
  try {
    const out = execFileSync("xclip", a, { timeout: 5000, stdio: ["ignore", "pipe", "pipe"] });
    return binary ? out : out.toString("utf8");
  } catch (e) {
    return null;
  }
}
const clipTargets = () => (xclip("TARGETS") ?? "").split("\n").map((s) => s.trim()).filter(Boolean);
function clearClipboard() {
  const t0 = now();
  try {
    // xclip forks to serve the selection; detach its stdio or execFileSync waits for it.
    execFileSync("sh", ["-c", "printf 'e2e-cleared' | xclip -selection clipboard -i >/dev/null 2>&1"], { timeout: 5000, stdio: "ignore" });
  } catch {}
  if (process.env.E2E_VERBOSE) log(`clearClipboard ${Math.round(now() - t0)} ms`);
}
function pngSize(buf) {
  if (!buf || buf.length < 24 || buf.readUInt32BE(0) !== 0x89504e47) return null;
  return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
}

function appLog() {
  try {
    return readdirSync(appLogDir)
      .map((f) => readFileSync(join(appLogDir, f), "utf8"))
      .join("\n");
  } catch {
    return "";
  }
}

/** Put text into the search box the way an input event would (React onChange path).
 *  (WebKitWebDriver key synthesis under Xvfb drops Shift for some symbols: ':' → ';'.) */
async function typeSearch(q) {
  await browser.execute((q) => {
    const input = document.querySelector('input[aria-label="Search SVGs"]');
    input.focus();
    Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value").set.call(input, q);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  }, q);
}
async function clearSearch() {
  await typeSearch("");
  await sleep(300);
}
const resultCount = (footer) => {
  const m = footer && footer.match(/([\d,]+) results? of/);
  return m ? Number(m[1].replace(/,/g, "")) : null;
};
const assetCount = (footer) => {
  const m = footer && footer.match(/^([\d,]+) assets?/);
  return m ? Number(m[1].replace(/,/g, "")) : null;
};
async function openLibraryViaInvoke(path) {
  await startMonitor();
  const t0 = now();
  const info = await invoke("open_library", { path });
  return { info, invokeMs: now() - t0 };
}
async function waitGallery(timeout = 60_000) {
  return poll(async () => {
    const s = await pageState();
    return s.cards > 0 ? s : null;
  }, { timeout, what: "gallery cards" });
}
async function waitIndexed(timeout = 120_000) {
  return poll(async () => {
    const s = await pageState();
    return s.status === "Indexed" ? s : null;
  }, { timeout, interval: 200, what: '"Indexed" status' });
}
async function idsFor(query) {
  return invoke("search", { query });
}
async function summaries(ids) {
  const out = [];
  for (let i = 0; i < ids.length; i += 2000) out.push(...(await invoke("get_assets", { ids: ids.slice(i, i + 2000), query: null })));
  return out;
}

// ---------------------------------------------------------------------------------------
// Scenario runner.

const results = [];
const shots = [];
function assert(cond, msg) {
  if (!cond) throw new Error(`assertion failed: ${msg}`);
}
async function scenario(id, name, fn) {
  id = String(id);
  if (SKIP.has(id) || (ONLY.size && !ONLY.has(id))) {
    results.push({ id, name, status: "SKIP" });
    return;
  }
  log(`=== [${id}] ${name}`);
  const t0 = now();
  const metrics = {};
  const notes = [];
  try {
    if (!browser) await startApp();
    await fn(metrics, notes);
    results.push({ id, name, status: "PASS", ms: Math.round(now() - t0), metrics, notes });
    log(`PASS [${id}]`, JSON.stringify(metrics));
  } catch (e) {
    results.push({ id, name, status: "FAIL", ms: Math.round(now() - t0), metrics, notes, error: e.message, last: e.last });
    log(`FAIL [${id}] ${e.message}`, e.last ? JSON.stringify(e.last).slice(0, 600) : "");
    try {
      if (browser) await shot(`fail-${id}`);
    } catch {}
  }
}
const r1 = (x) => (x == null ? null : Math.round(x));

// ---------------------------------------------------------------------------------------
// Scenarios.

await scenario(1, "Cold start, no library → empty state", async (m) => {
  m.sessionStartMs = r1(lastSessionMs);
  const { ms, value } = await poll(async () => {
    const s = await pageState();
    return s.body.includes("Open an SVG library") ? s : null;
  }, { timeout: 15_000, what: "empty state" });
  m.emptyStateAfterSessionMs = r1(ms);
  assert(value.body.includes("Select Folder"), "Select Folder button shown");
  const st = await invoke("get_app_state");
  assert(st.library === null, "no library in app state");
  await shot("01-empty");
});

let idsA = [];
await scenario(2, "Open dataset A → progressive gallery, thumbnails, Indexed", async (m, notes) => {
  const expected = Number(execFileSync("sh", ["-c", `find '${LIB_A}' -name '.*' -prune -o -type f -iname '*.svg' -print | wc -l`]).toString().trim());
  m.expectedSvgs = expected;
  const { info, invokeMs } = await openLibraryViaInvoke(LIB_A);
  m.openInvokeMs = r1(invokeMs);
  assert(info && info.path, "open_library returned LibraryInfo");
  const g = await waitGallery(30_000);
  m.cardsVisibleMs = r1(g.ms);
  await poll(async () => (await pageState()).loaded > 0, { timeout: 30_000, what: "first loaded thumbnail" });
  await shot("02a-gallery-filling");
  await waitIndexed(180_000);
  const mon = await readMonitor();
  Object.assign(m, {
    firstCardMs: r1(mon.firstCard),
    firstVisibleThumbMs: r1(mon.firstThumb),
    allVisibleThumbsMs: r1(mon.allVisible),
    indexedMs: r1(mon.indexed),
  });
  notes.push(`status sequence: ${mon.statuses.join(" → ")}`);
  const s = await pageState();
  m.footer = s.footer;
  assert(assetCount(s.footer) === expected, `footer shows ${expected} assets (got "${s.footer}")`);
  await poll(async () => {
    const st = await pageState();
    return st.visible > 0 && st.loaded === st.visible ? st : null;
  }, { timeout: 30_000, what: "all visible thumbnails loaded" });
  const st = await pageState();
  m.visibleCards = st.visible;
  m.visibleLoaded = st.loaded;
  idsA = await idsFor("");
  assert(idsA.length === expected, `search("") returns ${expected} ids (got ${idsA.length})`);
  const app = await invoke("get_app_state");
  assert(app.library && app.library.path === LIB_A, "app state library = A");
  await shot("02b-gallery-indexed");
});

await scenario("3a", "Search: token, glob, regex, bad regex (dataset A)", async (m, notes) => {
  const cases = [
    { q: "home", check: (f) => /home/i.test(f) },
    { q: "controls/*.svg", check: (f, rd) => rd === "controls" },
    { q: "re:^thermometer", check: (f) => /^thermometer/i.test(f) },
  ];
  for (const c of cases) {
    const ids = await idsFor(c.q);
    const backendMs = invoke.lastMs;
    await typeSearch(c.q);
    const { ms, value } = await poll(async () => {
      const s = await pageState();
      return resultCount(s.footer) === ids.length && !s.searchError && s.cards > 0 ? s : null;
    }, { timeout: 10_000, what: `UI results for ${c.q} (${ids.length})` });
    const sums = await summaries(ids);
    const bad = sums.filter((s) => !c.check(s.filename, s.relDir));
    m[`"${c.q}"`] = { results: ids.length, uiMs: r1(ms), backendMs: Math.round(backendMs * 10) / 10, footer: value.footer };
    assert(ids.length > 0, `results for ${c.q}`);
    if (c.q.includes("/")) assert(bad.length === 0, `${c.q}: all results in folder (bad: ${bad.slice(0, 3).map((b) => b.relDir + "/" + b.filename)})`);
    else if (bad.length) notes.push(`${c.q}: ${bad.length}/${ids.length} results matched via path/content, not filename`);
  }
  // Known svg-core issue: '^'-anchored regexes are prefiltered against "path\nstem_norm\n…",
  // which has no filename line, so '^name-with-separators' misses files in subfolders.
  const anchored = await idsFor("re:^thermometer-");
  const anchoredExpected = idsA.length ? (await summaries(idsA)).filter((s) => /^thermometer-/i.test(s.filename)).length : null;
  m.anchoredRegexProbe = `re:^thermometer- → ${anchored.length} (filenames starting "thermometer-": ${anchoredExpected})`;
  if (anchoredExpected !== null && anchored.length !== anchoredExpected) notes.push(`KNOWN CORE BUG: ${m.anchoredRegexProbe}`);
  await shot("03a-search-regex");
  await typeSearch("re:(unclosed");
  const { value } = await poll(async () => {
    const s = await pageState();
    return s.searchError ? s : null;
  }, { timeout: 10_000, what: "inline regex error" });
  m.badRegexError = value.searchError;
  assert(value.toasts.every((t) => !/Search failed/.test(t)), "bad regex shown inline, not as a toast");
  await shot("03b-search-bad-regex");
  await clearSearch();
});

await scenario(4, "Multi-selection, status count, context menu", async (m) => {
  await clearSearch();
  await poll(async () => (await pageState()).cards > 6, { what: "cards" });
  const cards = await browser.$$("[data-asset-id]");
  const sel = async () => Number((await pageState()).selectedText.replace(/,/g, ""));
  await clickWith(cards[0]);
  assert((await sel()) === 1, `click → 1 selected (got ${await sel()})`);
  await clickWith(cards[3], ["Shift"]);
  assert((await sel()) === 4, `shift+click → 4 selected (got ${await sel()})`);
  await clickWith(cards[1], ["Ctrl"]);
  assert((await sel()) === 3, `ctrl+click → 3 selected (got ${await sel()})`);
  const ariaSel = await browser.execute(() => document.querySelectorAll('[aria-selected="true"]').length);
  assert(ariaSel === 3, `3 cards aria-selected (got ${ariaSel})`);
  await shot("04a-multiselect");
  await keys("Ctrl", "a");
  const all = await sel();
  m.ctrlASelected = all;
  assert(all === idsA.length, `Ctrl+A selects all ${idsA.length} (got ${all})`);
  await keys("Escape");
  assert((await sel()) === 0, "Esc clears selection");
  await clickWith(cards[2]);
  // Keyboard context menu (Shift+F10) here; a real right-click is exercised in scenario 8.
  // (WebKitWebDriver loses the right-button release, so later left-drags would arrive as
  // chorded moves without a pointerdown — keep right-clicks for the end of the run.)
  await keys("Shift", "\uE03A"); // F10
  const menu = await poll(async () => {
    const items = await browser.execute(() =>
      Array.from(document.querySelectorAll('[role="menu"] [role^="menuitem"]')).map((e) => e.textContent.trim()),
    );
    return items.length ? items : null;
  }, { timeout: 5000, what: "context menu" });
  m.contextMenuItems = menu.value.length;
  await shot("04b-context-menu");
  await keys("Escape");
  await poll(async () => (await browser.execute(() => document.querySelectorAll('[role="menu"]').length)) === 0, { timeout: 3000, what: "menu closed" });
});

await scenario(5, "Viewer: open, zoom, fit, region select, clipboard (region + gallery)", async (m, notes) => {
  await clearSearch();
  // A real icon file with a known size.
  const homeIds = await idsFor("re:(^|/)home\\.svg$");
  await typeSearch("re:(^|/)home\\.svg$");
  await poll(async () => resultCount((await pageState()).footer) === homeIds.length, { what: "home.svg results" });
  const card = (await browser.$$("[data-asset-id]"))[0];
  const id = Number(await card.getAttribute("data-asset-id"));
  const detail = await invoke("get_asset_detail", { id });
  m.asset = detail.relativePath;
  await clickWith(card);
  await keys("Enter");
  const open = await poll(async () => {
    return browser.execute(() => {
      const img = document.querySelector('[role="dialog"] img');
      return img && img.complete && img.naturalWidth > 0 && img.style.opacity === "1" ? { w: img.naturalWidth, h: img.naturalHeight } : null;
    });
  }, { timeout: 10_000, what: "viewer image (Enter)" });
  m.viewerOpenMs = r1(open.ms);
  const zoomText = () =>
    browser.execute(() => ((document.querySelector('[role="dialog"]').textContent.match(/([\d,]+)%/) || [])[1] || "").replace(/,/g, ""));
  const z0 = Number(await zoomText());
  await (await browser.$('button[title="Zoom in (+)"]')).click();
  await sleep(250);
  const z1 = Number(await zoomText());
  assert(z1 > z0, `zoom in increases zoom (${z0}% → ${z1}%)`);
  await (await browser.$("button*=Fit")).click();
  await sleep(250);
  const z2 = Number(await zoomText());
  assert(z2 === z0, `fit restores zoom (${z2}% vs ${z0}%)`);
  m.zoom = `${z0}% → ${z1}% → fit ${z2}%`;
  await shot("05a-viewer-fit");
  // Region select drag.
  await (await browser.$('button[title="Select region (S)"]')).click();
  const ir = await browser.execute(() => {
    const b = document.querySelector('[role="dialog"] img').getBoundingClientRect();
    return { x: b.left + b.width / 2, y: b.top + b.height / 2, w: b.width, h: b.height };
  });
  const regionFooter = async () => {
    const t = await browser.execute(() => document.querySelector('[role="dialog"] footer').textContent);
    return /Selection/.test(t) ? t : null;
  };
  await sleep(300);
  if (process.env.E2E_DEBUG_POINTER) {
    await browser.execute(() => {
      window.__pe = [];
      for (const t of ["pointerdown", "pointermove", "pointerup", "pointercancel", "mousedown", "mouseup", "lostpointercapture"])
        window.addEventListener(t, (e) => window.__pe.push(`${t}@${Math.round(e.clientX)},${Math.round(e.clientY)} b${e.button}/${e.buttons} ${e.target.tagName}.${(e.target.className || "").toString().slice(0, 20)}`), true);
    });
  }
  let sel;
  for (let attempt = 1; attempt <= 3; attempt++) {
    await dragPoints({ x: ir.x - ir.w * 0.3, y: ir.y - ir.h * 0.3 }, { x: ir.x + ir.w * 0.15, y: ir.y + ir.h * 0.2 });
    sel = await poll(regionFooter, { timeout: 3000, what: "region selection in footer" }).catch((e) => (attempt === 3 ? Promise.reject(e) : null));
    if (process.env.E2E_DEBUG_POINTER) console.log("pointer events:", JSON.stringify(await browser.execute(() => window.__pe.splice(0))));
    if (sel) {
      if (attempt > 1) notes.push(`region drag needed ${attempt} attempts (synthetic pointer input)`);
      break;
    }
  }
  m.regionFooter = sel.value.replace(/\s+/g, " ").trim();
  await shot("05b-viewer-region");
  // Ctrl+C in the viewer = cropped SVG of the region (real UI path).
  clearClipboard();
  await keys("Ctrl", "c");
  await poll(async () => (await pageState()).toasts.some((t) => /Copied selection as SVG/.test(t)), { timeout: 10_000, what: "copy toast" });
  await sleep(200);
  const t1 = clipTargets();
  m.regionSvgTargets = t1.join(",");
  assert(t1.includes("image/svg+xml"), `region copy publishes image/svg+xml (targets: ${t1})`);
  assert(t1.includes("PNG") || t1.includes("image/png"), "region copy publishes PNG");
  const svg = xclip("image/svg+xml") ?? "";
  const vb = svg.match(/<svg[^>]*\bviewBox="0 0 ([\d.]+) ([\d.]+)"/);
  assert(vb, `cropped SVG root has viewBox="0 0 w h" (got ${svg.slice(0, 200)})`);
  m.regionSvgViewBox = `0 0 ${vb[1]} ${vb[2]}`;
  // copy_region png via invoke with an explicit region (half the doc box, 2x).
  const box = detail.docBox;
  const region = { x: box.minX, y: box.minY, width: box.width / 2, height: box.height / 2 };
  clearClipboard();
  await invoke("copy_region", { id, region, format: "png2x" });
  const t2 = clipTargets();
  m.regionPngTargets = t2.join(",");
  const png = xclip(t2.includes("PNG") ? "PNG" : "image/png", { binary: true });
  const ps = pngSize(png);
  assert(ps, "region PNG on clipboard");
  m.regionPng2x = `${ps.w}x${ps.h} for region ${region.width}x${region.height}`;
  assert(Math.abs(ps.w - region.width * 2) <= 1 && Math.abs(ps.h - region.height * 2) <= 1, `png2x size ${ps.w}x${ps.h}`);
  await invoke("copy_region", { id, region, format: "svg" });
  const svg2 = xclip("image/svg+xml") ?? "";
  assert(svg2.includes(`viewBox="0 0 ${region.width} ${region.height}"`), `invoke copy_region svg viewBox 0 0 ${region.width} ${region.height}`);
  // Close viewer (Esc twice: first clears the region).
  await keys("Escape");
  await sleep(150);
  if ((await pageState()).viewer) await keys("Escape");
  await poll(async () => !(await pageState()).viewer, { timeout: 3000, what: "viewer closed" });
  // Double-click also opens the viewer.
  await doubleClick((await browser.$$("[data-asset-id]"))[0]);
  await poll(async () => (await pageState()).viewer, { timeout: 5000, what: "viewer (double-click)" });
  await keys("Escape");
  await poll(async () => !(await pageState()).viewer, { timeout: 3000, what: "viewer closed" });
  // Gallery Ctrl+C (single → SVG) and Ctrl+Shift+C (path text).
  await clickWith((await browser.$$("[data-asset-id]"))[0]);
  clearClipboard();
  await keys("Ctrl", "c");
  await poll(async () => clipTargets().includes("image/svg+xml"), { timeout: 10_000, what: "gallery Ctrl+C svg on clipboard" });
  const t3 = clipTargets();
  m.galleryCopyTargets = t3.join(",");
  const src = readFileSync(detail.absolutePath, "utf8");
  assert((xclip("image/svg+xml") ?? "") === src, "gallery copy SVG bytes == source file");
  clearClipboard();
  await keys("Ctrl", "Shift", "c");
  const p = await poll(async () => {
    const t = xclip("UTF8_STRING") ?? xclip(null);
    return t && t.trim() ? t.trim() : null;
  }, { timeout: 10_000, what: "path text on clipboard" });
  m.pathCopy = p.value;
  assert(p.value === detail.absolutePath, `Ctrl+Shift+C copies absolute path (got ${p.value})`);
  // Whole-asset PNG (icons are upscaled so the longest side is ≥ 256 px).
  clearClipboard();
  await invoke("copy_assets", { ids: [id], format: "png" });
  const apng = pngSize(xclip("PNG", { binary: true }));
  assert(apng, "copy_assets png puts a PNG on the clipboard");
  m.assetPng = `${apng.w}x${apng.h} for ${detail.width}x${detail.height}`;
  assert(Math.max(apng.w, apng.h) >= 256, "icon PNG upscaled to ≥ 256 px");
  // Multi-asset SVG copy = file list (text/uri-list on X11) + paths as text.
  const two = idsA.slice(0, 2);
  clearClipboard();
  await invoke("copy_assets", { ids: two, format: "svg" });
  const tm = clipTargets();
  m.multiCopyTargets = tm.join(",");
  assert(tm.includes("text/uri-list"), "multi-asset copy publishes a file list");
  const uris = (xclip("text/uri-list") ?? "").split(/\r?\n/).filter(Boolean);
  assert(uris.length === 2 && uris.every((u) => u.startsWith("file://")), `two file:// URIs (got ${uris})`);
  await clearSearch();
});

await scenario(6, "Restart with same HOME → cached gallery, reconcile, Indexed", async (m, notes) => {
  await stopApp();
  const t0 = now();
  await startApp();
  m.sessionStartMs = r1(now() - t0);
  // Page-clock timestamps (performance.now() since navigation start).
  const g = await poll(async () => {
    return browser.execute(() => {
      const c = document.querySelector("[data-asset-id]");
      const img = c && c.querySelector("img");
      const st = document.querySelector('footer [role="status"]');
      return c ? { at: performance.now(), thumb: !!(img && img.complete && img.naturalWidth > 0), status: st ? st.textContent : null } : null;
    });
  }, { timeout: 20_000, interval: 20, what: "cached gallery" });
  m.cachedGalleryVisibleSinceNavMs = r1(g.value.at);
  m.statusAtFirstCard = g.value.status;
  await startMonitor();
  await sleep(100);
  await waitIndexed(60_000);
  const mon = await readMonitor();
  if (mon.indexed !== null) m.indexedAfterGalleryMs = r1(mon.indexed);
  notes.push(`status after first look: ${mon.statuses.join(" → ")}`);
  const s = await pageState();
  assert(assetCount(s.footer) === idsA.length, `restored ${idsA.length} assets (footer "${s.footer}")`);
  const st = await invoke("get_app_state");
  assert(st.library && st.library.path === LIB_A, "library restored");
  const logTxt = appLog();
  const opened = [...logTxt.matchAll(/opened library .*\((\d+) cached assets\) in ([\d.]+\w+)/g)].pop();
  if (opened) m.catalogLoad = `${opened[1]} cached assets in ${opened[2]}`;
  const sawChecking = mon.statuses.some((x) => /Checking for changes/.test(x)) || /Checking/.test(JSON.stringify(g));
  notes.push(sawChecking ? "observed 'Checking for changes…'" : "reconcile finished before first poll (A is small); verified via log");
  assert(opened && Number(opened[1]) === idsA.length, "startup loaded cached catalog (log)");
  await poll(async () => {
    const st = await pageState();
    return st.visible > 0 && st.loaded === st.visible;
  }, { timeout: 15_000, what: "visible thumbs after restart" });
  await shot("06-restart-restored");
});

await scenario(7, "Watcher: add, modify, delete, burst of 600", async (m, notes) => {
  const total = async () => assetCount((await pageState()).footer);
  const base = await total();
  assert(base > 0, "library open");
  const srcSvgs = readdirSync(join(DATA, "B/controls")).filter((f) => f.endsWith(".svg"));
  const dir = join(LIB_A, "controls");
  // add 3
  let t0 = now();
  for (let i = 1; i <= 3; i++) cpSync(join(DATA, "B/controls", srcSvgs[i]), join(dir, `e2e-added-${i}.svg`));
  await poll(async () => (await total()) === base + 3, { timeout: 15_000, what: "3 added assets" });
  m.addMs = r1(now() - t0);
  const findOne = async (name) => {
    const ids = await idsFor(`re:(^|/)${name.replace(/[.]/g, "\\.")}$`);
    return ids.length ? (await summaries(ids))[0] : null;
  };
  const before = await findOne("e2e-added-1.svg");
  assert(before, `added file searchable (search "e2e-added" → ${JSON.stringify(await summaries(await idsFor("e2e-added")))})`);
  // modify 1 (different content and size)
  t0 = now();
  cpSync(join(DATA, "B/controls", srcSvgs[10]), join(dir, "e2e-added-1.svg"));
  const mod = await poll(async () => {
    const s = await findOne("e2e-added-1.svg");
    return s && s.fingerprint !== before.fingerprint ? s : null;
  }, { timeout: 15_000, what: "modified fingerprint" });
  m.modifyMs = r1(now() - t0);
  // delete 1
  t0 = now();
  unlinkSync(join(dir, "e2e-added-2.svg"));
  await poll(async () => (await total()) === base + 2, { timeout: 15_000, what: "deleted asset gone" });
  m.deleteMs = r1(now() - t0);
  // burst: 600 files into an existing folder
  const pool = [];
  for (const sub of ["controls", "electrical", "network", "hydraulics"]) {
    for (const f of readdirSync(join(DATA, "B", sub))) if (f.endsWith(".svg")) pool.push(join(DATA, "B", sub, f));
  }
  assert(pool.length >= 600, "enough source files for burst");
  const logBefore = appLog().length;
  t0 = now();
  for (let i = 0; i < 600; i++) cpSync(pool[i], join(dir, `burst-${String(i).padStart(3, "0")}.svg`));
  m.burstCopyMs = r1(now() - t0);
  await poll(async () => (await total()) === base + 602, { timeout: 60_000, interval: 100, what: "burst of 600 reflected" });
  m.burstVisibleMs = r1(now() - t0);
  await sleep(1500);
  const after = appLog().slice(logBefore);
  m.fullReconcilesLogged = (after.match(/watcher: full reconcile/g) || []).length;
  const burstIds = await idsFor("burst-");
  assert(burstIds.length === 600, `600 burst files searchable (got ${burstIds.length})`);
  await typeSearch("burst-");
  await poll(async () => {
    const st = await pageState();
    return resultCount(st.footer) === 600 && st.visible > 0 && st.loaded === st.visible;
  }, { timeout: 30_000, what: "burst thumbnails" });
  await shot("07-watcher-burst");
  await clearSearch();
});

await scenario("3b", "Content search on dataset D (text-heavy)", async (m, notes) => {
  await openLibraryViaInvoke(join(DATA, "D"));
  await waitGallery(60_000);
  const mon0 = await readMonitor();
  m.firstCardMs = r1(mon0.firstCard);
  const q = '"motor controller"';
  await typeSearch(q);
  const t0 = now();
  const r = await poll(async () => {
    const s = await pageState();
    const lines = await browser.execute(() => Array.from(document.querySelectorAll('[aria-label^="Matched"]')).map((e) => e.getAttribute("aria-label")));
    return resultCount(s.footer) > 0 && lines.length ? { s, lines } : null;
  }, { timeout: 180_000, interval: 500, what: "content matches with explanation lines" });
  m.firstContentMatchMs = r1(now() - t0);
  m.results = resultCount(r.value.s.footer);
  m.sampleExplanation = r.value.lines[0];
  m.footer = r.value.s.footer;
  assert(r.value.lines.some((l) => /motor controller/i.test(l)), "explanation line quotes the matched text");
  await poll(async () => {
    const st = await pageState();
    return st.visible > 0 && st.loaded === st.visible;
  }, { timeout: 60_000, what: "content-result thumbnails" });
  await shot("03c-content-search");
  const ids = await idsFor(q);
  m.backendSearchMs = Math.round(invoke.lastMs * 10) / 10;
  m.backendResults = ids.length;
  if (flag("--full-d")) {
    // Content search is only complete once every file has been analyzed.
    const t0 = now();
    await waitIndexed(1_800_000);
    m.dIndexedAfterS = Math.round((now() - t0) / 1000 + (await readMonitor()).firstCard / 1000);
    m.backendResultsWhenIndexed = (await idsFor(q)).length;
  }
  await clearSearch();
});

await scenario(8, "Dataset E (pathological): no crash, state cards, graceful viewer", async (m, notes) => {
  await openLibraryViaInvoke(join(DATA, "E"));
  await waitGallery(60_000);
  await waitIndexed(300_000).catch((e) => notes.push(`not Indexed within 300 s: ${e.last && e.last.status}`));
  const mon = await readMonitor();
  m.indexedMs = r1(mon.indexed);
  notes.push(`status sequence: ${mon.statuses.join(" → ")}`);
  const st = await invoke("get_app_state");
  assert(st.library, "app alive, library open");
  // Compare manifest expectations with the backend's states.
  const manifest = readFileSync(join(DATA, "E/MANIFEST.tsv"), "utf8")
    .split("\n")
    .filter((l) => l && !l.startsWith("#"))
    .map((l) => l.split("\t"));
  const all = await summaries(await idsFor(""));
  const byPath = new Map(all.map((s) => [(s.relDir ? s.relDir + "/" : "") + s.filename, s]));
  const stateName = { ParseError: "parse_error", LimitExceeded: "limit_exceeded", Ready: "ready" };
  const mismatches = [];
  let checked = 0;
  for (const [path, expect] of manifest) {
    const s = byPath.get(path);
    if (expect === "Skipped") {
      if (s) mismatches.push(`${path}: expected not discovered, got ${s.state}`);
      continue;
    }
    if (!s) {
      mismatches.push(`${path}: not in catalog`);
      continue;
    }
    checked++;
    if (stateName[expect] && s.state !== stateName[expect]) mismatches.push(`${path}: expected ${stateName[expect]}, got ${s.state}`);
    if (expect === "Analyzed" && s.state === "discovered") mismatches.push(`${path}: never analyzed`);
  }
  m.manifestChecked = checked;
  m.manifestMismatches = mismatches.length;
  if (mismatches.length) notes.push(...mismatches.map((x) => "manifest: " + x));
  await typeSearch("limits");
  await poll(async () => {
    const s = await pageState();
    return s.states.unavailable > 0 ? s : null;
  }, { timeout: 30_000, what: "limit-exceeded cards" });
  await shot("08a-dataset-e-limits");
  await typeSearch("malformed");
  await poll(async () => (await pageState()).states.cantRead > 0, { timeout: 30_000, what: "parse-error cards" });
  await shot("08b-dataset-e-malformed");
  // Over-limit file: the UI must not offer the viewer (View disabled, Enter ignored) and the
  // backend paths the viewer uses must fail gracefully.
  const overIds = await idsFor("just-over-limit");
  await typeSearch("just-over-limit");
  await poll(async () => resultCount((await pageState()).footer) === overIds.length, { what: "over-limit file" });
  const id = overIds[0];
  const cardSel = `[data-asset-id="${id}"]`;
  await clickWith(await browser.$(cardSel));
  await keys("Enter");
  await sleep(500);
  assert(!(await pageState()).viewer, "Enter does not open the viewer for an over-limit asset");
  await clickWith(await browser.$(cardSel), [], { button: 2 });
  const view = await poll(async () => {
    return browser.execute(() => {
      const it = Array.from(document.querySelectorAll('[role="menu"] [role^="menuitem"]')).find((e) => /^View/.test(e.textContent.trim()));
      return it ? { disabled: it.getAttribute("aria-disabled") === "true" || it.disabled === true } : null;
    });
  }, { timeout: 5000, what: "View menu item" });
  m.viewMenuItemDisabled = view.value.disabled;
  assert(view.value.disabled, "View menu item disabled for over-limit asset");
  await shot("08c-over-limit-menu");
  await keys("Escape");
  // The viewer's image source fails cleanly (error event, no hang).
  const imgLoad = await browser.execute((url) => {
    return new Promise((res) => {
      const im = new Image();
      const t = setTimeout(() => res("timeout"), 10000);
      im.onload = () => (clearTimeout(t), res("loaded"));
      im.onerror = () => (clearTimeout(t), res("error"));
      im.src = url;
    });
  }, `svgfile://localhost/${id}/x`);
  m.svgfileOverLimit = imgLoad;
  assert(imgLoad === "error", `svgfile:// for over-limit asset errors cleanly (got ${imgLoad})`);
  const det = await invoke("get_asset_detail", { id });
  m.detailState = `${det.state}: ${det.parseError}`;
  assert(det.state === "limit_exceeded", "detail reports limit_exceeded");
  try {
    await invoke("copy_assets", { ids: [id], format: "svg" });
    throw new Error("copy_assets svg on an over-limit file unexpectedly succeeded");
  } catch (e) {
    if (!e.backend) throw e;
    m.copyOverLimit = e.backend.slice(0, 140);
  }
  await invoke("copy_assets", { ids: [id], format: "path" });
  assert((xclip("UTF8_STRING") ?? "").trim() === det.absolutePath, "Copy path still works for over-limit asset");
  await clearSearch();
});

await scenario(9, "Dataset C (50k): first thumbnails, scroll responsiveness, search during indexing", async (m, notes) => {
  const { invokeMs } = await openLibraryViaInvoke(join(DATA, "C"));
  m.openInvokeMs = r1(invokeMs);
  await waitGallery(120_000);
  await poll(async () => (await pageState()).loaded > 0, { timeout: 60_000, what: "first thumbnail" });
  let mon = await readMonitor();
  m.firstCardMs = r1(mon.firstCard);
  m.firstVisibleThumbMs = r1(mon.firstThumb);
  // Search latency while the scan/analysis is running.
  const lat = [];
  for (const q of ["motor", "valve", "re:^ic_", "network/*.svg", "pump -legacy"]) {
    const ids = await idsFor(q);
    lat.push(`${q}: ${ids.length} in ${Math.round(invoke.lastMs * 10) / 10} ms`);
  }
  m.searchDuringIndexing = lat;
  m.statusDuringSearch = (await pageState()).status;
  // Wait for the discovery to finish (total stable), but not for all analysis.
  const expectedC = Number(execFileSync("sh", ["-c", `find '${join(DATA, "C")}' -name '.*' -prune -o -type f -iname '*.svg' -print | wc -l`]).toString().trim());
  await poll(async () => {
    const s = await pageState();
    return s.status && !/Indexing|Scanning|Looking/.test(s.status) && assetCount(s.footer) === expectedC ? s : null;
  }, { timeout: 300_000, interval: 100, what: "discovery finished" }).then(
    () => browser.execute(() => performance.now() - window.__e2eMon.t0).then((t) => (m.discoveryDoneMs = r1(t))),
    () => notes.push("discovery still running after 300 s"),
  );
  m.totalAfterDiscovery = assetCount((await pageState()).footer);
  // Scroll to several positions; measure time until the visible thumbnails at the new position load.
  const positions = [0.25, 0.5, 0.9, 0.1];
  const scrollRes = [];
  await startMonitor();
  for (const f of positions) {
    const t0 = now();
    await browser.execute((f) => {
      const g = document.querySelector('[role="grid"]');
      g.scrollTop = Math.floor((g.scrollHeight - g.clientHeight) * f);
    }, f);
    const cards = await poll(async () => {
      const s = await pageState();
      return s.visible > 0 ? s : null;
    }, { timeout: 10_000, interval: 20, what: "cards at new position" });
    const thumbs = await poll(async () => {
      const s = await pageState();
      return s.visible > 0 && s.loaded + s.states.unavailable + s.states.cantRead + s.states.noThumb >= s.visible ? s : null;
    }, { timeout: 60_000, interval: 50, what: `thumbnails at ${f}` }).catch((e) => ({ ms: null, value: e.last }));
    scrollRes.push(`${Math.round(f * 100)}%: cards ${r1(cards.ms)} ms, all visible thumbs ${thumbs.ms == null ? "TIMEOUT" : r1(now() - t0) + " ms"}`);
  }
  await shot("09-dataset-c-scrolled");
  mon = await readMonitor();
  m.scroll = scrollRes;
  m.uiTimerGap = `p50 ${r1(mon.timerGapP50)} ms, p99 ${r1(mon.timerGapP99)} ms, max ${r1(mon.timerGapMax)} ms (20 ms timer)`;
  const ids = await idsFor("motor");
  m.searchMotorMs = Math.round(invoke.lastMs * 10) / 10;
  assert(ids.length > 0, "search works on 50k");
  assert(m.totalAfterDiscovery === expectedC, `all ${expectedC} SVGs discovered (got ${m.totalAfterDiscovery})`);
});

if (flag("--drag")) {
  await scenario(10, "start_drag_assets on Linux (no crash / no hang)", async (m, notes) => {
    const ids = await idsFor("");
    const r = await browser.execute(function (id) {
      return Promise.race([
        new Promise((res) => setTimeout(() => res("timeout"), 5000)),
        window.__TAURI_INTERNALS__.invoke("start_drag_assets", { ids: [id] }).then(
          () => "ok",
          (e) => "error: " + JSON.stringify(e),
        ),
      ]);
    }, ids[0]);
    m.result = r;
    const st = await invoke("get_app_state");
    assert(st.library, "app still responsive after drag attempt");
  });
}

// ---------------------------------------------------------------------------------------
await stopApp();
driver.kill();

const crashes = (appLog().match(/panicked|worker panicked/g) || []).length;
console.log("\n================ E2E SUMMARY ================");
console.log(`binary: ${BIN}`);
for (const r of results) {
  console.log(`[${r.status}] ${r.id}. ${r.name}${r.ms != null ? ` (${(r.ms / 1000).toFixed(1)} s)` : ""}`);
  if (r.error) console.log(`    error: ${r.error}`);
  for (const [k, v] of Object.entries(r.metrics ?? {})) console.log(`    ${k}: ${typeof v === "string" ? v : JSON.stringify(v)}`);
  for (const n of r.notes ?? []) console.log(`    note: ${n}`);
}
console.log(`panics in app log: ${crashes}`);
console.log(`screenshots: ${shots.map((s) => basename(s)).join(", ")}`);
if (JSON_OUT) writeFileSync(JSON_OUT, JSON.stringify({ bin: BIN, results, shots, panics: crashes }, null, 2));
process.exit(results.some((r) => r.status === "FAIL") ? 1 : 0);
