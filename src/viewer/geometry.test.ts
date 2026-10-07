import { describe, expect, it } from "vitest";
import {
  MAX_DISPLAY_PX,
  centerOn,
  clampPan,
  clampRegion,
  clampZoom,
  displaySize,
  docGeometry,
  fitView,
  formatUnits,
  handlePositions,
  minimapLayout,
  minimapPointToView,
  minimapViewportRect,
  moveRegion,
  nextZoomStep,
  regionFromDrag,
  regionLabel,
  regionToScreen,
  resizeRegion,
  screenToSvg,
  shouldShowMinimap,
  svgToScreen,
  wheelZoomFactor,
  zoomAt,
  zoomLimits,
} from "./geometry";

const close = (a: number, b: number, eps = 1e-6) => expect(Math.abs(a - b)).toBeLessThan(eps);

describe("docGeometry", () => {
  it("uses docBox and intrinsic width", () => {
    const g = docGeometry(1200, 800, { minX: 0, minY: 0, width: 2400, height: 1600 });
    expect(g.baseWidth).toBe(1200);
    expect(g.baseHeight).toBe(800);
  });
  it("falls back to width/height then 100x100", () => {
    expect(docGeometry(50, 20, null).box).toEqual({ minX: 0, minY: 0, width: 50, height: 20 });
    expect(docGeometry(null, null, null).box.width).toBe(100);
  });
  it("keeps the box aspect even if width/height disagree", () => {
    const g = docGeometry(100, 100, { minX: 0, minY: 0, width: 200, height: 100 });
    expect(g.baseHeight).toBe(50);
  });
});

describe("screen <-> svg mapping", () => {
  const doc = docGeometry(1000, 500, { minX: -500, minY: -250, width: 2000, height: 1000 });
  const view = { zoom: 2, panX: 40, panY: 30 };

  it("maps the image origin to the docBox origin", () => {
    const p = screenToSvg({ x: 40, y: 30 }, view, doc);
    close(p.x, -500);
    close(p.y, -250);
  });

  it("follows svgX = minX + (sx - imgLeft)/displayW * width", () => {
    const d = displaySize(doc, view.zoom); // 2000 x 1000
    const sx = 40 + d.width / 4;
    const p = screenToSvg({ x: sx, y: 30 + d.height / 2 }, view, doc);
    close(p.x, -500 + 2000 / 4);
    close(p.y, -250 + 500);
  });

  it("round-trips", () => {
    const s = { x: 123.4, y: 567.8 };
    const back = svgToScreen(screenToSvg(s, view, doc), view, doc);
    close(back.x, s.x);
    close(back.y, s.y);
  });

  it("region stays fixed in svg space under zoom", () => {
    const r = { x: 0, y: 0, width: 100, height: 50 };
    const a = regionToScreen(r, view, doc);
    const v2 = zoomAt(view, 4, { x: 300, y: 200 }, doc);
    const b = regionToScreen(r, v2, doc);
    close(b.width, a.width * 2);
    // mapping back yields the same svg region
    const tl = screenToSvg({ x: b.x, y: b.y }, v2, doc);
    close(tl.x, 0);
    close(tl.y, 0);
  });
});

describe("zoom", () => {
  const doc = docGeometry(800, 600, null);
  it("zoomAt keeps the anchor point fixed", () => {
    const v = { zoom: 1, panX: 10, panY: 20 };
    const anchor = { x: 300, y: 250 };
    const before = screenToSvg(anchor, v, doc);
    const v2 = zoomAt(v, 3.7, anchor, doc);
    const after = screenToSvg(anchor, v2, doc);
    close(before.x, after.x);
    close(before.y, after.y);
  });

  it("limits zoom and display size", () => {
    const big = docGeometry(5000, 3000, null);
    const { max } = zoomLimits(big);
    expect(displaySize(big, max).width).toBeLessThanOrEqual(MAX_DISPLAY_PX + 1e-6);
    expect(clampZoom(1000, doc)).toBeLessThanOrEqual(64);
    expect(clampZoom(0, doc)).toBeGreaterThan(0);
    const icon = docGeometry(24, 24, null);
    expect(zoomLimits(icon).max).toBe(64);
  });

  it("steps through presets", () => {
    expect(nextZoomStep(1, 1)).toBe(1.25);
    expect(nextZoomStep(1, -1)).toBe(0.75);
    expect(nextZoomStep(1.1, 1)).toBe(1.25);
    expect(nextZoomStep(64, 1)).toBe(64);
    expect(nextZoomStep(0.01, -1)).toBe(0.01);
  });

  it("wheel factor is symmetric and bounded", () => {
    close(wheelZoomFactor(100) * wheelZoomFactor(-100), 1);
    expect(wheelZoomFactor(-100)).toBeGreaterThan(1);
    expect(wheelZoomFactor(1e6)).toBeGreaterThan(0.5);
    expect(wheelZoomFactor(3, 1)).toBeCloseTo(wheelZoomFactor(48), 10);
  });

  it("fit centres the document inside padding", () => {
    const v = fitView(doc, { width: 1000, height: 1000 }, 50);
    const d = displaySize(doc, v.zoom);
    close(d.width, 900);
    close(v.panX, 50);
    close(v.panY, (1000 - d.height) / 2);
  });

  it("centerOn puts the svg point at the viewport centre", () => {
    const v = centerOn({ zoom: 2, panX: 0, panY: 0 }, { x: 400, y: 300 }, { width: 600, height: 400 }, doc);
    const c = screenToSvg({ x: 300, y: 200 }, v, doc);
    close(c.x, 400);
    close(c.y, 300);
  });

  it("clampPan keeps some of the document visible", () => {
    const v = clampPan({ zoom: 1, panX: 5000, panY: -5000 }, { width: 500, height: 500 }, doc, 64);
    expect(v.panX).toBe(500 - 64);
    expect(v.panY).toBe(64 - 600);
  });
});

describe("regions", () => {
  const box = { minX: 0, minY: 0, width: 1000, height: 800 };
  it("normalizes drags in any direction and clamps to the document", () => {
    expect(regionFromDrag({ x: 300, y: 400 }, { x: 100, y: 100 }, box)).toEqual({ x: 100, y: 100, width: 200, height: 300 });
    expect(regionFromDrag({ x: -50, y: 700 }, { x: 200, y: 900 }, box)).toEqual({ x: 0, y: 700, width: 200, height: 100 });
  });

  it("resizes with each handle and flips past the opposite edge", () => {
    const r = { x: 100, y: 100, width: 200, height: 100 };
    expect(resizeRegion(r, "se", { x: 400, y: 300 }, box)).toEqual({ x: 100, y: 100, width: 300, height: 200 });
    expect(resizeRegion(r, "n", { x: 999, y: 50 }, box)).toEqual({ x: 100, y: 50, width: 200, height: 150 });
    expect(resizeRegion(r, "w", { x: 350, y: 0 }, box)).toEqual({ x: 300, y: 100, width: 50, height: 100 });
    expect(resizeRegion(r, "nw", { x: -10, y: -10 }, box)).toEqual({ x: 0, y: 0, width: 300, height: 200 });
  });

  it("moves within the document", () => {
    const r = { x: 100, y: 100, width: 200, height: 100 };
    expect(moveRegion(r, 50, 20, box)).toEqual({ x: 150, y: 120, width: 200, height: 100 });
    expect(moveRegion(r, 5000, -5000, box)).toEqual({ x: 800, y: 0, width: 200, height: 100 });
  });

  it("clamps/intersects regions", () => {
    expect(clampRegion({ x: -10, y: -10, width: 50, height: 50 }, box)).toEqual({ x: 0, y: 0, width: 40, height: 40 });
    expect(clampRegion({ x: 2000, y: 0, width: 5, height: 5 }, box)).toBeNull();
  });

  it("labels and handle positions", () => {
    expect(regionLabel({ x: 0, y: 0, width: 850.2, height: 420 })).toBe("850 × 420 SVG units");
    expect(formatUnits(12.345)).toBe("12.3");
    expect(formatUnits(1.234)).toBe("1.23");
    expect(formatUnits(1240)).toBe("1,240");
    const h = handlePositions({ x: 10, y: 20, width: 100, height: 50 });
    expect(h.se).toEqual({ x: 110, y: 70 });
    expect(h.n).toEqual({ x: 60, y: 20 });
  });
});

describe("minimap", () => {
  const doc = docGeometry(2000, 1000, null);
  const mm = minimapLayout(doc, 200, 150);
  it("fits the document into the minimap box", () => {
    close(mm.width, 200);
    close(mm.height, 100);
  });

  it("shows the visible portion", () => {
    const view = { zoom: 1, panX: -500, panY: 0 };
    const vp = { width: 1000, height: 1000 };
    expect(shouldShowMinimap(view, vp, doc)).toBe(true);
    const r = minimapViewportRect(view, vp, doc, mm);
    close(r.x, 50);
    close(r.width, 100);
    close(r.height, 100);
  });

  it("click recentres the main view", () => {
    const view = { zoom: 1, panX: 0, panY: 0 };
    const vp = { width: 800, height: 600 };
    const v = minimapPointToView({ x: 150, y: 50 }, view, vp, doc, mm);
    // minimap (150,50) => doc fraction (0.75, 0.5) => svg (1500, 500)
    const c = screenToSvg({ x: 400, y: 300 }, v, doc);
    close(c.x, 1500);
    close(c.y, 500);
    expect(shouldShowMinimap(fitView(doc, vp), vp, doc)).toBe(false);
  });
});

describe("tiny coordinate systems", () => {
  it("can still be zoomed to a usable display size", () => {
    const tiny = docGeometry(null, null, { minX: 0, minY: 0, width: 1, height: 1 });
    expect(zoomLimits(tiny).max * tiny.baseWidth).toBeGreaterThanOrEqual(1024);
  });
});
