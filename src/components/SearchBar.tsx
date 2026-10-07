import { CircleAlert, LoaderCircle, Search, X } from "lucide-react";
import { useEffect } from "react";
import { focusGrid, searchInputRef } from "../app/focus";
import { formatCount, modKey } from "../lib/format";
import { useLibrary } from "../stores/libraryStore";
import { useSearch } from "../stores/searchStore";
import { useSelection } from "../stores/selectionStore";
import { useViewer } from "../stores/viewerStore";

export function SearchBar() {
  const query = useSearch((s) => s.query);
  const error = useSearch((s) => s.error);
  const searching = useSearch((s) => s.searching);
  const total = useLibrary((s) => s.totalAssets);
  const hasLibrary = useLibrary((s) => s.library !== null);

  // Ctrl+F focuses search from anywhere outside the viewer.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        if (useViewer.getState().openId !== null) return;
        e.preventDefault();
        searchInputRef.current?.focus();
        searchInputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const enterGrid = () => {
    const sel = useSelection.getState();
    if (sel.focus == null || useSearch.getState().index.indexOf(sel.focus) < 0) {
      sel.move("home", 1, 1, { shift: false, ctrl: false });
    }
    focusGrid();
  };

  const onKeyDown = async (e: React.KeyboardEvent<HTMLInputElement>) => {
    const s = useSearch.getState();
    if (e.key === "Escape") {
      e.preventDefault();
      if (s.query) {
        s.setQuery("");
        void s.runNow();
      } else {
        focusGrid();
      }
    } else if (e.key === "Enter") {
      e.preventDefault();
      await s.runNow();
      if (useSearch.getState().results.length) enterGrid();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      if (s.results.length) enterGrid();
    }
  };

  const placeholder = hasLibrary ? `Search ${formatCount(total)} SVGs…` : "Open a folder to search";

  return (
    <div className="relative min-w-0 flex-1">
      <div
        className={[
          "flex h-[34px] items-center gap-2 rounded-[8px] border bg-surface-2 px-2.5 transition-[border-color,box-shadow,background-color] duration-150",
          error
            ? "border-danger shadow-[0_0_0_3px_var(--danger-soft)]"
            : "border-transparent focus-within:border-accent focus-within:bg-surface focus-within:shadow-[0_0_0_3px_var(--accent-soft)]",
        ].join(" ")}
      >
        {searching && query ? (
          <LoaderCircle size={15} className="spin shrink-0 text-fg-subtle" />
        ) : (
          <Search size={15} className="shrink-0 text-fg-subtle" />
        )}
        <input
          ref={(el) => {
            searchInputRef.current = el;
          }}
          value={query}
          disabled={!hasLibrary}
          onChange={(e) => useSearch.getState().setQuery(e.target.value)}
          onKeyDown={onKeyDown}
          placeholder={placeholder}
          spellCheck={false}
          autoComplete="off"
          aria-label="Search SVGs"
          aria-invalid={!!error}
          aria-describedby={error ? "search-error" : undefined}
          className="min-w-0 flex-1 bg-transparent text-[13.5px] text-fg outline-none placeholder:text-fg-subtle disabled:cursor-not-allowed"
        />
        {error && (
          <span id="search-error" role="alert" className="flex min-w-0 max-w-[55%] shrink items-center gap-1.5 text-[12px] text-danger" title={error}>
            <CircleAlert size={14} className="shrink-0" />
            <span className="truncate">{error}</span>
          </span>
        )}
        {query ? (
          <button
            type="button"
            aria-label="Clear search"
            onClick={() => {
              const s = useSearch.getState();
              s.setQuery("");
              void s.runNow();
              searchInputRef.current?.focus();
            }}
            className="focus-ring flex h-5 w-5 shrink-0 items-center justify-center rounded-full text-fg-subtle transition-colors hover:bg-tile-hover hover:text-fg"
          >
            <X size={13} />
          </button>
        ) : (
          hasLibrary && (
            <kbd className="hidden shrink-0 rounded-[4px] border border-line px-1.5 py-px font-sans text-[11px] text-fg-subtle sm:block">
              {modKey}+F
            </kbd>
          )
        )}
      </div>
    </div>
  );
}
