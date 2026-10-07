import { Check, LoaderCircle } from "lucide-react";
import type { ScanStatus } from "../api/types";
import { formatCount, plural } from "../lib/format";
import { useLibrary } from "../stores/libraryStore";
import { useSearch } from "../stores/searchStore";
import { useSelection } from "../stores/selectionStore";

export function scanLabel(s: ScanStatus): { text: string; busy: boolean; progress: number | null } {
  switch (s.phase) {
    case "loading":
      return { text: s.message || "Loading catalog…", busy: true, progress: null };
    case "discovering":
      return { text: s.discovered > 0 ? `Indexing ${formatCount(s.discovered)}…` : s.message || "Looking for SVG files…", busy: true, progress: null };
    case "reconciling":
      return { text: s.message || "Checking for changes…", busy: true, progress: null };
    case "processing":
      return {
        text: `Reading contents ${formatCount(s.processed)} of ${formatCount(s.total)}`,
        busy: true,
        progress: s.total > 0 ? s.processed / s.total : null,
      };
    default:
      // Backend may append background preview progress ("Indexed · generating previews (n left)").
      return { text: s.message?.startsWith("Indexed") ? s.message : "Indexed", busy: false, progress: null };
  }
}

export function StatusBar() {
  const library = useLibrary((s) => s.library);
  const scan = useLibrary((s) => s.scan);
  const total = useLibrary((s) => s.totalAssets);
  const results = useSearch((s) => s.results.length);
  const hasQuery = useSearch((s) => s.resultsQuery.trim().length > 0);
  const ms = useSearch((s) => s.lastMs);
  const selected = useSelection((s) => s.selected.size);

  if (!library) {
    return <footer className="h-7 shrink-0 border-t border-line bg-surface" />;
  }

  const label = scanLabel(scan);

  return (
    <footer className="grid h-7 shrink-0 grid-cols-[1fr_auto_1fr] items-center gap-4 border-t border-line bg-surface px-3 text-[12px] text-fg-muted tnum">
      <div className="flex min-w-0 items-center gap-2">
        {hasQuery ? (
          <span className="truncate">
            <span className="text-fg">{plural(results, "result")}</span>
            <span className="text-fg-subtle"> of {formatCount(total)}</span>
            {ms !== null && <span className="text-fg-subtle"> in {ms < 10 ? ms.toFixed(1) : Math.round(ms)} ms</span>}
          </span>
        ) : (
          <span className="truncate">{plural(total, "asset")}</span>
        )}
      </div>
      <div className="flex items-center justify-center gap-1.5" role="status" aria-live="polite">
        {label.busy ? <LoaderCircle size={12} className="spin text-accent" /> : <Check size={13} strokeWidth={2.4} className="text-success" />}
        <span className={label.busy ? "" : "text-fg-subtle"}>{label.text}</span>
        {label.progress !== null && (
          <span className="ml-1 h-1 w-20 overflow-hidden rounded-full bg-tile">
            <span className="block h-full rounded-full bg-accent transition-[width] duration-200" style={{ width: `${Math.round(label.progress * 100)}%` }} />
          </span>
        )}
      </div>
      <div className="flex justify-end">{selected > 0 && <span className="text-fg">{formatCount(selected)} selected</span>}</div>
    </footer>
  );
}
