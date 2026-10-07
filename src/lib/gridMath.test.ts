import { describe, expect, it } from "vitest";
import { GRID_GAP, GRID_PADDING, computeGrid, pageRows, rowCount, rowOfIndex, rowsToIndexRange } from "./gridMath";

describe("computeGrid", () => {
  it("derives columns from the container width", () => {
    const g = computeGrid(1280, "medium", false);
    const inner = 1280 - GRID_PADDING * 2;
    expect(g.columns).toBe(Math.floor((inner + GRID_GAP) / (148 + GRID_GAP)));
    expect(g.cardWidth * g.columns + GRID_GAP * (g.columns - 1)).toBeLessThanOrEqual(inner);
    expect(g.cardWidth).toBeGreaterThanOrEqual(148);
  });

  it("small cards produce more columns than large", () => {
    const s = computeGrid(1200, "small", false);
    const l = computeGrid(1200, "large", false);
    expect(s.columns).toBeGreaterThan(l.columns);
  });

  it("never returns zero columns", () => {
    expect(computeGrid(10, "large", false).columns).toBe(1);
    expect(computeGrid(0, "small", false).columns).toBe(1);
  });

  it("secondary line increases row height", () => {
    expect(computeGrid(800, "medium", true).rowHeight).toBeGreaterThan(computeGrid(800, "medium", false).rowHeight);
  });
});

describe("row helpers", () => {
  it("counts rows", () => {
    expect(rowCount(0, 5)).toBe(0);
    expect(rowCount(1, 5)).toBe(1);
    expect(rowCount(10, 5)).toBe(2);
    expect(rowCount(11, 5)).toBe(3);
  });
  it("maps index to row", () => {
    expect(rowOfIndex(0, 4)).toBe(0);
    expect(rowOfIndex(7, 4)).toBe(1);
    expect(rowOfIndex(8, 4)).toBe(2);
  });
  it("maps row range to index range", () => {
    expect(rowsToIndexRange(0, 1, 4, 10)).toEqual([0, 8]);
    expect(rowsToIndexRange(2, 5, 4, 10)).toEqual([8, 10]);
    expect(rowsToIndexRange(0, 0, 4, 0)).toEqual([0, 0]);
  });
  it("page rows", () => {
    expect(pageRows(800, 200)).toBe(3);
    expect(pageRows(100, 200)).toBe(1);
  });
});
