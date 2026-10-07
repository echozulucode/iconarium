// Contract types mirrored from crates/svg-core/src/model.rs and config.rs (serde camelCase).
// Keep in sync with docs/implementation-plan.md §3.

export type AssetId = number;

export type ProcessingState = "discovered" | "ready" | "limit_exceeded" | "parse_error" | "missing";

export interface Region {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface ViewBox {
  minX: number;
  minY: number;
  width: number;
  height: number;
}

export type MatchField = "filename" | "path" | "title" | "text" | "desc" | "id_class";

export interface MatchInfo {
  field: MatchField;
  snippet: string;
}

export interface AssetSummary {
  id: AssetId;
  filename: string;
  relDir: string;
  state: ProcessingState;
  width: number | null;
  height: number | null;
  fingerprint: string;
  matchInfo?: MatchInfo;
}

export interface AssetDetail {
  id: AssetId;
  filename: string;
  relativePath: string;
  absolutePath: string;
  fileSize: number;
  mtimeMs: number;
  state: ProcessingState;
  parseError: string | null;
  width: number | null;
  height: number | null;
  viewBox: ViewBox | null;
  /** Effective coordinate system for regions (viewBox, else 0 0 w h). */
  docBox: ViewBox | null;
  elementCount: number | null;
  contentHash: string | null;
  fingerprint: string;
  title: string;
  description: string;
  /** File size ≥ warn limit. */
  sizeWarning: boolean;
}

export interface LibraryInfo {
  id: number;
  path: string;
  displayName: string;
  createdAt: number;
  lastOpened: number;
  lastScan: number | null;
}

export type ScanPhase = "idle" | "loading" | "discovering" | "reconciling" | "processing";

export interface ScanStatus {
  phase: ScanPhase;
  discovered: number;
  processed: number;
  total: number;
  message: string;
}

export interface Limits {
  warnFileBytes: number;
  maxFileBytes: number;
  maxNodes: number;
  maxEmbeddedRasterBytes: number;
  maxTextNodeChars: number;
  maxExtractedTextBytes: number;
  maxRenderPixels: number;
  maxNestingDepth: number;
  maxRenderTextChars: number;
}

export type ViewerBackground = "checkerboard" | "white" | "dark";
export type GallerySize = "small" | "medium" | "large";

export interface Settings {
  limits: Limits;
  thumbnailSize: number;
  prefillThumbnails: boolean;
  workerThreads: number | null;
  scanBatchSize: number;
  ignoreHidden: boolean;
  clipboardIncludeSvgText: boolean;
  clipboardIncludeBitmapWithSvg: boolean;
  clipboardPngFallbackScale: number;
  viewerBackground: ViewerBackground;
  gallerySize: GallerySize;
  showRelativeDir: boolean;
}

export interface AppState {
  library: LibraryInfo | null;
  recent: LibraryInfo[];
  scan: ScanStatus;
  settings: Settings;
  /** Number of assets in the active library catalog. */
  totalAssets: number;
}

export type AssetCopyFormat = "svg" | "png" | "png_white" | "path" | "filename";
export type RegionCopyFormat = "svg" | "png" | "png2x" | "png_white";
export type RegionSaveFormat = "svg" | "png";

export interface CatalogChanged {
  total: number;
  reason: "scan" | "watch" | "metadata" | "load";
}

export interface ThumbReady {
  ids: AssetId[];
}

/** Error shape returned by backend commands. */
export interface BackendError {
  kind: string;
  message: string;
}
