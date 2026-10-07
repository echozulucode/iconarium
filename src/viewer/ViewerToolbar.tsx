import { Copy, Hand, Maximize, Minus, Plus, SquareDashed, X } from "lucide-react";
import type { ViewerBackground } from "../api/types";
import { modKey } from "../lib/format";
import type { ViewerMode } from "../stores/viewerStore";

interface Props {
  filename: string;
  relDir: string;
  mode: ViewerMode;
  onMode(m: ViewerMode): void;
  zoom: number | null;
  onZoomIn(): void;
  onZoomOut(): void;
  onFit(): void;
  onActual(): void;
  background: ViewerBackground;
  onBackground(b: ViewerBackground): void;
  hasRegion: boolean;
  onCopy(): void;
  onClose(): void;
  disabled: boolean;
}

export function ViewerToolbar(p: Props) {
  return (
    <div className="flex h-[52px] shrink-0 items-center gap-3 border-b border-line bg-surface px-3">
      <div className="flex min-w-0 flex-1 items-baseline gap-2 pl-1">
        <h2 className="truncate text-[13.5px] font-semibold text-fg">{p.filename}</h2>
        {p.relDir && <span className="hidden truncate text-[12px] text-fg-subtle md:inline">{p.relDir}</span>}
      </div>

      <Segmented>
        <SegButton active={p.mode === "pan"} onClick={() => p.onMode("pan")} title="Pan (H) — hold Space to pan in any mode" disabled={p.disabled}>
          <Hand size={14} /> Pan
        </SegButton>
        <SegButton active={p.mode === "select"} onClick={() => p.onMode("select")} title="Select region (S)" disabled={p.disabled}>
          <SquareDashed size={14} /> Select
        </SegButton>
      </Segmented>

      <div className="flex items-center gap-0.5">
        <IconButton onClick={p.onZoomOut} title="Zoom out (−)" disabled={p.disabled}>
          <Minus size={15} />
        </IconButton>
        <button
          type="button"
          onClick={p.onActual}
          disabled={p.disabled}
          title="Actual size (0)"
          className="focus-ring h-7 w-[58px] rounded-[7px] text-center text-[12.5px] font-medium text-fg tnum transition-colors hover:bg-surface-2"
        >
          {p.zoom != null ? `${formatZoom(p.zoom)}%` : "–"}
        </button>
        <IconButton onClick={p.onZoomIn} title="Zoom in (+)" disabled={p.disabled}>
          <Plus size={15} />
        </IconButton>
      </div>

      <div className="flex items-center gap-1">
        <TextButton onClick={p.onFit} title="Fit to window (F)" disabled={p.disabled}>
          <Maximize size={13} /> Fit
        </TextButton>
        <TextButton onClick={p.onActual} title="Actual size (0)" disabled={p.disabled}>
          100%
        </TextButton>
      </div>

      <BackgroundPicker value={p.background} onChange={p.onBackground} />

      <span className="h-5 w-px bg-line" />

      <button
        type="button"
        onClick={p.onCopy}
        disabled={p.disabled}
        title={`${p.hasRegion ? "Copy selected region as SVG" : "Copy SVG"} (${modKey}+C)`}
        className="focus-ring flex h-8 items-center gap-1.5 rounded-[8px] bg-accent px-3 text-[12.5px] font-semibold text-accent-fg transition-colors hover:bg-accent-hover disabled:opacity-50"
      >
        <Copy size={14} />
        {p.hasRegion ? "Copy Selection" : "Copy SVG"}
      </button>

      <IconButton onClick={p.onClose} title="Close (Esc)">
        <X size={16} />
      </IconButton>
    </div>
  );
}

export function formatZoom(z: number): string {
  const pct = z * 100;
  if (pct >= 10) return Math.round(pct).toLocaleString("en-US");
  return pct.toFixed(1);
}

function Segmented({ children }: { children: React.ReactNode }) {
  return <div className="flex h-[30px] items-center rounded-[8px] bg-surface-2 p-[3px]">{children}</div>;
}

function SegButton({ active, onClick, title, children, disabled }: { active: boolean; onClick(): void; title: string; children: React.ReactNode; disabled?: boolean }) {
  return (
    <button
      type="button"
      aria-pressed={active}
      onClick={onClick}
      title={title}
      disabled={disabled}
      className={[
        "focus-ring flex h-6 items-center gap-1.5 rounded-[6px] px-2.5 text-[12.5px] font-medium transition-[background-color,color,box-shadow] duration-150",
        active ? "bg-surface text-fg shadow-[0_0_0_1px_var(--border),0_1px_2px_rgba(0,0,0,0.06)]" : "text-fg-muted hover:text-fg",
      ].join(" ")}
    >
      {children}
    </button>
  );
}

function IconButton({ onClick, title, children, disabled }: { onClick(): void; title: string; children: React.ReactNode; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      aria-label={title}
      disabled={disabled}
      className="focus-ring flex h-7 w-7 items-center justify-center rounded-[7px] text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg disabled:opacity-40"
    >
      {children}
    </button>
  );
}

function TextButton({ onClick, title, children, disabled }: { onClick(): void; title: string; children: React.ReactNode; disabled?: boolean }) {
  return (
    <button
      type="button"
      onClick={onClick}
      title={title}
      disabled={disabled}
      className="focus-ring flex h-7 items-center gap-1.5 rounded-[7px] px-2 text-[12.5px] font-medium text-fg-muted transition-colors hover:bg-surface-2 hover:text-fg disabled:opacity-40"
    >
      {children}
    </button>
  );
}

const BG_OPTIONS: { id: ViewerBackground; title: string }[] = [
  { id: "checkerboard", title: "Transparent (checkerboard)" },
  { id: "white", title: "White" },
  { id: "dark", title: "Dark" },
];

function BackgroundPicker({ value, onChange }: { value: ViewerBackground; onChange(b: ViewerBackground): void }) {
  return (
    <div role="radiogroup" aria-label="Viewer background" className="flex items-center gap-1">
      {BG_OPTIONS.map((o) => (
        <button
          key={o.id}
          type="button"
          role="radio"
          aria-checked={value === o.id}
          title={`Background: ${o.title}`}
          onClick={() => onChange(o.id)}
          className={[
            "focus-ring h-[22px] w-[22px] rounded-full border transition-shadow duration-150",
            o.id === "checkerboard" ? "checkerboard" : "",
            value === o.id ? "border-transparent shadow-[0_0_0_2px_var(--surface),0_0_0_3.5px_var(--accent)]" : "border-line-strong",
          ].join(" ")}
          style={{
            background: o.id === "white" ? "#ffffff" : o.id === "dark" ? "var(--viewer-dark)" : undefined,
            backgroundSize: o.id === "checkerboard" ? "8px 8px" : undefined,
          }}
        />
      ))}
    </div>
  );
}
