// Pure coordinate math for the viewer (plan §2.2, §21–§26).
//
// Coordinate spaces
//   screen  — CSS px relative to the top-left of the viewer canvas element
//   svg     — document user units inside `docBox` (viewBox, else 0 0 w h)
//
// The image is drawn as an <img> whose top-left sits at (panX, panY) and whose
// CSS size is `displaySize(doc, zoom)` — explicit width/height rather than a CSS
// scale transform so the engine re-rasterizes the vector crisply at every zoom.
//
//   svgX = box.minX + (screenX − panX) / displayW × box.width

import type { Region, ViewBox } from "../api/types";

export interface Point {
  x: number;
  y: number;
}
export interface Size {
  width: number;
  height: number;
}
export interface Rect extends Point, Size {}

export interface View {
  zoom: number;
  panX: number;
  panY: number;
}

export interface DocGeometry {
  box: ViewBox;
  /** CSS px size of the document at 100 % zoom. Aspect always equals box aspect. */
  baseWidth: number;
  baseHeight: number;
}

export const MIN_ZOOM = 0.01;
export const MAX_ZOOM = 64;
/** Largest edge of the rendered <img>, in CSS px. Keeps rasterization sane at extreme zoom. */
export const MAX_DISPLAY_PX = 32768;
/** Smallest edge we let the document shrink to. */
export const MIN_DISPLAY_PX = 8;

export const ZOOM_STEPS = [
  0.01, 0.02, 0.03, 0.05, 0.0625, 0.1, 0.125, 0.167, 0.25, 0.333, 0.5, 0.667, 0.75, 1, 1.25, 1.5, 2, 3, 4, 6, 8,
  12, 16, 24, 32, 48, 64,
];

export function docGeometry(width: number | null, height: number | null, docBox: ViewBox | null): DocGeometry {
  let box: ViewBox;
  if (docBox && docBox.width > 0 && docBox.height > 0) box = docBox;
  else if (width && height && width > 0 && height > 0) box = { minX: 0, minY: 0, width, height };
  else box = { minX: 0, minY: 0, width: 100, height: 100 };
  const aspect = box.height / box.width;
  let baseWidth = width && width > 0 ? width : box.width;
  // Guard absurd intrinsic sizes (e.g. width="1" with a huge viewBox).
  if (!Number.isFinite(baseWidth) || baseWidth <= 0) baseWidth = box.width;
  const baseHeight = baseWidth * aspect;
  return { box, baseWidth, baseHeight };
}

export function zoomLimits(doc: DocGeometry): { min: number; max: number } {
  const longest = Math.max(doc.baseWidth, doc.baseHeight);
  // Absolute cap for normal documents; tiny coordinate systems (e.g. viewBox 0 0 1 1 with
  // no size) may zoom further so they can still be shown at a usable size.
  const max = Math.min(MAX_DISPLAY_PX / longest, Math.max(MAX_ZOOM, 1024 / longest));
  const min = Math.max(MIN_ZOOM, MIN_DISPLAY_PX / longest);
  return { min: Math.min(min, max), max: Math.max(min, max) };
}

export function clampZoom(zoom: number, doc: DocGeometry): number {
  const { min, max } = zoomLimits(doc);
  if (!Number.isFinite(zoom)) return 1;
  return Math.min(max, Math.max(min, zoom));
}

export function displaySize(doc: DocGeometry, zoom: number): Size {
  return { width: doc.baseWidth * zoom, height: doc.baseHeight * zoom };
}

export function screenToSvg(p: Point, view: View, doc: DocGeometry): Point {
  const d = displaySize(doc, view.zoom);
  return {
    x: doc.box.minX + ((p.x - view.panX) / d.width) * doc.box.width,
    y: doc.box.minY + ((p.y - view.panY) / d.height) * doc.box.height,
  };
}

export function svgToScreen(p: Point, view: View, doc: DocGeometry): Point {
  const d = displaySize(doc, view.zoom);
  return {
    x: view.panX + ((p.x - doc.box.minX) / doc.box.width) * d.width,
    y: view.panY + ((p.y - doc.box.minY) / doc.box.height) * d.height,
  };
}

export function regionToScreen(r: Region, view: View, doc: DocGeometry): Rect {
  const a = svgToScreen({ x: r.x, y: r.y }, view, doc);
  const b = svgToScreen({ x: r.x + r.width, y: r.y + r.height }, view, doc);
  return { x: a.x, y: a.y, width: b.x - a.x, height: b.y - a.y };
}

/** Zoom so the document point under `anchor` (screen) stays under it. */
export function zoomAt(view: View, newZoom: number, anchor: Point, doc: DocGeometry): View {
  const zoom = clampZoom(newZoom, doc);
  const k = zoom / view.zoom;
  return {
    zoom,
    panX: anchor.x - (anchor.x - view.panX) * k,
    panY: anchor.y - (anchor.y - view.panY) * k,
  };
}

/** Multiplicative zoom factor for a wheel event; normalized across delta modes. */
export function wheelZoomFactor(deltaY: number, deltaMode = 0): number {
  const px = deltaMode === 1 ? deltaY * 16 : deltaMode === 2 ? deltaY * 400 : deltaY;
  const clamped = Math.max(-200, Math.min(200, px));
  return Math.exp(-clamped * 0.0022);
}

export function nextZoomStep(zoom: number, direction: 1 | -1): number {
  const eps = 1e-6;
  if (direction > 0) {
    for (const s of ZOOM_STEPS) if (s > zoom + eps) return s;
    return ZOOM_STEPS[ZOOM_STEPS.length - 1];
  }
  for (let i = ZOOM_STEPS.length - 1; i >= 0; i--) if (ZOOM_STEPS[i] < zoom - eps) return ZOOM_STEPS[i];
  return ZOOM_STEPS[0];
}

/** Centered "fit to window" view. */
export function fitView(doc: DocGeometry, viewport: Size, padding = 32): View {
  const aw = Math.max(1, viewport.width - padding * 2);
  const ah = Math.max(1, viewport.height - padding * 2);
  const zoom = clampZoom(Math.min(aw / doc.baseWidth, ah / doc.baseHeight), doc);
  return centeredView(doc, viewport, zoom);
}

export function centeredView(doc: DocGeometry, viewport: Size, zoom: number): View {
  const z = clampZoom(zoom, doc);
  const d = displaySize(doc, z);
  return { zoom: z, panX: (viewport.width - d.width) / 2, panY: (viewport.height - d.height) / 2 };
}

/** Pan so that `svgPoint` lands in the centre of the viewport. */
export function centerOn(view: View, svgPoint: Point, viewport: Size, doc: DocGeometry): View {
  const d = displaySize(doc, view.zoom);
  const fx = (svgPoint.x - doc.box.minX) / doc.box.width;
  const fy = (svgPoint.y - doc.box.minY) / doc.box.height;
  return { zoom: view.zoom, panX: viewport.width / 2 - fx * d.width, panY: viewport.height / 2 - fy * d.height };
}

/** Keep at least `margin` px of the document on screen (or all of it when smaller). */
export function clampPan(view: View, viewport: Size, doc: DocGeometry, margin = 64): View {
  const d = displaySize(doc, view.zoom);
  const mx = Math.min(margin, d.width);
  const my = Math.min(margin, d.height);
  const panX = Math.min(viewport.width - mx, Math.max(mx - d.width, view.panX));
  const panY = Math.min(viewport.height - my, Math.max(my - d.height, view.panY));
  return { zoom: view.zoom, panX, panY };
}

/** Keep the same document point at the viewport centre after the viewport resizes. */
export function preserveCenterOnResize(view: View, oldViewport: Size, newViewport: Size): View {
  return {
    zoom: view.zoom,
    panX: view.panX + (newViewport.width - oldViewport.width) / 2,
    panY: view.panY + (newViewport.height - oldViewport.height) / 2,
  };
}

// ---------------------------------------------------------------------------
// Regions (SVG user units)
// ---------------------------------------------------------------------------

export type Handle = "n" | "s" | "e" | "w" | "ne" | "nw" | "se" | "sw";
export const HANDLES: Handle[] = ["nw", "n", "ne", "e", "se", "s", "sw", "w"];

export function normalizeRegion(a: Point, b: Point): Region {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(b.x - a.x),
    height: Math.abs(b.y - a.y),
  };
}

/** Intersection of a region with the document box (null if empty). */
export function clampRegion(r: Region, box: ViewBox): Region | null {
  const x0 = Math.max(r.x, box.minX);
  const y0 = Math.max(r.y, box.minY);
  const x1 = Math.min(r.x + r.width, box.minX + box.width);
  const y1 = Math.min(r.y + r.height, box.minY + box.height);
  if (x1 <= x0 || y1 <= y0) return null;
  return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 };
}

function clampPoint(p: Point, box: ViewBox): Point {
  return {
    x: Math.min(box.minX + box.width, Math.max(box.minX, p.x)),
    y: Math.min(box.minY + box.height, Math.max(box.minY, p.y)),
  };
}

/** Region from a drag between two SVG points, clamped to the document. */
export function regionFromDrag(a: Point, b: Point, box: ViewBox): Region {
  return normalizeRegion(clampPoint(a, box), clampPoint(b, box));
}

/**
 * Resize `start` by dragging `handle` to SVG point `p`. Edges that cross over
 * flip naturally (the result is always normalized). Clamped to the document.
 */
export function resizeRegion(start: Region, handle: Handle, p: Point, box: ViewBox): Region {
  const q = clampPoint(p, box);
  let x0 = start.x;
  let y0 = start.y;
  let x1 = start.x + start.width;
  let y1 = start.y + start.height;
  if (handle.includes("w")) x0 = q.x;
  if (handle.includes("e")) x1 = q.x;
  if (handle.includes("n")) y0 = q.y;
  if (handle.includes("s")) y1 = q.y;
  return normalizeRegion({ x: x0, y: y0 }, { x: x1, y: y1 });
}

/** Move a region by (dx, dy) SVG units, keeping it fully inside the document. */
export function moveRegion(start: Region, dx: number, dy: number, box: ViewBox): Region {
  const maxX = box.minX + box.width - start.width;
  const maxY = box.minY + box.height - start.height;
  return {
    x: Math.min(Math.max(box.minX, start.x + dx), Math.max(box.minX, maxX)),
    y: Math.min(Math.max(box.minY, start.y + dy), Math.max(box.minY, maxY)),
    width: start.width,
    height: start.height,
  };
}

/** Screen positions of the 8 handles of a screen rect. */
export function handlePositions(r: Rect): Record<Handle, Point> {
  const cx = r.x + r.width / 2;
  const cy = r.y + r.height / 2;
  const x1 = r.x + r.width;
  const y1 = r.y + r.height;
  return {
    nw: { x: r.x, y: r.y },
    n: { x: cx, y: r.y },
    ne: { x: x1, y: r.y },
    e: { x: x1, y: cy },
    se: { x: x1, y: y1 },
    s: { x: cx, y: y1 },
    sw: { x: r.x, y: y1 },
    w: { x: r.x, y: cy },
  };
}

export function handleCursor(h: Handle): string {
  switch (h) {
    case "n":
    case "s":
      return "ns-resize";
    case "e":
    case "w":
      return "ew-resize";
    case "ne":
    case "sw":
      return "nesw-resize";
    default:
      return "nwse-resize";
  }
}

/** Region is big enough on screen to be intentional (not a stray click). */
export function isMeaningfulDrag(a: Point, b: Point, minPx = 4): boolean {
  return Math.abs(a.x - b.x) >= minPx || Math.abs(a.y - b.y) >= minPx;
}

/** Human-friendly SVG unit number: integers when large, up to 2 decimals when small. */
export function formatUnits(n: number): string {
  const a = Math.abs(n);
  if (a >= 100) return Math.round(n).toLocaleString("en-US");
  if (a >= 10) return (Math.round(n * 10) / 10).toString();
  return (Math.round(n * 100) / 100).toString();
}

export function regionLabel(r: Region): string {
  return `${formatUnits(r.width)} × ${formatUnits(r.height)} SVG units`;
}

/** Round region values to a sensible precision before sending to the backend. */
export function roundRegion(r: Region, decimals = 3): Region {
  const f = 10 ** decimals;
  const q = (v: number) => Math.round(v * f) / f;
  return { x: q(r.x), y: q(r.y), width: q(r.width), height: q(r.height) };
}

// ---------------------------------------------------------------------------
// Minimap
// ---------------------------------------------------------------------------

export interface MinimapLayout extends Size {
  scale: number; // minimap px per 100 %-zoom px
}

export function minimapLayout(doc: DocGeometry, maxWidth: number, maxHeight: number): MinimapLayout {
  const scale = Math.min(maxWidth / doc.baseWidth, maxHeight / doc.baseHeight);
  return { width: doc.baseWidth * scale, height: doc.baseHeight * scale, scale };
}

export function shouldShowMinimap(view: View, viewport: Size, doc: DocGeometry): boolean {
  const d = displaySize(doc, view.zoom);
  return d.width > viewport.width + 1 || d.height > viewport.height + 1;
}

/** The visible part of the document, in minimap px (clamped to the minimap). */
export function minimapViewportRect(view: View, viewport: Size, doc: DocGeometry, mm: Size): Rect {
  const d = displaySize(doc, view.zoom);
  const fx0 = Math.max(0, Math.min(1, -view.panX / d.width));
  const fy0 = Math.max(0, Math.min(1, -view.panY / d.height));
  const fx1 = Math.max(0, Math.min(1, (viewport.width - view.panX) / d.width));
  const fy1 = Math.max(0, Math.min(1, (viewport.height - view.panY) / d.height));
  return { x: fx0 * mm.width, y: fy0 * mm.height, width: (fx1 - fx0) * mm.width, height: (fy1 - fy0) * mm.height };
}

/** View that centres the main viewport on minimap point `p` (minimap px). */
export function minimapPointToView(p: Point, view: View, viewport: Size, doc: DocGeometry, mm: Size): View {
  const d = displaySize(doc, view.zoom);
  const fx = Math.max(0, Math.min(1, p.x / mm.width));
  const fy = Math.max(0, Math.min(1, p.y / mm.height));
  return { zoom: view.zoom, panX: viewport.width / 2 - fx * d.width, panY: viewport.height / 2 - fy * d.height };
}
