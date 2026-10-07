import { FileWarning, FileX2, Hash, Heading, ImageOff, TextQuote, AlignLeft } from "lucide-react";
import { memo, useEffect, useRef, useState } from "react";
import type { AssetSummary, MatchInfo } from "../api/types";
import { backend } from "../app/backendRef";
import { dragAssets, openViewer } from "../app/commands";
import { focusGrid } from "../app/focus";
import type { GridLayout } from "../lib/gridMath";
import { CARD_PRESETS } from "../lib/gridMath";
import { dragIds } from "../lib/selection";
import { useSearch } from "../stores/searchStore";
import { useSelection } from "../stores/selectionStore";
import { useSummary, useThumbNonce } from "../stores/summaryCache";
import { useUi } from "../stores/uiStore";
import type { GallerySize } from "../api/types";

const DRAG_THRESHOLD = 5;

const MATCH_ICON: Partial<Record<MatchInfo["field"], typeof TextQuote>> = {
  title: Heading,
  text: TextQuote,
  desc: AlignLeft,
  id_class: Hash,
};

const MATCH_LABEL: Partial<Record<MatchInfo["field"], string>> = {
  title: "Matched title",
  text: "Matched text",
  desc: "Matched description",
  id_class: "Matched ID",
};

export function isPreviewable(s: AssetSummary | undefined): boolean {
  return !!s && (s.state === "ready" || s.state === "discovered");
}

interface Props {
  id: number;
  layout: GridLayout;
  size: GallerySize;
  showRelDir: boolean;
  secondaryLine: boolean;
}

export const AssetCard = memo(function AssetCard({ id, layout, size, showRelDir, secondaryLine }: Props) {
  const summary = useSummary(id);
  const selected = useSelection((s) => s.selected.has(id));
  const focused = useSelection((s) => s.focus === id);
  const preset = CARD_PRESETS[size];
  const dragState = useRef<{ x: number; y: number; started: boolean } | null>(null);
  const suppressClick = useRef(false);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    suppressClick.current = false;
    dragState.current = { x: e.clientX, y: e.clientY, started: false };
    const move = (ev: PointerEvent) => {
      const d = dragState.current;
      if (!d || d.started) return;
      if (Math.hypot(ev.clientX - d.x, ev.clientY - d.y) > DRAG_THRESHOLD) {
        d.started = true;
        suppressClick.current = true;
        cleanup();
        const sel = useSelection.getState();
        if (!sel.selected.has(id)) sel.click(id, { ctrl: false, shift: false });
        const ids = dragIds(useSelection.getState(), useSearch.getState().index, id);
        void dragAssets(ids);
      }
    };
    const up = () => cleanup();
    const cleanup = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("blur", up);
      if (dragState.current && !dragState.current.started) dragState.current = null;
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("blur", up);
  };

  const onClick = (e: React.MouseEvent) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    useSelection.getState().click(id, { ctrl: e.ctrlKey || e.metaKey, shift: e.shiftKey });
    focusGrid();
  };

  const onDoubleClick = (e: React.MouseEvent) => {
    if (e.ctrlKey || e.shiftKey || e.metaKey) return;
    if (isPreviewable(summary)) openViewer(id);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const sel = useSelection.getState();
    sel.context(id);
    focusGrid();
    useUi.getState().openAssetMenu({ x: e.clientX, y: e.clientY, ids: useSelection.getState().ordered(), primary: id });
  };

  const match = summary?.matchInfo && MATCH_LABEL[summary.matchInfo.field] ? summary.matchInfo : null;
  const relDir = summary?.relDir ?? "";

  return (
    <div
      role="gridcell"
      aria-selected={selected}
      data-asset-id={id}
      title={summary ? (relDir ? `${relDir}/${summary.filename}` : summary.filename) : undefined}
      onPointerDown={onPointerDown}
      onClick={onClick}
      onDoubleClick={onDoubleClick}
      onContextMenu={onContextMenu}
      onDragStart={(e) => e.preventDefault()}
      className={[
        "group relative flex shrink-0 flex-col rounded-[10px] p-1 transition-[background-color,box-shadow] duration-150",
        selected
          ? "bg-accent-soft shadow-[inset_0_0_0_1.5px_var(--accent)]"
          : focused
            ? "shadow-[inset_0_0_0_1px_var(--border-strong)] hover:bg-surface-2"
            : "hover:bg-surface-2",
      ].join(" ")}
      style={{ width: layout.cardWidth, height: layout.cardHeight }}
    >
      <Thumb id={id} summary={summary} height={layout.thumbHeight - 8} padding={preset.thumbPadding} size={size} />
      <div className="flex min-w-0 flex-1 flex-col justify-center px-1.5 pt-1">
        {summary ? (
          <div className="truncate font-medium leading-[18px] text-fg" style={{ fontSize: preset.fontSize }}>
            {summary.filename}
          </div>
        ) : (
          <div className="skeleton my-[5px] h-2 w-2/3 rounded-full bg-tile" />
        )}
        {secondaryLine &&
          (match ? (
            <MatchLine match={match} />
          ) : showRelDir && summary ? (
            <div className="truncate text-[11px] leading-4 text-fg-subtle">{relDir || "."}</div>
          ) : (
            <div className="h-4" />
          ))}
      </div>
    </div>
  );
});

function MatchLine({ match }: { match: MatchInfo }) {
  const Icon = MATCH_ICON[match.field] ?? TextQuote;
  const label = `${MATCH_LABEL[match.field]}: “${match.snippet}”`;
  return (
    <div className="flex min-w-0 items-center gap-1 text-[11px] leading-4 text-accent" title={label} aria-label={label}>
      <Icon size={11} strokeWidth={2.2} className="shrink-0 opacity-80" />
      <span className="truncate">“{match.snippet}”</span>
    </div>
  );
}

function Thumb({
  id,
  summary,
  height,
  padding,
  size,
}: {
  id: number;
  summary: AssetSummary | undefined;
  height: number;
  padding: number;
  size: GallerySize;
}) {
  const [loadedSrc, setLoadedSrc] = useState<string | null>(null);
  const [failedNonce, setFailedNonce] = useState<number | null>(null);
  const nonce = useThumbNonce(id);
  const small = size === "small";

  const base = summary && isPreviewable(summary) ? backend().thumbnailUrl(id, summary.fingerprint) : null;
  // Only add a cache-busting retry suffix after a failure followed by thumb://ready.
  const src = base && failedNonce !== null && nonce > failedNonce ? `${base}${base.includes("?") ? "&" : "?"}r=${nonce}` : base;
  const loaded = src !== null && loadedSrc === src;
  const failed = failedNonce !== null && !(nonce > failedNonce);

  useEffect(() => {
    if (base === null) setFailedNonce(null);
  }, [base]);

  let overlay: React.ReactNode = null;
  if (summary?.state === "limit_exceeded") {
    overlay = (
      <StateTile icon={<FileWarning size={small ? 18 : 22} strokeWidth={1.6} />} title="Preview unavailable" detail="File exceeds rendering limit" compact={small} tone="warning" />
    );
  } else if (summary?.state === "parse_error") {
    overlay = <StateTile icon={<FileX2 size={small ? 18 : 22} strokeWidth={1.6} />} title="Can't read SVG" detail="The file isn't valid SVG" compact={small} tone="danger" />;
  } else if (summary?.state === "missing") {
    overlay = <StateTile icon={<ImageOff size={small ? 18 : 22} strokeWidth={1.6} />} title="File missing" detail="Moved or deleted" compact={small} tone="muted" />;
  } else if (failed) {
    overlay = <StateTile icon={<ImageOff size={small ? 18 : 22} strokeWidth={1.6} />} title="No thumbnail" detail="Rendering failed" compact={small} tone="muted" />;
  }

  return (
    <div
      className="relative flex shrink-0 items-center justify-center overflow-hidden rounded-[7px] bg-tile transition-colors duration-150 group-hover:bg-tile-hover"
      style={{ height }}
    >
      {!overlay && !loaded && <div className="skeleton absolute inset-[22%] rounded-[6px] bg-tile-hover" />}
      {src && !overlay && (
        <img
          key={src}
          src={src}
          alt=""
          draggable={false}
          loading="lazy"
          decoding="async"
          onLoad={() => setLoadedSrc(src)}
          onError={() => setFailedNonce(nonce)}
          className="pointer-events-none h-full w-full object-contain transition-opacity duration-150 ease-out"
          style={{ padding, opacity: loaded ? 1 : 0 }}
        />
      )}
      {overlay}
    </div>
  );
}

function StateTile({
  icon,
  title,
  detail,
  compact,
  tone,
}: {
  icon: React.ReactNode;
  title: string;
  detail: string;
  compact: boolean;
  tone: "warning" | "danger" | "muted";
}) {
  const color = tone === "warning" ? "text-warning" : tone === "danger" ? "text-danger" : "text-fg-subtle";
  return (
    <div className="flex flex-col items-center gap-1 px-3 text-center">
      <span className={color}>{icon}</span>
      <div className={`font-medium text-fg-muted ${compact ? "text-[10.5px]" : "text-[11.5px]"}`}>{title}</div>
      {!compact && <div className="text-[10.5px] leading-tight text-fg-subtle">{detail}</div>}
    </div>
  );
}
