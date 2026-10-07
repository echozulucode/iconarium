// User-facing commands: call the backend and report the outcome as a toast.
import { errorMessage } from "../api/errors";
import type { AssetCopyFormat, Region, RegionCopyFormat, RegionSaveFormat } from "../api/types";
import { roundRegion } from "../viewer/geometry";
import { useSelection } from "../stores/selectionStore";
import { toast } from "../stores/uiStore";
import { useViewer } from "../stores/viewerStore";
import { backend } from "./backendRef";

const COPY_LABEL: Record<AssetCopyFormat, (n: number) => string> = {
  svg: (n) => (n > 1 ? `Copied ${n} SVGs` : "Copied SVG"),
  png: (n) => (n > 1 ? `Copied ${n} PNGs` : "Copied PNG"),
  png_white: () => "Copied PNG with white background",
  path: (n) => (n > 1 ? `Copied ${n} paths` : "Copied path"),
  filename: (n) => (n > 1 ? `Copied ${n} filenames` : "Copied filename"),
};

const REGION_LABEL: Record<RegionCopyFormat, string> = {
  svg: "Copied selection as SVG",
  png: "Copied selection as PNG",
  png2x: "Copied selection as PNG (2×)",
  png_white: "Copied selection as PNG with white background",
};

export async function copyAssets(ids: number[], format: AssetCopyFormat): Promise<void> {
  if (ids.length === 0) return;
  try {
    await backend().copyAssets(ids, format);
    toast.success(COPY_LABEL[format](ids.length));
  } catch (e) {
    toast.error("Couldn't copy", errorMessage(e));
  }
}

export async function copyRegion(id: number, region: Region, format: RegionCopyFormat): Promise<void> {
  try {
    await backend().copyRegion(id, roundRegion(region), format);
    toast.success(REGION_LABEL[format]);
  } catch (e) {
    toast.error("Couldn't copy selection", errorMessage(e));
  }
}

export async function saveRegion(id: number, region: Region, format: RegionSaveFormat): Promise<void> {
  try {
    const path = await backend().saveRegion(id, roundRegion(region), format);
    if (path) toast.success(`Saved selection as ${format.toUpperCase()}`, path);
  } catch (e) {
    toast.error("Couldn't save selection", errorMessage(e));
  }
}

export async function revealAssets(ids: number[]): Promise<void> {
  if (ids.length === 0) return;
  try {
    await backend().revealAssets(ids);
  } catch (e) {
    toast.error("Couldn't reveal in Explorer", errorMessage(e));
  }
}

export async function openExternal(id: number): Promise<void> {
  try {
    await backend().openExternal(id);
  } catch (e) {
    toast.error("Couldn't open file", errorMessage(e));
  }
}

export async function dragAssets(ids: number[]): Promise<void> {
  if (ids.length === 0) return;
  try {
    await backend().startDragAssets(ids);
  } catch (e) {
    toast.error("Couldn't start drag", errorMessage(e));
  }
}

export async function dragRegion(id: number, region: Region): Promise<void> {
  try {
    await backend().startDragRegion(id, roundRegion(region));
  } catch (e) {
    toast.error("Couldn't drag selection", errorMessage(e));
  }
}

export function openViewer(id: number): void {
  void useViewer.getState().open(id);
}

/** Context-aware Ctrl+C in the gallery (plan §29): multi → paths, single → SVG. */
export function galleryCopy(): void {
  const ids = useSelection.getState().ordered();
  if (ids.length === 0) return;
  void copyAssets(ids, ids.length > 1 ? "path" : "svg");
}

/** Context-aware Ctrl+C in the viewer: region → cropped SVG, else whole SVG. */
export function viewerCopy(): void {
  const { openId, region } = useViewer.getState();
  if (openId == null) return;
  if (region) void copyRegion(openId, region, "svg");
  else void copyAssets([openId], "svg");
}
