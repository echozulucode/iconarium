import { FolderSearch, Hand, Link2, X } from "lucide-react";
import { copyAssets, revealAssets } from "../app/commands";
import { formatCount } from "../lib/format";
import { useSelection } from "../stores/selectionStore";

/** Floating bar shown while several assets are selected. */
export function MultiSelectionToolbar() {
  const count = useSelection((s) => s.selected.size);
  if (count < 2) return null;
  const ids = () => useSelection.getState().ordered();
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-4 z-20 flex justify-center">
      <div
        role="toolbar"
        aria-label="Selection actions"
        className="toast-in pointer-events-auto flex h-10 items-center gap-1 rounded-[10px] border border-line bg-elevated pr-1 pl-3 shadow-[var(--shadow-float)]"
      >
        <span className="text-[12.5px] font-semibold text-fg tnum">{formatCount(count)} selected</span>
        <span className="mx-2 flex items-center gap-1.5 text-[12px] text-fg-subtle">
          <Hand size={13} /> drag to copy files
        </span>
        <span className="mx-1 h-5 w-px bg-line" />
        <BarButton onClick={() => void copyAssets(ids(), "path")} icon={<Link2 size={14} />} label="Copy Paths" />
        <BarButton onClick={() => void revealAssets(ids())} icon={<FolderSearch size={14} />} label="Reveal" />
        <button
          type="button"
          aria-label="Clear selection"
          title="Clear selection (Esc)"
          onClick={() => useSelection.getState().clear()}
          className="focus-ring flex h-8 w-8 items-center justify-center rounded-[7px] text-fg-subtle transition-colors hover:bg-surface-2 hover:text-fg"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}

function BarButton({ onClick, icon, label }: { onClick: () => void; icon: React.ReactNode; label: string }) {
  return (
    <button
      type="button"
      onClick={onClick}
      className="focus-ring flex h-8 items-center gap-1.5 rounded-[7px] px-2.5 text-[12.5px] font-medium text-fg transition-colors hover:bg-surface-2"
    >
      <span className="text-fg-muted">{icon}</span>
      {label}
    </button>
  );
}
