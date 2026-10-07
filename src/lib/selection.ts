// Pure multi-selection logic for the gallery (plan §18). Independent of the DOM:
// operates on asset IDs and the current ordered result list.

export interface SelectionState {
  selected: ReadonlySet<number>;
  /** Fixed end of Shift ranges. */
  anchor: number | null;
  /** Keyboard cursor / last interacted item. */
  focus: number | null;
}

export interface ResultIndex {
  readonly length: number;
  idAt(index: number): number;
  /** -1 when the id is not in the current results. */
  indexOf(id: number): number;
}

export const EMPTY_SELECTION: SelectionState = { selected: new Set(), anchor: null, focus: null };

export function createResultIndex(ids: ArrayLike<number>): ResultIndex {
  let map: Map<number, number> | null = null;
  const build = () => {
    map = new Map();
    for (let i = 0; i < ids.length; i++) map.set(ids[i], i);
    return map;
  };
  return {
    length: ids.length,
    idAt: (i) => ids[i],
    indexOf: (id) => {
      const m = map ?? build();
      const v = m.get(id);
      return v === undefined ? -1 : v;
    },
  };
}

export function rangeIds(ix: ResultIndex, a: number, b: number): number[] {
  const lo = Math.max(0, Math.min(a, b));
  const hi = Math.min(ix.length - 1, Math.max(a, b));
  const out: number[] = [];
  for (let i = lo; i <= hi; i++) out.push(ix.idAt(i));
  return out;
}

export interface ClickModifiers {
  ctrl: boolean;
  shift: boolean;
}

/** Click / Ctrl+Click / Shift+Click / Ctrl+Shift+Click on the item `id`. */
export function clickSelect(state: SelectionState, ix: ResultIndex, id: number, mods: ClickModifiers): SelectionState {
  const target = ix.indexOf(id);
  if (target < 0) return state;

  if (mods.shift) {
    const anchorIdx = state.anchor != null ? ix.indexOf(state.anchor) : -1;
    if (anchorIdx < 0) {
      return { selected: new Set([id]), anchor: id, focus: id };
    }
    const range = rangeIds(ix, anchorIdx, target);
    const selected = mods.ctrl ? new Set(state.selected) : new Set<number>();
    for (const r of range) selected.add(r);
    return { selected, anchor: state.anchor, focus: id };
  }

  if (mods.ctrl) {
    const selected = new Set(state.selected);
    if (selected.has(id)) selected.delete(id);
    else selected.add(id);
    return { selected, anchor: id, focus: id };
  }

  return { selected: new Set([id]), anchor: id, focus: id };
}

/** Selection behaviour for a right-click: keep the selection if the item is in it, else select only it. */
export function contextSelect(state: SelectionState, id: number): SelectionState {
  if (state.selected.has(id)) return { ...state, focus: id };
  return { selected: new Set([id]), anchor: id, focus: id };
}

export function selectAll(ix: ResultIndex, state: SelectionState): SelectionState {
  const selected = new Set<number>();
  for (let i = 0; i < ix.length; i++) selected.add(ix.idAt(i));
  const focus = state.focus != null && ix.indexOf(state.focus) >= 0 ? state.focus : ix.length ? ix.idAt(0) : null;
  return { selected, anchor: state.anchor ?? focus, focus };
}

export function clearSelection(state: SelectionState): SelectionState {
  return { selected: new Set(), anchor: state.focus, focus: state.focus };
}

export type MoveKey = "left" | "right" | "up" | "down" | "home" | "end" | "pageup" | "pagedown";

export function moveTarget(current: number, key: MoveKey, length: number, columns: number, pageRows: number): number {
  if (length === 0) return -1;
  if (current < 0) return key === "end" ? length - 1 : 0;
  const cols = Math.max(1, columns);
  let next = current;
  switch (key) {
    case "left":
      next = current - 1;
      break;
    case "right":
      next = current + 1;
      break;
    case "up":
      next = current - cols;
      if (next < 0) next = current; // stay on the top row
      break;
    case "down":
      next = current + cols;
      if (next >= length) {
        // move to the last item if there is a (partial) row below
        const lastRow = Math.floor((length - 1) / cols);
        next = Math.floor(current / cols) < lastRow ? length - 1 : current;
      }
      break;
    case "home":
      next = 0;
      break;
    case "end":
      next = length - 1;
      break;
    case "pageup":
      next = Math.max(current - cols * Math.max(1, pageRows), current % cols);
      break;
    case "pagedown": {
      next = current + cols * Math.max(1, pageRows);
      if (next >= length) next = length - 1;
      break;
    }
  }
  return Math.max(0, Math.min(length - 1, next));
}

export interface MoveModifiers {
  shift: boolean;
  /** Ctrl+arrow moves the cursor without changing the selection. */
  ctrl: boolean;
}

export function moveSelection(
  state: SelectionState,
  ix: ResultIndex,
  key: MoveKey,
  columns: number,
  pageRows: number,
  mods: MoveModifiers,
): SelectionState {
  const cur = state.focus != null ? ix.indexOf(state.focus) : -1;
  const nextIdx = moveTarget(cur, key, ix.length, columns, pageRows);
  if (nextIdx < 0) return state;
  const id = ix.idAt(nextIdx);
  if (mods.ctrl && !mods.shift) return { ...state, focus: id };
  if (mods.shift) {
    let anchor = state.anchor;
    let anchorIdx = anchor != null ? ix.indexOf(anchor) : -1;
    if (anchorIdx < 0) {
      anchor = cur >= 0 ? ix.idAt(cur) : id;
      anchorIdx = cur >= 0 ? cur : nextIdx;
    }
    return { selected: new Set(rangeIds(ix, anchorIdx, nextIdx)), anchor, focus: id };
  }
  return { selected: new Set([id]), anchor: id, focus: id };
}

/** Toggle the focused item (Ctrl+Space). */
export function toggleFocused(state: SelectionState): SelectionState {
  if (state.focus == null) return state;
  const selected = new Set(state.selected);
  if (selected.has(state.focus)) selected.delete(state.focus);
  else selected.add(state.focus);
  return { selected, anchor: state.focus, focus: state.focus };
}

/** Drop IDs that are no longer in the results (after a re-search). Returns the same object when unchanged. */
export function pruneSelection(state: SelectionState, ix: ResultIndex): SelectionState {
  let changed = false;
  const selected = new Set<number>();
  for (const id of state.selected) {
    if (ix.indexOf(id) >= 0) selected.add(id);
    else changed = true;
  }
  const anchor = state.anchor != null && ix.indexOf(state.anchor) >= 0 ? state.anchor : null;
  const focus = state.focus != null && ix.indexOf(state.focus) >= 0 ? state.focus : null;
  if (!changed && anchor === state.anchor && focus === state.focus) return state;
  return { selected, anchor, focus };
}

/** Selected IDs in current result order (stable for commands that care about order). */
export function orderedSelection(state: SelectionState, ix: ResultIndex): number[] {
  const withIdx: [number, number][] = [];
  for (const id of state.selected) {
    const i = ix.indexOf(id);
    if (i >= 0) withIdx.push([i, id]);
  }
  withIdx.sort((a, b) => a[0] - b[0]);
  return withIdx.map((p) => p[1]);
}

/** IDs to drag when the pointer goes down on `id`. */
export function dragIds(state: SelectionState, ix: ResultIndex, id: number): number[] {
  if (state.selected.has(id)) return orderedSelection(state, ix);
  return [id];
}
