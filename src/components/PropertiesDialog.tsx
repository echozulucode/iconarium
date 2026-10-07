import { Copy, ExternalLink, FolderSearch, LoaderCircle, TriangleAlert, X } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { errorMessage } from "../api/errors";
import type { AssetDetail, ProcessingState } from "../api/types";
import { backend } from "../app/backendRef";
import { copyAssets, openExternal, revealAssets } from "../app/commands";
import { formatBytes, formatCount, formatDate, formatDimension } from "../lib/format";
import { useUi } from "../stores/uiStore";

const STATE_LABEL: Record<ProcessingState, string> = {
  discovered: "Waiting to be read",
  ready: "Ready",
  limit_exceeded: "Exceeds rendering limit",
  parse_error: "Invalid SVG",
  missing: "File missing",
};

export function PropertiesDialog() {
  const id = useUi((s) => s.propertiesId);
  if (id == null) return null;
  return <Dialog key={id} id={id} />;
}

function Dialog({ id }: { id: number }) {
  const close = useUi((s) => s.hideProperties);
  const [detail, setDetail] = useState<AssetDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const panelRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    let alive = true;
    backend()
      .getAssetDetail(id)
      .then((d) => alive && setDetail(d))
      .catch((e) => alive && setError(errorMessage(e)));
    return () => {
      alive = false;
    };
  }, [id]);

  useEffect(() => {
    const prev = document.activeElement;
    useUi.getState().popoverOpened();
    panelRef.current?.focus();
    return () => {
      useUi.getState().popoverClosed();
      if (prev instanceof HTMLElement) prev.focus({ preventScroll: true });
    };
  }, []);

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close();
    }
    if (e.key === "Tab") {
      // Minimal focus trap.
      const f = panelRef.current?.querySelectorAll<HTMLElement>("button, [href], [tabindex]:not([tabindex='-1'])");
      if (!f || f.length === 0) return;
      const first = f[0];
      const last = f[f.length - 1];
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
    }
  };

  const d = detail;
  const vb = d?.viewBox;
  const rows: [string, React.ReactNode][] = d
    ? [
        ["Location", <PathValue key="p" path={d.absolutePath} onCopy={() => void copyAssets([id], "path")} />],
        [
          "Size",
          <span key="s" className="inline-flex items-center gap-1.5">
            {formatBytes(d.fileSize)} <span className="text-fg-subtle">({formatCount(d.fileSize)} bytes)</span>
            {d.sizeWarning && (
              <span className="inline-flex items-center gap-1 text-warning">
                <TriangleAlert size={13} /> large file
              </span>
            )}
          </span>,
        ],
        ["Modified", formatDate(d.mtimeMs)],
        ["Dimensions", d.width != null && d.height != null ? `${formatDimension(d.width)} × ${formatDimension(d.height)}` : "–"],
        ["viewBox", vb ? `${vb.minX} ${vb.minY} ${vb.width} ${vb.height}` : "None"],
        ["Elements", d.elementCount != null ? formatCount(d.elementCount) : "–"],
        [
          "Status",
          <span key="st" className={d.state === "ready" ? "" : d.state === "discovered" ? "text-fg-muted" : "text-danger"}>
            {STATE_LABEL[d.state]}
            {d.parseError && <span className="block text-[12px] text-fg-muted">{d.parseError}</span>}
          </span>,
        ],
        ["Title", d.title || <span className="text-fg-subtle">None</span>],
        ["Description", d.description || <span className="text-fg-subtle">None</span>],
        ["Content hash", <Mono key="h">{d.contentHash ?? "Not computed yet"}</Mono>],
      ]
    : [];

  return createPortal(
    <div
      className="fade-in fixed inset-0 z-[70] flex items-center justify-center bg-[var(--scrim)] p-6"
      onPointerDown={(e) => e.target === e.currentTarget && close()}
    >
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="props-title"
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className="pop-in flex max-h-full w-[560px] max-w-full flex-col overflow-hidden rounded-[12px] border border-line bg-elevated shadow-[var(--shadow-float)] outline-none"
        style={{ transformOrigin: "center" }}
      >
        <div className="flex items-start gap-3.5 border-b border-line px-5 py-4">
          <div className="flex h-14 w-14 shrink-0 items-center justify-center overflow-hidden rounded-[8px] bg-tile">
            {d && (d.state === "ready" || d.state === "discovered") && (
              <img src={backend().thumbnailUrl(d.id, d.fingerprint)} alt="" draggable={false} className="h-full w-full object-contain p-2" />
            )}
          </div>
          <div className="min-w-0 flex-1 pt-1">
            <h2 id="props-title" className="truncate text-[15px] font-semibold text-fg">
              {d?.filename ?? "Properties"}
            </h2>
            <div className="truncate text-[12px] text-fg-muted">{d?.relativePath ?? " "}</div>
          </div>
          <button
            type="button"
            aria-label="Close"
            onClick={close}
            className="focus-ring flex h-7 w-7 items-center justify-center rounded-[7px] text-fg-subtle transition-colors hover:bg-surface-2 hover:text-fg"
          >
            <X size={15} />
          </button>
        </div>
        <div className="scroll-quiet min-h-[200px] overflow-y-auto px-5 py-3 select-text">
          {error ? (
            <p className="py-6 text-[13px] text-danger">{error}</p>
          ) : !d ? (
            <div className="flex justify-center py-10">
              <LoaderCircle size={18} className="spin text-fg-subtle" />
            </div>
          ) : (
            <dl className="grid grid-cols-[112px_1fr] gap-x-4 text-[12.5px]">
              {rows.map(([k, v]) => (
                <div key={k} className="contents">
                  <dt className="border-b border-line py-2 text-fg-muted">{k}</dt>
                  <dd className="min-w-0 border-b border-line py-2 break-words text-fg">{v}</dd>
                </div>
              ))}
            </dl>
          )}
        </div>
        <div className="flex items-center justify-end gap-2 border-t border-line bg-surface px-5 py-3">
          <FooterButton onClick={() => void revealAssets([id])} icon={<FolderSearch size={14} />} label="Reveal in Explorer" />
          <FooterButton onClick={() => void openExternal(id)} icon={<ExternalLink size={14} />} label="Open Externally" />
          <button
            type="button"
            onClick={close}
            className="focus-ring ml-1 h-8 rounded-[7px] bg-accent px-4 text-[12.5px] font-semibold text-accent-fg transition-colors hover:bg-accent-hover"
          >
            Done
          </button>
        </div>
      </div>
    </div>,
    document.body,
  );
}

function Mono({ children }: { children: React.ReactNode }) {
  return <span className="font-mono text-[11.5px] break-all text-fg-muted">{children}</span>;
}

function PathValue({ path, onCopy }: { path: string; onCopy: () => void }) {
  return (
    <span className="flex items-start gap-2">
      <span className="min-w-0 flex-1 break-all">{path}</span>
      <button
        type="button"
        title="Copy full path"
        aria-label="Copy full path"
        onClick={onCopy}
        className="focus-ring -mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-[6px] text-fg-subtle transition-colors hover:bg-surface-2 hover:text-fg"
      >
        <Copy size={13} />
      </button>
    </span>
  );
}

function FooterButton({ onClick, icon, label }: { onClick: () => void; icon: React.ReactNode; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="focus-ring flex h-8 items-center gap-1.5 rounded-[7px] border border-line px-3 text-[12.5px] font-medium text-fg transition-colors hover:bg-surface-2"
    >
      <span className="text-fg-muted">{icon}</span>
      {label}
    </button>
  );
}
