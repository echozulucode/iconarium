// Real backend: Tauri IPC commands + events + custom URI protocols.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { Backend, Unlisten } from "./backend";
import { normalizeError } from "./errors";
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

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  try {
    return await invoke<T>(cmd, args);
  } catch (e) {
    throw normalizeError(e);
  }
}

/** Decode the binary `search` response (little-endian u32 IDs). Also accepts number[] for robustness. */
export function decodeIds(raw: unknown): Uint32Array {
  if (raw instanceof Uint32Array) return raw;
  if (raw instanceof ArrayBuffer) return decodeBytes(new Uint8Array(raw));
  if (ArrayBuffer.isView(raw)) {
    const v = raw as ArrayBufferView;
    return decodeBytes(new Uint8Array(v.buffer, v.byteOffset, v.byteLength));
  }
  if (Array.isArray(raw)) return Uint32Array.from(raw as number[]);
  if (raw == null) return new Uint32Array(0);
  throw normalizeError({ kind: "protocol", message: "Unexpected search response" });
}

function decodeBytes(bytes: Uint8Array): Uint32Array {
  const n = Math.floor(bytes.byteLength / 4);
  const out = new Uint32Array(n);
  const dv = new DataView(bytes.buffer, bytes.byteOffset, n * 4);
  for (let i = 0; i < n; i++) out[i] = dv.getUint32(i * 4, true);
  return out;
}

function isWindows(): boolean {
  return typeof navigator !== "undefined" && /Windows/i.test(navigator.userAgent);
}

/** Custom protocol URL: http://{scheme}.localhost/... on Windows (WebView2), {scheme}://localhost/... elsewhere. */
export function protocolUrl(scheme: string, path: string, windows = isWindows()): string {
  return windows ? `http://${scheme}.localhost/${path}` : `${scheme}://localhost/${path}`;
}

export function createTauriBackend(): Backend {
  const win = isWindows();

  const on = async <T,>(event: string, cb: (p: T) => void): Promise<Unlisten> => {
    const un = await listen<T>(event, (e) => cb(e.payload));
    return un;
  };

  return {
    kind: "tauri",

    getAppState: () => call<AppState>("get_app_state"),
    pickAndOpenLibrary: () => call<LibraryInfo | null>("pick_and_open_library"),
    openLibrary: (path: string) => call<LibraryInfo>("open_library", { path }),

    search: async (query: string) => decodeIds(await call<unknown>("search", { query })),
    getAssets: (ids: AssetId[], query?: string) =>
      call<AssetSummary[]>("get_assets", { ids, query: query && query.trim() ? query : null }),
    getAssetDetail: (id: AssetId) => call<AssetDetail>("get_asset_detail", { id }),
    setViewport: (visible: AssetId[], nearby: AssetId[]) => call<void>("set_viewport", { visible, nearby }),

    thumbnailUrl: (id: AssetId, fingerprint: string) =>
      protocolUrl("thumb", `${id}/${encodeURIComponent(fingerprint)}`, win),
    svgUrl: (id: AssetId, fingerprint: string) =>
      protocolUrl("svgfile", `${id}/${encodeURIComponent(fingerprint)}`, win),

    copyAssets: (ids: AssetId[], format: AssetCopyFormat) => call<void>("copy_assets", { ids, format }),
    copyRegion: (id: AssetId, region: Region, format: RegionCopyFormat) =>
      call<void>("copy_region", { id, region, format }),
    saveRegion: (id: AssetId, region: Region, format: RegionSaveFormat) =>
      call<string | null>("save_region", { id, region, format }),
    startDragAssets: (ids: AssetId[]) => call<void>("start_drag_assets", { ids }),
    startDragRegion: (id: AssetId, region: Region) => call<void>("start_drag_region", { id, region }),
    revealAssets: (ids: AssetId[]) => call<void>("reveal_assets", { ids }),
    openExternal: (id: AssetId) => call<void>("open_external", { id }),

    updateSettings: (patch: Partial<Settings>) => call<Settings>("update_settings", { patch }),

    onScanProgress: (cb: (s: ScanStatus) => void) => on<ScanStatus>("scan://progress", cb),
    onCatalogChanged: (cb: (c: CatalogChanged) => void) => on<CatalogChanged>("catalog://changed", cb),
    onThumbReady: (cb: (t: ThumbReady) => void) => on<ThumbReady>("thumb://ready", cb),
  };
}
