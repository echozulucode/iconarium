import { GripVertical } from "lucide-react";
import { useRef } from "react";
import type { Region } from "../api/types";
import { HANDLES, handleCursor, handlePositions, regionLabel, type Rect, type Size } from "./geometry";

interface Props {
  /** Region in screen px (canvas-relative). */
  rect: Rect;
  region: Region;
  viewport: Size;
  active: boolean;
  onGripDrag(): void;
}

const HANDLE = 9;
const GRIP_THRESHOLD = 5;

/** Region outline, dimmed surround, 8 resize handles, live dimensions and a drag-out grip. */
export function SelectionOverlay({ rect, region, viewport, active, onGripDrag }: Props) {
  const r = rect;
  const handles = handlePositions(r);
  const grip = useRef<{ x: number; y: number; fired: boolean } | null>(null);

  // Label above the region when there is room, otherwise inside the top edge.
  const labelAbove = r.y > 30;
  const labelTop = labelAbove ? r.y - 28 : r.y + 6;
  const labelLeft = Math.max(4, Math.min(r.x, viewport.width - 200));
  const gripLeft = Math.min(viewport.width - 28, r.x + r.width + 6);
  const gripTop = Math.max(4, r.y);

  const W = viewport.width;
  const H = viewport.height;

  return (
    <div className="pointer-events-none absolute inset-0 overflow-hidden">
      <svg width={W} height={H} className="absolute inset-0" aria-hidden>
        <path
          d={`M0 0H${W}V${H}H0Z M${r.x} ${r.y}V${r.y + r.height}H${r.x + r.width}V${r.y}Z`}
          fill="var(--region-dim)"
          fillRule="evenodd"
        />
        <rect x={r.x - 0.5} y={r.y - 0.5} width={r.width + 1} height={r.height + 1} fill="none" stroke="#ffffff" strokeOpacity="0.9" strokeWidth={3} />
        <rect x={r.x} y={r.y} width={r.width} height={r.height} fill="none" stroke="var(--accent)" strokeWidth={1.5} />
      </svg>

      {/* Move target */}
      <div
        data-region-body=""
        className="pointer-events-auto absolute"
        style={{ left: r.x, top: r.y, width: Math.max(0, r.width), height: Math.max(0, r.height), cursor: "move" }}
      />

      {HANDLES.map((h) => (
        <div
          key={h}
          data-handle={h}
          className="pointer-events-auto absolute rounded-[2px] border-[1.5px] border-[var(--accent)] bg-white shadow-[0_1px_2px_rgba(0,0,0,0.25)]"
          style={{
            left: handles[h].x - HANDLE / 2,
            top: handles[h].y - HANDLE / 2,
            width: HANDLE,
            height: HANDLE,
            cursor: handleCursor(h),
          }}
        />
      ))}

      <div
        className={`absolute rounded-[6px] bg-[var(--accent)] px-2 py-[3px] text-[11.5px] font-semibold whitespace-nowrap text-[var(--accent-fg)] shadow-[0_2px_8px_rgba(0,0,0,0.25)] tnum transition-opacity duration-150 ${active ? "opacity-100" : "opacity-95"}`}
        style={{ left: labelLeft, top: labelTop }}
      >
        {regionLabel(region)}
      </div>

      <button
        type="button"
        data-grip=""
        title="Drag the selection into another app"
        aria-label="Drag selection"
        className="pointer-events-auto absolute flex h-[26px] w-[22px] cursor-grab items-center justify-center rounded-[6px] border border-line bg-elevated text-fg-muted shadow-[0_2px_8px_rgba(0,0,0,0.2)] transition-colors hover:text-accent"
        style={{ left: gripLeft, top: gripTop }}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          e.stopPropagation();
          grip.current = { x: e.clientX, y: e.clientY, fired: false };
          (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
        }}
        onPointerMove={(e) => {
          const g = grip.current;
          if (!g || g.fired) return;
          if (Math.hypot(e.clientX - g.x, e.clientY - g.y) > GRIP_THRESHOLD) {
            g.fired = true;
            (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
            onGripDrag();
          }
        }}
        onPointerUp={() => {
          grip.current = null;
        }}
      >
        <GripVertical size={14} />
      </button>
    </div>
  );
}
