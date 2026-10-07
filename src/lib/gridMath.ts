// Pure layout math for the virtualized gallery grid.
import type { GallerySize } from "../api/types";

export interface CardPreset {
  /** Minimum card width; columns are derived from it. */
  minWidth: number;
  /** Inner padding around the thumbnail inside its tile. */
  thumbPadding: number;
  /** Filename font size (px). */
  fontSize: number;
}

export const CARD_PRESETS: Record<GallerySize, CardPreset> = {
  small: { minWidth: 104, thumbPadding: 12, fontSize: 11.5 },
  medium: { minWidth: 148, thumbPadding: 18, fontSize: 12 },
  large: { minWidth: 220, thumbPadding: 26, fontSize: 12.5 },
};

export const GRID_PADDING = 16;
export const GRID_GAP = 10;
export const LABEL_LINE_PRIMARY = 18;
export const LABEL_LINE_SECONDARY = 16;
export const LABEL_PADDING = 10;

export interface GridLayout {
  columns: number;
  cardWidth: number;
  thumbHeight: number;
  labelHeight: number;
  rowHeight: number; // card height + gap
  cardHeight: number;
  gap: number;
  padding: number;
}

export function computeGrid(containerWidth: number, size: GallerySize, secondaryLine: boolean): GridLayout {
  const preset = CARD_PRESETS[size];
  const padding = GRID_PADDING;
  const gap = GRID_GAP;
  const inner = Math.max(0, containerWidth - padding * 2);
  const columns = Math.max(1, Math.floor((inner + gap) / (preset.minWidth + gap)));
  const cardWidth = Math.max(40, Math.floor((inner - gap * (columns - 1)) / columns));
  const thumbHeight = Math.round(cardWidth * 0.78);
  const labelHeight = LABEL_PADDING + LABEL_LINE_PRIMARY + (secondaryLine ? LABEL_LINE_SECONDARY : 0) + 4;
  const cardHeight = thumbHeight + labelHeight;
  return { columns, cardWidth, thumbHeight, labelHeight, cardHeight, rowHeight: cardHeight + gap, gap, padding };
}

export function rowCount(total: number, columns: number): number {
  if (total <= 0) return 0;
  return Math.ceil(total / Math.max(1, columns));
}

export function rowOfIndex(index: number, columns: number): number {
  return Math.floor(index / Math.max(1, columns));
}

/** Item index range [start, end) covered by rows [firstRow, lastRow] inclusive. */
export function rowsToIndexRange(firstRow: number, lastRow: number, columns: number, total: number): [number, number] {
  if (total <= 0 || lastRow < firstRow) return [0, 0];
  const start = Math.max(0, firstRow * columns);
  const end = Math.min(total, (lastRow + 1) * columns);
  return [Math.min(start, end), end];
}

/** Number of full rows visible in a viewport (used for PageUp/PageDown). */
export function pageRows(viewportHeight: number, rowHeight: number): number {
  return Math.max(1, Math.floor(viewportHeight / Math.max(1, rowHeight)) - 1);
}
