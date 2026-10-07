import { useRef } from "react";
import type { Region, ViewerBackground } from "../api/types";
import {
  minimapLayout,
  minimapPointToView,
  minimapViewportRect,
  type DocGeometry,
  type Size,
  type View,
} from "./geometry";

interface Props {
  src: string;
  doc: DocGeometry;
  view: View;
  viewport: Size;
  region: Region | null;
  background: ViewerBackground;
  onView(v: View): void;
}

const MAX_W = 200;
const MAX_H = 140;

/** Overview of the whole document with the visible area; click or drag to navigate. */
export function MiniMap({ src, doc, view, viewport, region, background, onView }: Props) {
  const mm = minimapLayout(doc, MAX_W, MAX_H);
  const vr = minimapViewportRect(view, viewport, doc, mm);
  const ref = useRef<HTMLDivElement>(null);
  const dragging = useRef(false);

  const go = (e: React.PointerEvent) => {
    const el = ref.current;
    if (!el) return;
    const b = el.getBoundingClientRect();
    onView(minimapPointToView({ x: e.clientX - b.left, y: e.clientY - b.top }, view, viewport, doc, mm));
  };

  const rr = region
    ? {
        x: ((region.x - doc.box.minX) / doc.box.width) * mm.width,
        y: ((region.y - doc.box.minY) / doc.box.height) * mm.height,
        w: (region.width / doc.box.width) * mm.width,
        h: (region.height / doc.box.height) * mm.height,
      }
    : null;

  return (
    <div
      className="fade-in absolute right-4 bottom-4 z-10 rounded-[10px] border border-line bg-elevated p-1.5 shadow-[var(--shadow-float)]"
      onPointerDown={(e) => e.stopPropagation()}
      onWheel={(e) => e.stopPropagation()}
    >
      <div
        ref={ref}
        role="navigation"
        aria-label="Minimap"
        className={`relative cursor-pointer overflow-hidden rounded-[6px] ${background === "checkerboard" ? "checkerboard" : ""}`}
        style={{
          width: mm.width,
          height: mm.height,
          background: background === "white" ? "#ffffff" : background === "dark" ? "var(--viewer-dark)" : undefined,
          backgroundSize: background === "checkerboard" ? "10px 10px" : undefined,
        }}
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          dragging.current = true;
          (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
          go(e);
        }}
        onPointerMove={(e) => dragging.current && go(e)}
        onPointerUp={() => (dragging.current = false)}
        onPointerCancel={() => (dragging.current = false)}
      >
        <img src={src} alt="" draggable={false} className="pointer-events-none absolute inset-0 h-full w-full" />
        {rr && (
          <div
            className="pointer-events-none absolute border border-[var(--accent)] bg-[var(--accent-soft)]"
            style={{ left: rr.x, top: rr.y, width: rr.w, height: rr.h }}
          />
        )}
        <div
          className="pointer-events-none absolute rounded-[2px] border-[1.5px] border-[var(--accent)] shadow-[0_0_0_9999px_rgba(0,0,0,0.18)]"
          style={{ left: vr.x, top: vr.y, width: Math.max(4, vr.width), height: Math.max(4, vr.height) }}
        />
      </div>
    </div>
  );
}
