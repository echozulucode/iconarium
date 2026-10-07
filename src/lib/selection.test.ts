import { describe, expect, it } from "vitest";
import {
  EMPTY_SELECTION,
  clearSelection,
  clickSelect,
  contextSelect,
  createResultIndex,
  dragIds,
  moveSelection,
  moveTarget,
  orderedSelection,
  pruneSelection,
  selectAll,
  toggleFocused,
} from "./selection";

const ids = Uint32Array.from([10, 11, 12, 13, 14, 15, 16, 17, 18, 19]);
const ix = createResultIndex(ids);
const none = { ctrl: false, shift: false };
const sorted = (s: ReadonlySet<number>) => [...s].sort((a, b) => a - b);

describe("clickSelect", () => {
  it("plain click selects one and sets anchor/focus", () => {
    const s = clickSelect(EMPTY_SELECTION, ix, 12, none);
    expect(sorted(s.selected)).toEqual([12]);
    expect(s.anchor).toBe(12);
    expect(s.focus).toBe(12);
  });

  it("ctrl+click toggles", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 12, none);
    s = clickSelect(s, ix, 15, { ctrl: true, shift: false });
    expect(sorted(s.selected)).toEqual([12, 15]);
    s = clickSelect(s, ix, 12, { ctrl: true, shift: false });
    expect(sorted(s.selected)).toEqual([15]);
    expect(s.anchor).toBe(12);
  });

  it("shift+click selects range from anchor (both directions)", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 13, none);
    s = clickSelect(s, ix, 16, { ctrl: false, shift: true });
    expect(sorted(s.selected)).toEqual([13, 14, 15, 16]);
    s = clickSelect(s, ix, 11, { ctrl: false, shift: true });
    expect(sorted(s.selected)).toEqual([11, 12, 13]);
    expect(s.anchor).toBe(13);
    expect(s.focus).toBe(11);
  });

  it("ctrl+shift+click adds range to existing selection", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 10, none);
    s = clickSelect(s, ix, 17, { ctrl: true, shift: false });
    s = clickSelect(s, ix, 19, { ctrl: true, shift: true });
    expect(sorted(s.selected)).toEqual([10, 17, 18, 19]);
  });

  it("shift+click without anchor behaves like click", () => {
    const s = clickSelect(EMPTY_SELECTION, ix, 14, { ctrl: false, shift: true });
    expect(sorted(s.selected)).toEqual([14]);
  });

  it("ignores ids not in results", () => {
    const s = clickSelect(EMPTY_SELECTION, ix, 999, none);
    expect(s).toBe(EMPTY_SELECTION);
  });
});

describe("selectAll / clear / context", () => {
  it("selects all results", () => {
    const s = selectAll(ix, EMPTY_SELECTION);
    expect(s.selected.size).toBe(10);
    expect(s.focus).toBe(10);
  });
  it("clear keeps focus", () => {
    const s = clearSelection(clickSelect(EMPTY_SELECTION, ix, 13, none));
    expect(s.selected.size).toBe(0);
    expect(s.focus).toBe(13);
  });
  it("context click keeps an existing multi-selection", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 10, none);
    s = clickSelect(s, ix, 12, { ctrl: false, shift: true });
    const c = contextSelect(s, 11);
    expect(sorted(c.selected)).toEqual([10, 11, 12]);
    const d = contextSelect(s, 15);
    expect(sorted(d.selected)).toEqual([15]);
  });
});

describe("keyboard movement", () => {
  it("computes targets in a 4-column grid", () => {
    // 10 items, 4 cols: rows [0..3] [4..7] [8,9]
    expect(moveTarget(5, "right", 10, 4, 2)).toBe(6);
    expect(moveTarget(5, "left", 10, 4, 2)).toBe(4);
    expect(moveTarget(5, "up", 10, 4, 2)).toBe(1);
    expect(moveTarget(1, "up", 10, 4, 2)).toBe(1);
    expect(moveTarget(5, "down", 10, 4, 2)).toBe(9); // partial last row → last item
    expect(moveTarget(4, "down", 10, 4, 2)).toBe(8);
    expect(moveTarget(9, "down", 10, 4, 2)).toBe(9);
    expect(moveTarget(9, "right", 10, 4, 2)).toBe(9);
    expect(moveTarget(0, "left", 10, 4, 2)).toBe(0);
    expect(moveTarget(-1, "down", 10, 4, 2)).toBe(0);
    expect(moveTarget(1, "pagedown", 10, 4, 2)).toBe(9);
    expect(moveTarget(9, "pageup", 10, 4, 2)).toBe(1);
    expect(moveTarget(3, "end", 10, 4, 2)).toBe(9);
    expect(moveTarget(3, "home", 10, 4, 2)).toBe(0);
    expect(moveTarget(0, "down", 0, 4, 2)).toBe(-1);
  });

  it("arrow moves single selection; shift extends from anchor", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 11, none);
    s = moveSelection(s, ix, "right", 4, 2, { shift: false, ctrl: false });
    expect(sorted(s.selected)).toEqual([12]);
    s = moveSelection(s, ix, "down", 4, 2, { shift: true, ctrl: false });
    expect(sorted(s.selected)).toEqual([12, 13, 14, 15, 16]);
    expect(s.anchor).toBe(12);
    s = moveSelection(s, ix, "left", 4, 2, { shift: true, ctrl: false });
    expect(sorted(s.selected)).toEqual([12, 13, 14, 15]);
  });

  it("ctrl+arrow moves focus only; ctrl+space toggles", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 11, none);
    s = moveSelection(s, ix, "right", 4, 2, { shift: false, ctrl: true });
    expect(s.focus).toBe(12);
    expect(sorted(s.selected)).toEqual([11]);
    s = toggleFocused(s);
    expect(sorted(s.selected)).toEqual([11, 12]);
  });

  it("first arrow press with no focus selects the first item", () => {
    const s = moveSelection(EMPTY_SELECTION, ix, "down", 4, 2, { shift: false, ctrl: false });
    expect(sorted(s.selected)).toEqual([10]);
  });
});

describe("prune / ordering / drag", () => {
  it("drops ids missing from new results and keeps identity when unchanged", () => {
    let s = clickSelect(EMPTY_SELECTION, ix, 10, none);
    s = clickSelect(s, ix, 13, { ctrl: false, shift: true });
    const same = pruneSelection(s, ix);
    expect(same).toBe(s);
    const next = createResultIndex(Uint32Array.from([13, 12, 50]));
    const p = pruneSelection(s, next);
    expect(sorted(p.selected)).toEqual([12, 13]);
    expect(p.anchor).toBeNull();
    expect(p.focus).toBe(13);
  });

  it("orders the selection by result order", () => {
    const rev = createResultIndex(Uint32Array.from([19, 18, 17, 16]));
    const s = { selected: new Set([16, 18, 19]), anchor: null, focus: null };
    expect(orderedSelection(s, rev)).toEqual([19, 18, 16]);
  });

  it("drags whole selection only when the item is selected", () => {
    const s = { selected: new Set([12, 14]), anchor: 12, focus: 14 };
    expect(dragIds(s, ix, 14)).toEqual([12, 14]);
    expect(dragIds(s, ix, 15)).toEqual([15]);
  });
});
