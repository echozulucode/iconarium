// The single seam between the UI and the native backend.
// Two implementations: `tauri.ts` (real) and `mock.ts` (in-browser, for dev/screenshots).
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

export type Unlisten = () => void;

export interface Backend {
  readonly kind: "tauri" | "mock";

  getAppState(): Promise<AppState>;
  /** Show the native folder picker and open the chosen folder as the active library. */
  pickAndOpenLibrary(): Promise<LibraryInfo | null>;
  openLibrary(path: string): Promise<LibraryInfo>;

  /** Ranked asset IDs for a query ("" = all, path order). Rejects with BackendError on bad regex. */
  search(query: string): Promise<Uint32Array>;
  /** Summaries for the given IDs (order preserved; unknown IDs omitted). */
  getAssets(ids: AssetId[], query?: string): Promise<AssetSummary[]>;
  getAssetDetail(id: AssetId): Promise<AssetDetail>;
  /** Reprioritize background work toward what the user sees. */
  setViewport(visible: AssetId[], nearby: AssetId[]): Promise<void>;

  /** URL for a gallery thumbnail (served by the backend, rendered on demand). */
  thumbnailUrl(id: AssetId, fingerprint: string): string;
  /** URL for the full SVG (viewer-normalized: root always has a viewBox). */
  svgUrl(id: AssetId, fingerprint: string): string;

  copyAssets(ids: AssetId[], format: AssetCopyFormat): Promise<void>;
  copyRegion(id: AssetId, region: Region, format: RegionCopyFormat): Promise<void>;
  saveRegion(id: AssetId, region: Region, format: RegionSaveFormat): Promise<string | null>;
  startDragAssets(ids: AssetId[]): Promise<void>;
  startDragRegion(id: AssetId, region: Region): Promise<void>;
  revealAssets(ids: AssetId[]): Promise<void>;
  openExternal(id: AssetId): Promise<void>;

  updateSettings(patch: Partial<Settings>): Promise<Settings>;

  onScanProgress(cb: (s: ScanStatus) => void): Promise<Unlisten>;
  onCatalogChanged(cb: (c: CatalogChanged) => void): Promise<Unlisten>;
  onThumbReady(cb: (t: ThumbReady) => void): Promise<Unlisten>;
}

export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

let instance: Promise<Backend> | null = null;

/** Lazily resolve the backend: Tauri when running inside the app, otherwise the mock. */
export function getBackend(): Promise<Backend> {
  if (!instance) {
    instance = isTauri()
      ? import("./tauri").then((m) => m.createTauriBackend())
      : import("./mock").then((m) => m.createMockBackend());
  }
  return instance;
}
