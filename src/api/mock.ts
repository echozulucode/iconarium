// In-browser mock backend: deterministic procedural library, progressive discovery,
// search semantics like the real backend. Used outside Tauri (dev, screenshots, demos).
//
// URL switches (dev only):  ?mock=empty  → start without a library
//                           ?mock=scan   → start the default library with a fresh discovery scan
import type { Backend, Unlisten } from "./backend";
import { AppError } from "./errors";
import { generateLibrary, svgFor, type LibrarySpec, type MockAsset } from "./mocklib/generate";
import { matchDoc, parseQuery, searchDocs, type ParsedQuery } from "./mocklib/search";
import type {
  AppState,
  AssetCopyFormat,
  AssetDetail,
  AssetId,
  AssetSummary,
  CatalogChanged,
  LibraryInfo,
  Region,
  RegionCopyFormat,
  RegionSaveFormat,
  ScanStatus,
  Settings,
  ThumbReady,
} from "./types";

interface MockLibraryDef {
  info: LibraryInfo;
  spec: LibrarySpec;
}

const DAY = 86400000;
const NOW = Date.UTC(2026, 9, 6, 14, 30);

const LIBRARIES: MockLibraryDef[] = [
  {
    info: { id: 1, path: "C:\\Users\\eric\\Libraries\\Engineering Assets", displayName: "Engineering Assets", createdAt: NOW - 90 * DAY, lastOpened: NOW, lastScan: NOW - DAY },
    spec: { seed: 1 },
  },
  {
    info: { id: 2, path: "D:\\Projects\\Plant-7\\network-diagrams", displayName: "network-diagrams", createdAt: NOW - 40 * DAY, lastOpened: NOW - 2 * DAY, lastScan: NOW - 2 * DAY },
    spec: { seed: 2, include: ["network", "diagrams/network", "diagrams/architecture"] },
  },
  {
    info: { id: 3, path: "C:\\Users\\eric\\Design\\icon-sets", displayName: "icon-sets", createdAt: NOW - 20 * DAY, lastOpened: NOW - 6 * DAY, lastScan: NOW - 6 * DAY },
    spec: { seed: 3, include: ["ui"] },
  },
];

const PICKED: MockLibraryDef = {
  info: { id: 4, path: "C:\\Users\\eric\\Documents\\Vendor Symbols", displayName: "Vendor Symbols", createdAt: NOW, lastOpened: NOW, lastScan: null },
  spec: { seed: 4, include: ["electrical", "controls", "process"], diagrams: 0 },
};

const DEFAULT_SETTINGS: Settings = {
  limits: {
    warnFileBytes: 10 * 1024 * 1024,
    maxFileBytes: 25 * 1024 * 1024,
    maxNodes: 100_000,
    maxEmbeddedRasterBytes: 50 * 1024 * 1024,
    maxTextNodeChars: 10_000,
    maxExtractedTextBytes: 1024 * 1024,
    maxRenderPixels: 64 * 1024 * 1024,
    maxNestingDepth: 256,
    maxRenderTextChars: 200_000,
  },
  thumbnailSize: 256,
  prefillThumbnails: true,
  workerThreads: null,
  scanBatchSize: 256,
  ignoreHidden: true,
  clipboardIncludeSvgText: false,
  clipboardIncludeBitmapWithSvg: true,
  clipboardPngFallbackScale: 2,
  viewerBackground: "checkerboard",
  gallerySize: "medium",
  showRelativeDir: true,
};

const sleep = (ms: number) => new Promise<void>((r) => setTimeout(r, ms));

export function createMockBackend(): Backend {
  const params = typeof location !== "undefined" ? new URLSearchParams(location.search) : new URLSearchParams();
  const mode = params.get("mock");

  const libs = new Map<string, MockLibraryDef>(LIBRARIES.map((l) => [l.info.path, l]));
  let settings: Settings = { ...DEFAULT_SETTINGS, limits: { ...DEFAULT_SETTINGS.limits } };
  let current: { def: MockLibraryDef; assets: MockAsset[]; byId: Map<number, MockAsset>; discovered: number } | null = null;
  let scan: ScanStatus = { phase: "idle", discovered: 0, processed: 0, total: 0, message: "" };
  let scanToken = 0;
  const urlCache = new Map<string, string>();

  const scanListeners = new Set<(s: ScanStatus) => void>();
  const catalogListeners = new Set<(c: CatalogChanged) => void>();
  const thumbListeners = new Set<(t: ThumbReady) => void>();

  const emitScan = (s: ScanStatus) => {
    scan = s;
    scanListeners.forEach((cb) => cb({ ...s }));
  };
  const emitCatalog = (reason: CatalogChanged["reason"]) => {
    const total = current?.discovered ?? 0;
    catalogListeners.forEach((cb) => cb({ total, reason }));
  };

  const load = (def: MockLibraryDef, discovered: boolean) => {
    const assets = generateLibrary(def.spec);
    const byId = new Map(assets.map((a) => [a.id, a]));
    if (!discovered) for (const a of assets) a.state = "discovered";
    current = { def, assets, byId, discovered: discovered ? assets.length : 0 };
    def.info = { ...def.info, lastOpened: Date.now() };
  };

  const runDiscovery = async () => {
    const token = ++scanToken;
    const cur = current!;
    const total = cur.assets.length;
    const batches = 9;
    const per = Math.ceil(total / batches);
    emitScan({ phase: "discovering", discovered: 0, processed: 0, total: 0, message: "Discovering SVG files…" });
    for (let b = 0; b < batches; b++) {
      await sleep(240);
      if (token !== scanToken) return;
      cur.discovered = Math.min(total, cur.discovered + per);
      emitScan({ phase: "discovering", discovered: cur.discovered, processed: 0, total: cur.discovered, message: "" });
      emitCatalog("scan");
    }
    // Metadata + text extraction pass.
    const chunk = Math.ceil(total / 8);
    for (let p = 0; p < total; p += chunk) {
      await sleep(220);
      if (token !== scanToken) return;
      const ids: number[] = [];
      for (let i = p; i < Math.min(total, p + chunk); i++) {
        cur.assets[i].state = cur.assets[i].finalState;
        ids.push(cur.assets[i].id);
      }
      emitScan({ phase: "processing", discovered: total, processed: Math.min(total, p + chunk), total, message: "" });
      emitCatalog("metadata");
      thumbListeners.forEach((cb) => cb({ ids }));
    }
    cur.def.info = { ...cur.def.info, lastScan: Date.now() };
    emitScan({ phase: "idle", discovered: total, processed: total, total, message: "" });
  };

  const runReconcile = async () => {
    const token = ++scanToken;
    const total = current!.assets.length;
    emitScan({ phase: "reconciling", discovered: total, processed: 0, total, message: "Checking for changes…" });
    await sleep(1800);
    if (token !== scanToken) return;
    emitScan({ phase: "idle", discovered: total, processed: total, total, message: "" });
  };

  // Initial state
  if (mode !== "empty") {
    const def = LIBRARIES[0];
    if (mode === "scan") {
      load(def, false);
      setTimeout(() => void runDiscovery(), 300);
    } else {
      load(def, true);
      setTimeout(() => void runReconcile(), 200);
    }
  }

  const visibleAssets = () => (current ? current.assets.slice(0, current.discovered) : []);

  const requireAsset = (id: AssetId): MockAsset => {
    const a = current?.byId.get(id);
    if (!a || !current || current.assets.indexOf(a) >= current.discovered) throw new AppError("not_found", `Asset ${id} not found`);
    return a;
  };

  const toSummary = (a: MockAsset, pq: ParsedQuery | null): AssetSummary => {
    const processed = a.state !== "discovered";
    const s: AssetSummary = {
      id: a.id,
      filename: a.filename,
      relDir: a.relDir,
      state: a.state,
      width: processed ? a.width : null,
      height: processed ? a.height : null,
      fingerprint: a.fingerprint,
    };
    if (pq && !pq.empty) {
      const m = matchDoc(a, pq);
      if (m) s.matchInfo = m.info;
    }
    return s;
  };

  const blobUrl = (a: MockAsset): string => {
    const key = `${a.id}:${a.fingerprint}`;
    let u = urlCache.get(key);
    if (!u) {
      const svg = svgFor(a);
      u = typeof URL.createObjectURL === "function" ? URL.createObjectURL(new Blob([svg], { type: "image/svg+xml" })) : `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
      urlCache.set(key, u);
    }
    return u;
  };

  const info = (...args: unknown[]) => console.info("[mock backend]", ...args);

  const recents = (): LibraryInfo[] =>
    [...libs.values()].map((l) => l.info).sort((a, b) => b.lastOpened - a.lastOpened);

  const openDef = async (def: MockLibraryDef): Promise<LibraryInfo> => {
    scanToken++;
    libs.set(def.info.path, def);
    load(def, false);
    emitCatalog("load");
    void runDiscovery();
    return def.info;
  };

  const on = <T,>(set: Set<(p: T) => void>, cb: (p: T) => void): Promise<Unlisten> => {
    set.add(cb);
    return Promise.resolve(() => void set.delete(cb));
  };

  return {
    kind: "mock",

    async getAppState(): Promise<AppState> {
      await sleep(20);
      return {
        library: current?.def.info ?? null,
        recent: recents(),
        scan: { ...scan },
        settings: { ...settings },
        totalAssets: current?.discovered ?? 0,
      };
    },

    async pickAndOpenLibrary() {
      await sleep(150);
      return openDef(PICKED);
    },

    async openLibrary(path: string) {
      await sleep(30);
      const def =
        libs.get(path) ??
        ({
          info: {
            id: 100 + libs.size,
            path,
            displayName: path.split(/[\\/]/).filter(Boolean).pop() ?? path,
            createdAt: Date.now(),
            lastOpened: Date.now(),
            lastScan: null,
          },
          spec: { seed: path.length, iconLimit: 2000, diagrams: 40 },
        } satisfies MockLibraryDef);
      return openDef(def);
    },

    async search(query: string) {
      const t0 = performance.now();
      const ids = searchDocs(visibleAssets(), query); // throws AppError('invalid_query')
      const dt = performance.now() - t0;
      await sleep(Math.max(0, 6 - dt));
      return Uint32Array.from(ids);
    },

    async getAssets(ids: AssetId[], query?: string) {
      await sleep(8);
      if (!current) return [];
      let pq: ParsedQuery | null = null;
      if (query && query.trim()) {
        try {
          pq = parseQuery(query);
        } catch {
          pq = null;
        }
      }
      const out: AssetSummary[] = [];
      for (const id of ids) {
        const a = current.byId.get(id);
        if (a) out.push(toSummary(a, pq));
      }
      return out;
    },

    async getAssetDetail(id: AssetId): Promise<AssetDetail> {
      await sleep(15);
      const a = requireAsset(id);
      const processed = a.state !== "discovered";
      const docBox = a.viewBox ?? (a.width && a.height ? { minX: 0, minY: 0, width: a.width, height: a.height } : null);
      return {
        id: a.id,
        filename: a.filename,
        relativePath: a.relPath,
        absolutePath: `${current!.def.info.path}\\${a.relPath.replace(/\//g, "\\")}`,
        fileSize: a.fileSize,
        mtimeMs: a.mtimeMs,
        state: a.state,
        parseError: a.parseError,
        width: a.width,
        height: a.height,
        viewBox: a.viewBox,
        docBox,
        elementCount: processed ? a.elementCount : null,
        contentHash: processed && a.state === "ready" ? a.fingerprint + a.fingerprint.split("").reverse().join("") : null,
        fingerprint: a.fingerprint,
        title: a.title,
        description: a.desc,
        sizeWarning: a.fileSize >= settings.limits.warnFileBytes,
      };
    },

    async setViewport(visible: AssetId[], nearby: AssetId[]) {
      void visible;
      void nearby;
    },

    thumbnailUrl(id: AssetId) {
      const a = current?.byId.get(id);
      return a ? blobUrl(a) : "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'/%3E";
    },
    svgUrl(id: AssetId) {
      const a = current?.byId.get(id);
      return a ? blobUrl(a) : "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg'/%3E";
    },

    async copyAssets(ids: AssetId[], format: AssetCopyFormat) {
      await sleep(40);
      if (ids.length === 0) throw new AppError("invalid_argument", "Nothing selected");
      const bad = ids.map((id) => current?.byId.get(id)).find((a) => a && (format === "svg" || format.startsWith("png")) && a.state !== "ready" && a.state !== "discovered");
      if (bad) throw new AppError("limit_exceeded", `${bad.filename} can't be rendered (${bad.state === "parse_error" ? "invalid SVG" : "exceeds rendering limit"})`);
      info("copyAssets", ids, format);
    },
    async copyRegion(id: AssetId, region: Region, format: RegionCopyFormat) {
      await sleep(60);
      info("copyRegion", id, region, format);
    },
    async saveRegion(id: AssetId, region: Region, format: RegionSaveFormat) {
      await sleep(60);
      const a = requireAsset(id);
      info("saveRegion", id, region, format);
      return `C:\\Users\\eric\\Desktop\\${a.filename.replace(/\.svg$/, "")}-crop.${format}`;
    },
    async startDragAssets(ids: AssetId[]) {
      info("startDragAssets", ids);
    },
    async startDragRegion(id: AssetId, region: Region) {
      info("startDragRegion", id, region);
    },
    async revealAssets(ids: AssetId[]) {
      info("revealAssets", ids);
    },
    async openExternal(id: AssetId) {
      info("openExternal", id);
    },

    async updateSettings(patch: Partial<Settings>) {
      settings = { ...settings, ...patch, limits: { ...settings.limits, ...(patch.limits ?? {}) } };
      return { ...settings };
    },

    onScanProgress: (cb) => on(scanListeners, cb),
    onCatalogChanged: (cb) => on(catalogListeners, cb),
    onThumbReady: (cb) => on(thumbListeners, cb),
  };
}
