import { useVirtualizer } from "@tanstack/react-virtual";
import { useCallback, useEffect, useMemo, useRef } from "react";
import { backend } from "../app/backendRef";
import { gridRef } from "../app/focus";
import { useElementSize } from "../hooks/useElementSize";
import { useGalleryKeyboard } from "../hooks/useGalleryKeyboard";
import { computeGrid, pageRows as computePageRows, rowCount, rowOfIndex, rowsToIndexRange } from "../lib/gridMath";
import { useLibrary } from "../stores/libraryStore";
import { useSearch } from "../stores/searchStore";
import { useSelection } from "../stores/selectionStore";
import { summaryCache } from "../stores/summaryCache";
import { useUi } from "../stores/uiStore";
import { AssetCard } from "./AssetCard";

const OVERSCAN_ROWS = 3;
/** Extra rows (beyond overscan) whose summaries are prefetched. */
const PREFETCH_ROWS = 6;
const VIEWPORT_DEBOUNCE_MS = 120;

export function AssetGrid() {
  const scrollRef = useRef<HTMLDivElement>(null);
  const { width, height } = useElementSize(scrollRef);
  const results = useSearch((s) => s.results);
  const version = useSearch((s) => s.version);
  const queryVersion = useSearch((s) => s.queryVersion);
  const hasQuery = useSearch((s) => s.resultsQuery.trim().length > 0);
  const size = useLibrary((s) => s.settings?.gallerySize ?? "medium");
  const showRelDir = useLibrary((s) => s.settings?.showRelativeDir ?? true);
  const secondaryLine = showRelDir || hasQuery;
  const layout = useMemo(() => computeGrid(width || 800, size, secondaryLine), [width, size, secondaryLine]);
  const rows = rowCount(results.length, layout.columns);

  const virtualizer = useVirtualizer({
    count: rows,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => layout.rowHeight,
    overscan: OVERSCAN_ROWS,
    paddingStart: layout.padding,
    paddingEnd: layout.padding + 48,
  });

  useEffect(() => {
    virtualizer.measure();
  }, [layout.rowHeight, virtualizer]);

  // New query → back to top. Catalog refreshes keep the scroll position.
  useEffect(() => {
    scrollRef.current?.scrollTo({ top: 0 });
  }, [queryVersion]);

  const items = virtualizer.getVirtualItems();
  const firstRow = items.length ? items[0].index : 0;
  const lastRow = items.length ? items[items.length - 1].index : -1;

  // Fetch summaries for mounted rows + a prefetch margin. Re-run when entries go stale.
  const ensureRange = useCallback(() => {
    const ids = useSearch.getState().results;
    const [s, e] = rowsToIndexRange(Math.max(0, firstRow - PREFETCH_ROWS), lastRow + PREFETCH_ROWS, layout.columns, ids.length);
    if (e > s) summaryCache.ensure(ids.subarray(s, e));
  }, [firstRow, lastRow, layout.columns]);

  useEffect(() => {
    ensureRange();
  }, [ensureRange, version]);
  useEffect(() => summaryCache.onStale(() => queueMicrotask(ensureRange)), [ensureRange]);

  // Tell the backend what is visible (P0) and nearby (P1), debounced.
  const scrollTop = virtualizer.scrollOffset ?? 0;
  useEffect(() => {
    const t = setTimeout(() => {
      const ids = useSearch.getState().results;
      if (ids.length === 0) return;
      const rh = layout.rowHeight;
      const vFirst = Math.max(0, Math.floor((scrollTop - layout.padding) / rh));
      const vLast = Math.max(vFirst, Math.floor((scrollTop + height - layout.padding) / rh));
      const span = vLast - vFirst + 1;
      const [vs, ve] = rowsToIndexRange(vFirst, vLast, layout.columns, ids.length);
      const [ns, ne] = rowsToIndexRange(Math.max(0, vFirst - span), vLast + span, layout.columns, ids.length);
      const visible = Array.from(ids.subarray(vs, ve));
      const nearby = [...ids.subarray(ns, vs), ...ids.subarray(ve, ne)];
      backend()
        .setViewport(visible, nearby)
        .catch(() => {});
    }, VIEWPORT_DEBOUNCE_MS);
    return () => clearTimeout(t);
  }, [scrollTop, height, layout, version]);

  // Keyboard focus → scroll into view.
  const revealTick = useSelection((s) => s.revealTick);
  useEffect(() => {
    if (revealTick === 0) return;
    const focus = useSelection.getState().focus;
    if (focus == null) return;
    const idx = useSearch.getState().index.indexOf(focus);
    if (idx >= 0) virtualizer.scrollToIndex(rowOfIndex(idx, layout.columns), { align: "auto" });
  }, [revealTick, layout.columns, virtualizer]);

  const openMenuForFocus = useCallback(() => {
    const sel = useSelection.getState();
    const id = sel.focus ?? sel.ordered()[0];
    if (id == null) return;
    if (!sel.selected.has(id)) sel.context(id);
    const el = scrollRef.current?.querySelector<HTMLElement>(`[data-asset-id="${id}"]`);
    const r = el?.getBoundingClientRect();
    useUi.getState().openAssetMenu({
      x: r ? r.left + r.width / 2 : 200,
      y: r ? r.top + r.height / 2 : 200,
      ids: useSelection.getState().ordered(),
      primary: id,
    });
  }, []);

  useGalleryKeyboard({ columns: layout.columns, pageRows: computePageRows(height, layout.rowHeight), openMenuForFocus });

  const setRefs = useCallback((el: HTMLDivElement | null) => {
    scrollRef.current = el;
    gridRef.current = el;
  }, []);

  const onBackgroundClick = (e: React.MouseEvent) => {
    if (e.target === e.currentTarget || (e.target as HTMLElement).dataset.gridRow !== undefined) {
      if (!e.ctrlKey && !e.shiftKey) useSelection.getState().clear();
    }
  };

  return (
    <div
      ref={setRefs}
      role="grid"
      aria-label="SVG assets"
      aria-rowcount={rows}
      aria-multiselectable
      tabIndex={0}
      onClick={onBackgroundClick}
      onContextMenu={(e) => e.preventDefault()}
      className="scroll-quiet relative h-full overflow-y-auto overflow-x-hidden outline-none"
      style={{ contain: "strict" }}
    >
      <div style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }} data-grid-row="">
        {items.map((row) => {
          const start = row.index * layout.columns;
          const end = Math.min(results.length, start + layout.columns);
          const cells = [];
          for (let i = start; i < end; i++) {
            const id = results[i];
            cells.push(
              <AssetCard key={id} id={id} layout={layout} size={size} showRelDir={showRelDir} secondaryLine={secondaryLine} />,
            );
          }
          return (
            <div
              key={row.key}
              role="row"
              aria-rowindex={row.index + 1}
              data-grid-row=""
              className="absolute left-0 flex"
              style={{
                top: 0,
                transform: `translateY(${row.start}px)`,
                height: layout.cardHeight,
                paddingLeft: layout.padding,
                gap: layout.gap,
                width: "100%",
              }}
            >
              {cells}
            </div>
          );
        })}
      </div>
    </div>
  );
}
