import { create } from "zustand";
import { normalizeError } from "../api/errors";
import { backend } from "../app/backendRef";
import { createResultIndex, type ResultIndex } from "../lib/selection";
import { useSelection } from "./selectionStore";
import { summaryCache } from "./summaryCache";
import { toast } from "./uiStore";

interface SearchState {
  /** Text in the search box. */
  query: string;
  /** Query that produced `results` (used for getAssets match explanations). */
  resultsQuery: string;
  results: Uint32Array;
  index: ResultIndex;
  /** Bumped on every applied result set. */
  version: number;
  /** Bumped only when results came from a different query (grid scrolls to top). */
  queryVersion: number;
  searching: boolean;
  error: string | null;
  lastMs: number | null;
  /** Whether at least one search has completed for the current library. */
  loaded: boolean;

  setQuery(q: string): void;
  /** Run the current query immediately. */
  runNow(): Promise<void>;
  /** Throttled re-run (catalog changed) that preserves scroll and selection. */
  refresh(): void;
  reset(): void;
}

const DEBOUNCE_MS = 150;
const REFRESH_THROTTLE_MS = 350;

let seq = 0;
let debounceTimer: ReturnType<typeof setTimeout> | null = null;
let refreshTimer: ReturnType<typeof setTimeout> | null = null;
let lastRefresh = 0;

const EMPTY = new Uint32Array(0);

export const useSearch = create<SearchState>((set, get) => ({
  query: "",
  resultsQuery: "",
  results: EMPTY,
  index: createResultIndex(EMPTY),
  version: 0,
  queryVersion: 0,
  searching: false,
  error: null,
  lastMs: null,
  loaded: false,

  setQuery(q) {
    set({ query: q });
    if (debounceTimer) clearTimeout(debounceTimer);
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      void get().runNow();
    }, DEBOUNCE_MS);
  },

  async runNow() {
    if (debounceTimer) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
    const my = ++seq;
    const q = get().query;
    set({ searching: true });
    const t0 = performance.now();
    try {
      const ids = await backend().search(q);
      if (my !== seq) return;
      const prevQuery = get().resultsQuery;
      const index = createResultIndex(ids);
      const queryChanged = prevQuery.trim() !== q.trim();
      summaryCache.invalidateAll(q);
      set((s) => ({
        results: ids,
        index,
        resultsQuery: q,
        version: s.version + 1,
        queryVersion: queryChanged ? s.queryVersion + 1 : s.queryVersion,
        searching: false,
        error: null,
        lastMs: performance.now() - t0,
        loaded: true,
      }));
      useSelection.getState().prune(index);
    } catch (e) {
      if (my !== seq) return;
      const err = normalizeError(e);
      if (err.kind === "invalid_query" || /regular expression|regex|glob|query/i.test(err.message)) {
        set({ searching: false, error: err.message });
      } else {
        set({ searching: false });
        toast.error("Search failed", err.message);
      }
    }
  },

  refresh() {
    if (refreshTimer) return;
    const wait = Math.max(0, REFRESH_THROTTLE_MS - (performance.now() - lastRefresh));
    refreshTimer = setTimeout(() => {
      refreshTimer = null;
      lastRefresh = performance.now();
      // A pending debounced query run will pick up the latest catalog anyway.
      if (!debounceTimer) void get().runNow();
    }, wait);
  },

  reset() {
    seq++;
    if (debounceTimer) clearTimeout(debounceTimer);
    if (refreshTimer) clearTimeout(refreshTimer);
    debounceTimer = refreshTimer = null;
    summaryCache.clear();
    set({
      results: EMPTY,
      index: createResultIndex(EMPTY),
      resultsQuery: "",
      version: get().version + 1,
      queryVersion: get().queryVersion + 1,
      error: null,
      loaded: false,
      searching: false,
    });
  },
}));
