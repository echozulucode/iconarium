import { LoaderCircle, TriangleAlert } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import type { Region, ViewerBackground } from "../api/types";
import { backend } from "../app/backendRef";
import { copyAssets, dragRegion, viewerCopy } from "../app/commands";
import { focusGrid, isTypingTarget } from "../app/focus";
import type { MenuEntry } from "../components/Menu";
import { useElementSize } from "../hooks/useElementSize";
import { formatBytes, formatDimension } from "../lib/format";
import { useLibrary } from "../stores/libraryStore";
import { useUi } from "../stores/uiStore";
import { useViewer } from "../stores/viewerStore";
import {
  clampPan,
  displaySize,
  docGeometry,
  fitView,
  formatUnits,
  isMeaningfulDrag,
  moveRegion,
  nextZoomStep,
  preserveCenterOnResize,
  regionFromDrag,
  regionToScreen,
  resizeRegion,
  screenToSvg,
  shouldShowMinimap,
  wheelZoomFactor,
  zoomAt,
  type Handle,
  type Point,
  type View,
} from "./geometry";
import { MiniMap } from "./MiniMap";
import { SelectionOverlay } from "./SelectionOverlay";
import { ViewerContextMenu, documentMenuItems, regionMenuItems } from "./ViewerContextMenu";
import { ViewerToolbar } from "./ViewerToolbar";

type Drag =
  | { kind: "pan"; start: Point; startView: View; pointerId: number }
  | { kind: "create"; start: Point; startSvg: Point; moved: boolean; pointerId: number }
  | { kind: "resize"; handle: Handle; startRegion: Region; pointerId: number }
  | { kind: "move"; start: Point; startRegion: Region; pointerId: number };

const FIT_PADDING = 40;
const PAN_MARGIN = 48;

export function SvgViewer() {
  const openId = useViewer((s) => s.openId);
  if (openId == null) return null;
  return <ViewerOverlay key={openId} id={openId} />;
}

function ViewerOverlay({ id }: { id: number }) {
  const detail = useViewer((s) => s.detail);
  const loading = useViewer((s) => s.loading);
  const loadError = useViewer((s) => s.error);
  const mode = useViewer((s) => s.mode);
  const region = useViewer((s) => s.region);
  const bgOverride = useViewer((s) => s.background);
  const settingsBg = useLibrary((s) => s.settings?.viewerBackground ?? "checkerboard");
  const background: ViewerBackground = bgOverride ?? settingsBg;

  const canvasRef = useRef<HTMLDivElement>(null);
  const vp = useElementSize(canvasRef);
  const doc = useMemo(() => (detail ? docGeometry(detail.width, detail.height, detail.docBox) : null), [detail]);
  const [view, setView] = useState<View | null>(null);
  const viewRef = useRef<View | null>(null);
  viewRef.current = view;
  const autoFit = useRef(true);
  const prevVp = useRef(vp);
  const drag = useRef<Drag | null>(null);
  const [dragKind, setDragKind] = useState<Drag["kind"] | null>(null);
  const [space, setSpace] = useState(false);
  const spaceRef = useRef(false);
  const [imgState, setImgState] = useState<"loading" | "ready" | "error">("loading");
  const [menu, setMenu] = useState<{ at: { x: number; y: number }; items: MenuEntry[] } | null>(null);

  const previewable = detail ? detail.state === "ready" || detail.state === "discovered" : false;
  const src = detail && previewable ? backend().svgUrl(detail.id, detail.fingerprint) : null;

  // Fit on open and while the user hasn't navigated; otherwise keep the centre on resize.
  useLayoutEffect(() => {
    if (!doc || vp.width === 0 || vp.height === 0) return;
    setView((v) => {
      if (!v || autoFit.current) return fitView(doc, vp, FIT_PADDING);
      return clampPan(preserveCenterOnResize(v, prevVp.current, vp), vp, doc, PAN_MARGIN);
    });
    prevVp.current = vp;
  }, [doc, vp]);

  const apply = useCallback(
    (v: View) => {
      if (!doc) return;
      autoFit.current = false;
      setView(clampPan(v, vp, doc, PAN_MARGIN));
    },
    [doc, vp],
  );

  const center = useMemo(() => ({ x: vp.width / 2, y: vp.height / 2 }), [vp]);
  const zoomStep = useCallback(
    (dir: 1 | -1) => {
      const v = viewRef.current;
      if (v && doc) apply(zoomAt(v, nextZoomStep(v.zoom, dir), center, doc));
    },
    [apply, center, doc],
  );
  const fit = useCallback(() => {
    if (!doc) return;
    autoFit.current = true;
    setView(fitView(doc, vp, FIT_PADDING));
  }, [doc, vp]);
  const actual = useCallback(() => {
    const v = viewRef.current;
    if (v && doc) apply(zoomAt(v, 1, center, doc));
  }, [apply, center, doc]);
  const selectAllRegion = useCallback(() => {
    if (!doc) return;
    const b = doc.box;
    useViewer.getState().setRegion({ x: b.minX, y: b.minY, width: b.width, height: b.height });
  }, [doc]);

  const close = useCallback(() => {
    useViewer.getState().close();
    requestAnimationFrame(() => focusGrid());
  }, []);

  // Wheel zoom around the pointer (native listener so preventDefault works).
  useEffect(() => {
    const el = canvasRef.current;
    if (!el) return;
    const onWheel = (e: WheelEvent) => {
      e.preventDefault();
      const v = viewRef.current;
      if (!v || !doc) return;
      const b = el.getBoundingClientRect();
      const p = { x: e.clientX - b.left, y: e.clientY - b.top };
      apply(zoomAt(v, v.zoom * wheelZoomFactor(e.deltaY, e.deltaMode), p, doc));
    };
    el.addEventListener("wheel", onWheel, { passive: false });
    return () => el.removeEventListener("wheel", onWheel);
  }, [apply, doc]);

  // Keyboard model (plan §37) while the viewer is open.
  useEffect(() => {
    const onDown = (e: KeyboardEvent) => {
      const ui = useUi.getState();
      if (ui.popovers > 0 || ui.propertiesId !== null || isTypingTarget(e.target)) return;
      const ctrl = e.ctrlKey || e.metaKey;
      const vs = useViewer.getState();
      if (e.key === " ") {
        e.preventDefault();
        if (!spaceRef.current) {
          spaceRef.current = true;
          setSpace(true);
        }
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        if (vs.region) vs.setRegion(null);
        else close();
        return;
      }
      if (ctrl && e.key.toLowerCase() === "c") {
        e.preventDefault();
        if (e.shiftKey) void copyAssets([id], "path");
        else viewerCopy();
        return;
      }
      if (ctrl && e.key.toLowerCase() === "a") {
        e.preventDefault();
        selectAllRegion();
        return;
      }
      if (ctrl || e.altKey) return;
      switch (e.key) {
        case "+":
        case "=":
          e.preventDefault();
          zoomStep(1);
          break;
        case "-":
        case "_":
          e.preventDefault();
          zoomStep(-1);
          break;
        case "0":
          e.preventDefault();
          actual();
          break;
        case "f":
        case "F":
          e.preventDefault();
          fit();
          break;
        case "h":
        case "H":
          vs.setMode("pan");
          break;
        case "s":
        case "S":
          vs.setMode("select");
          break;
        case "ArrowLeft":
        case "ArrowRight":
        case "ArrowUp":
        case "ArrowDown": {
          e.preventDefault();
          const v = viewRef.current;
          if (!v) break;
          const step = e.shiftKey ? 200 : 48;
          const dx = e.key === "ArrowLeft" ? step : e.key === "ArrowRight" ? -step : 0;
          const dy = e.key === "ArrowUp" ? step : e.key === "ArrowDown" ? -step : 0;
          apply({ ...v, panX: v.panX + dx, panY: v.panY + dy });
          break;
        }
      }
    };
    const onUp = (e: KeyboardEvent) => {
      if (e.key === " ") {
        spaceRef.current = false;
        setSpace(false);
      }
    };
    const onBlur = () => {
      spaceRef.current = false;
      setSpace(false);
    };
    window.addEventListener("keydown", onDown);
    window.addEventListener("keyup", onUp);
    window.addEventListener("blur", onBlur);
    return () => {
      window.removeEventListener("keydown", onDown);
      window.removeEventListener("keyup", onUp);
      window.removeEventListener("blur", onBlur);
    };
  }, [id, apply, zoomStep, actual, fit, close, selectAllRegion]);

  const local = (e: { clientX: number; clientY: number }): Point => {
    const b = canvasRef.current!.getBoundingClientRect();
    return { x: e.clientX - b.left, y: e.clientY - b.top };
  };

  const onPointerDown = (e: React.PointerEvent<HTMLDivElement>) => {
    const v = viewRef.current;
    if (!v || !doc || !previewable) return;
    if (e.button !== 0 && e.button !== 1) return;
    const target = e.target as HTMLElement;
    if (target.closest("[data-grip]")) return;
    const p = local(e);
    const regionNow = useViewer.getState().region;
    let next: Drag;
    const handle = target.closest<HTMLElement>("[data-handle]")?.dataset.handle as Handle | undefined;
    if (e.button === 1 || spaceRef.current) {
      next = { kind: "pan", start: p, startView: v, pointerId: e.pointerId };
    } else if (handle && regionNow) {
      next = { kind: "resize", handle, startRegion: regionNow, pointerId: e.pointerId };
    } else if (target.closest("[data-region-body]") && regionNow) {
      next = { kind: "move", start: p, startRegion: regionNow, pointerId: e.pointerId };
    } else if (mode === "select") {
      next = { kind: "create", start: p, startSvg: screenToSvg(p, v, doc), moved: false, pointerId: e.pointerId };
    } else {
      next = { kind: "pan", start: p, startView: v, pointerId: e.pointerId };
    }
    e.preventDefault();
    drag.current = next;
    setDragKind(next.kind);
    e.currentTarget.setPointerCapture(e.pointerId);
  };

  const onPointerMove = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    const v = viewRef.current;
    if (!d || !v || !doc || e.pointerId !== d.pointerId) return;
    const p = local(e);
    const vs = useViewer.getState();
    switch (d.kind) {
      case "pan":
        apply({ zoom: d.startView.zoom, panX: d.startView.panX + (p.x - d.start.x), panY: d.startView.panY + (p.y - d.start.y) });
        break;
      case "create":
        if (!d.moved && !isMeaningfulDrag(d.start, p, 3)) return;
        d.moved = true;
        vs.setRegion(regionFromDrag(d.startSvg, screenToSvg(p, v, doc), doc.box));
        break;
      case "resize":
        vs.setRegion(resizeRegion(d.startRegion, d.handle, screenToSvg(p, v, doc), doc.box));
        break;
      case "move": {
        const ds = displaySize(doc, v.zoom);
        const dx = ((p.x - d.start.x) / ds.width) * doc.box.width;
        const dy = ((p.y - d.start.y) / ds.height) * doc.box.height;
        vs.setRegion(moveRegion(d.startRegion, dx, dy, doc.box));
        break;
      }
    }
  };

  const endDrag = (e: React.PointerEvent<HTMLDivElement>) => {
    const d = drag.current;
    if (!d || e.pointerId !== d.pointerId) return;
    const vs = useViewer.getState();
    if (d.kind === "create" && !d.moved) vs.setRegion(null);
    if (vs.region && (vs.region.width <= 0 || vs.region.height <= 0)) vs.setRegion(null);
    drag.current = null;
    setDragKind(null);
    if (e.currentTarget.hasPointerCapture(e.pointerId)) e.currentTarget.releasePointerCapture(e.pointerId);
  };

  const onContextMenu = (e: React.MouseEvent) => {
    e.preventDefault();
    const v = viewRef.current;
    if (!v || !doc || !previewable) return;
    const at = { x: e.clientX, y: e.clientY };
    const r = useViewer.getState().region;
    if (r) {
      const sp = screenToSvg(local(e), v, doc);
      const inside = sp.x >= r.x && sp.x <= r.x + r.width && sp.y >= r.y && sp.y <= r.y + r.height;
      if (inside) {
        setMenu({ at, items: regionMenuItems(id, r) });
        return;
      }
    }
    setMenu({ at, items: documentMenuItems(id, { fit, actual, selectAll: selectAllRegion }) });
  };

  const ds = view && doc ? displaySize(doc, view.zoom) : null;
  const screenRegion = region && view && doc ? regionToScreen(region, view, doc) : null;
  const showMinimap = !!(view && doc && src && shouldShowMinimap(view, vp, doc));

  const cursor =
    dragKind === "pan"
      ? "grabbing"
      : dragKind === "move"
        ? "move"
        : space || mode === "pan"
          ? "grab"
          : "crosshair";

  const filename = detail?.filename ?? "";
  const relDir = detail ? detail.relativePath.slice(0, Math.max(0, detail.relativePath.length - detail.filename.length - 1)) : "";

  return (
    <div role="dialog" aria-modal="true" aria-label={`Viewer: ${filename}`} className="fade-in fixed inset-0 z-50 flex flex-col bg-bg">
      <ViewerToolbar
        filename={filename || "Loading…"}
        relDir={relDir}
        mode={mode}
        onMode={(m) => useViewer.getState().setMode(m)}
        zoom={view?.zoom ?? null}
        onZoomIn={() => zoomStep(1)}
        onZoomOut={() => zoomStep(-1)}
        onFit={fit}
        onActual={actual}
        background={background}
        onBackground={(b) => useViewer.getState().setBackground(b)}
        hasRegion={!!region}
        onCopy={viewerCopy}
        onClose={close}
        disabled={!previewable}
      />

      <div
        ref={canvasRef}
        className="relative min-h-0 flex-1 touch-none overflow-hidden bg-surface-2 select-none"
        style={{ cursor }}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={endDrag}
        onPointerCancel={endDrag}
        onContextMenu={onContextMenu}
        onDoubleClick={(e) => {
          const v = viewRef.current;
          if (!v || !doc || mode !== "pan" || (e.target as HTMLElement).closest("[data-region-body],[data-handle]")) return;
          apply(zoomAt(v, v.zoom * 2, local(e), doc));
        }}
      >
        {view && ds && src && (
          <>
            <div
              className={`absolute ${background === "checkerboard" ? "checkerboard" : ""}`}
              style={{
                left: 0,
                top: 0,
                width: ds.width,
                height: ds.height,
                transform: `translate(${view.panX}px, ${view.panY}px)`,
                background: background === "white" ? "#ffffff" : background === "dark" ? "var(--viewer-dark)" : undefined,
                boxShadow: "0 0 0 1px var(--border), 0 8px 24px -12px rgba(0,0,0,0.25)",
              }}
            />
            <img
              src={src}
              alt={filename}
              draggable={false}
              onLoad={() => setImgState("ready")}
              onError={() => setImgState("error")}
              className="pointer-events-none absolute max-w-none transition-opacity duration-150"
              style={{
                left: 0,
                top: 0,
                width: ds.width,
                height: ds.height,
                transform: `translate(${view.panX}px, ${view.panY}px)`,
                opacity: imgState === "ready" ? 1 : 0,
              }}
            />
          </>
        )}

        {screenRegion && region && (
          <SelectionOverlay
            rect={screenRegion}
            region={region}
            viewport={vp}
            active={dragKind === "create" || dragKind === "resize" || dragKind === "move"}
            onGripDrag={() => void dragRegion(id, region)}
          />
        )}

        {(loading || (src && imgState === "loading")) && !loadError && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center">
            <LoaderCircle size={22} className="spin text-fg-subtle" />
          </div>
        )}
        {(loadError || imgState === "error" || (detail && !previewable)) && (
          <ViewerMessage
            title={detail && !previewable ? "Preview unavailable" : "Couldn't display this SVG"}
            detail={
              loadError ??
              (detail?.state === "limit_exceeded"
                ? "This file exceeds the rendering limit. You can still copy its path or open it in another app."
                : detail?.parseError ?? "The file could not be rendered.")
            }
          />
        )}

        {showMinimap && view && doc && src && (
          <MiniMap src={src} doc={doc} view={view} viewport={vp} region={region} background={background} onView={apply} />
        )}
      </div>

      <ViewerFooter />

      {menu && <ViewerContextMenu at={menu.at} items={menu.items} onClose={() => setMenu(null)} />}
    </div>
  );
}

function ViewerMessage({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="absolute inset-0 flex items-center justify-center p-8">
      <div className="max-w-[380px] text-center">
        <TriangleAlert size={24} strokeWidth={1.6} className="mx-auto text-warning" />
        <div className="mt-2 text-[14px] font-semibold text-fg">{title}</div>
        <div className="mt-1 text-[12.5px] text-fg-muted">{detail}</div>
      </div>
    </div>
  );
}

function ViewerFooter() {
  const d = useViewer((s) => s.detail);
  const region = useViewer((s) => s.region);
  const vb = d?.viewBox;
  const warnings: string[] = [];
  if (d?.sizeWarning) warnings.push(`Large file (${formatBytes(d.fileSize)})`);
  if (d && !vb && d.state === "ready") warnings.push("No viewBox; using width and height");
  if (d?.state === "discovered") warnings.push("Metadata not read yet");

  return (
    <footer className="flex h-8 shrink-0 items-center gap-5 border-t border-line bg-surface px-4 text-[12px] text-fg-muted tnum">
      {d ? (
        <>
          <span className="text-fg">
            {d.width != null && d.height != null ? `${formatDimension(d.width)} × ${formatDimension(d.height)}` : "Unknown size"}
          </span>
          <span>{formatBytes(d.fileSize)}</span>
          <span className="min-w-0 truncate" title={d.absolutePath}>
            {d.relativePath}
          </span>
          {vb && <span className="hidden shrink-0 lg:inline">viewBox {`${vb.minX} ${vb.minY} ${vb.width} ${vb.height}`}</span>}
          {warnings.map((w) => (
            <span key={w} className="flex shrink-0 items-center gap-1 text-warning">
              <TriangleAlert size={12} /> {w}
            </span>
          ))}
          <span className="flex-1" />
          {region && (
            <span className="shrink-0 text-fg">
              Selection {formatUnits(region.width)} × {formatUnits(region.height)}
              <span className="text-fg-muted">
                {" "}
                at {formatUnits(region.x)}, {formatUnits(region.y)}
              </span>
            </span>
          )}
        </>
      ) : (
        <span> </span>
      )}
    </footer>
  );
}
