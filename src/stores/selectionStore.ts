import { create } from "zustand";
import {
  EMPTY_SELECTION,
  clearSelection,
  clickSelect,
  contextSelect,
  moveSelection,
  orderedSelection,
  pruneSelection,
  selectAll,
  toggleFocused,
  type ClickModifiers,
  type MoveKey,
  type MoveModifiers,
  type ResultIndex,
  type SelectionState,
} from "../lib/selection";
import { useSearch } from "./searchStore";

interface SelectionStore extends SelectionState {
  /** Bumped when keyboard navigation wants the focused item scrolled into view. */
  revealTick: number;
  click(id: number, mods: ClickModifiers): void;
  context(id: number): void;
  selectAll(): void;
  clear(): void;
  move(key: MoveKey, columns: number, pageRows: number, mods: MoveModifiers): void;
  toggleFocused(): void;
  prune(ix: ResultIndex): void;
  reset(): void;
  ordered(): number[];
}

const ix = () => useSearch.getState().index;
const pick = (s: SelectionState): SelectionState => ({ selected: s.selected, anchor: s.anchor, focus: s.focus });

export const useSelection = create<SelectionStore>((set, get) => ({
  ...EMPTY_SELECTION,
  revealTick: 0,
  click(id, mods) {
    set(pick(clickSelect(get(), ix(), id, mods)));
  },
  context(id) {
    set(pick(contextSelect(get(), id)));
  },
  selectAll() {
    set(pick(selectAll(ix(), get())));
  },
  clear() {
    set(pick(clearSelection(get())));
  },
  move(key, columns, pageRows, mods) {
    const next = moveSelection(get(), ix(), key, columns, pageRows, mods);
    set({ ...pick(next), revealTick: get().revealTick + 1 });
  },
  toggleFocused() {
    set(pick(toggleFocused(get())));
  },
  prune(index) {
    const cur = get();
    const next = pruneSelection(cur, index);
    if (next !== cur) set(pick(next));
  },
  reset() {
    set({ ...EMPTY_SELECTION, selected: new Set() });
  },
  ordered() {
    return orderedSelection(get(), ix());
  },
}));
