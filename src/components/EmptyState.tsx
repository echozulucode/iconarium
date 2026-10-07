import { Folder, FolderOpen, LoaderCircle, SearchX } from "lucide-react";
import { formatRelativeTime } from "../lib/format";
import { useLibrary } from "../stores/libraryStore";
import { useSearch } from "../stores/searchStore";

/** Shown when no library is open: the main call to action plus recent folders. */
export function NoLibrary() {
  const recent = useLibrary((s) => s.recent);
  const opening = useLibrary((s) => s.opening);
  return (
    <div className="fade-in flex h-full items-center justify-center overflow-y-auto p-8">
      <div className="w-full max-w-[460px]">
        <LibraryGlyph />
        <h1 className="mt-6 font-[family-name:var(--font-display)] text-[22px] font-semibold tracking-[-0.01em] text-fg">Open an SVG library</h1>
        <p className="mt-1.5 max-w-[400px] text-[13.5px] leading-relaxed text-fg-muted">
          Pick a folder of icons or diagrams. Subfolders are indexed in the background, so you can browse and search right away.
        </p>
        <button
          type="button"
          disabled={opening}
          onClick={() => void useLibrary.getState().pickLibrary()}
          className="focus-ring mt-5 inline-flex h-9 items-center gap-2 rounded-[8px] bg-accent px-4 text-[13px] font-semibold text-accent-fg transition-colors duration-150 hover:bg-accent-hover disabled:opacity-60"
        >
          {opening ? <LoaderCircle size={15} className="spin" /> : <FolderOpen size={15} />}
          Select Folder…
        </button>
        {recent.length > 0 && (
          <div className="mt-8">
            <div className="mb-1.5 text-[12px] font-medium text-fg-subtle">Recent folders</div>
            <ul className="overflow-hidden rounded-[10px] border border-line bg-surface">
              {recent.slice(0, 6).map((r) => (
                <li key={r.id} className="border-b border-line last:border-b-0">
                  <button
                    type="button"
                    onClick={() => void useLibrary.getState().openLibrary(r.path)}
                    className="focus-ring flex w-full items-center gap-3 px-3 py-2.5 text-left transition-colors duration-150 hover:bg-surface-2"
                  >
                    <Folder size={16} className="shrink-0 text-fg-subtle" />
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[13px] font-medium text-fg">{r.displayName}</span>
                      <span className="block truncate text-[11.5px] text-fg-subtle">{r.path}</span>
                    </span>
                    <span className="shrink-0 text-[11.5px] text-fg-subtle">{formatRelativeTime(r.lastOpened)}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
      </div>
    </div>
  );
}

function LibraryGlyph() {
  // A small stack of "asset tiles" in the theme colours — quiet, not a logo.
  return (
    <svg width="64" height="52" viewBox="0 0 64 52" aria-hidden className="text-accent">
      <rect x="14" y="2" width="36" height="28" rx="6" fill="var(--tile)" stroke="var(--border-strong)" />
      <rect x="6" y="10" width="44" height="34" rx="7" fill="var(--surface)" stroke="var(--border-strong)" />
      <rect x="0.5" y="18.5" width="50" height="33" rx="7.5" fill="var(--surface)" stroke="currentColor" />
      <circle cx="16" cy="35" r="6" fill="none" stroke="currentColor" strokeWidth="2" />
      <path d="M27 41l6-11 6 11z" fill="none" stroke="currentColor" strokeWidth="2" strokeLinejoin="round" />
    </svg>
  );
}

const SYNTAX: [string, string][] = [
  ["ethernet switch", "both words, anywhere"],
  ['"motor controller"', "exact phrase"],
  ["pump -legacy", "exclude a word"],
  ["network/*switch*.svg", "glob on path"],
  ["re:^valve-\\d+", "regular expression"],
];

export function NoResults() {
  const q = useSearch((s) => s.resultsQuery);
  return (
    <div className="fade-in flex h-full items-center justify-center p-8">
      <div className="w-full max-w-[420px]">
        <SearchX size={28} strokeWidth={1.6} className="text-fg-subtle" />
        <h2 className="mt-3 text-[16px] font-semibold text-fg">No SVGs match “{q.trim()}”</h2>
        <p className="mt-1 text-[13px] text-fg-muted">Search covers filenames, folders, titles, descriptions and text inside the drawings.</p>
        <dl className="mt-5 grid grid-cols-[auto_1fr] gap-x-5 gap-y-1.5 text-[12.5px]">
          {SYNTAX.map(([k, v]) => (
            <div key={k} className="contents">
              <dt>
                <button
                  type="button"
                  onClick={() => {
                    const s = useSearch.getState();
                    s.setQuery(k);
                    void s.runNow();
                  }}
                  className="focus-ring rounded-[5px] bg-surface-2 px-1.5 py-0.5 font-mono text-[12px] text-fg transition-colors hover:bg-tile-hover"
                >
                  {k}
                </button>
              </dt>
              <dd className="self-center text-fg-muted">{v}</dd>
            </div>
          ))}
        </dl>
      </div>
    </div>
  );
}

export function Scanning({ path }: { path: string }) {
  return (
    <div className="fade-in flex h-full items-center justify-center p-8">
      <div className="flex flex-col items-center text-center">
        <LoaderCircle size={24} className="spin text-accent" />
        <h2 className="mt-3 text-[15px] font-semibold text-fg">Looking for SVG files…</h2>
        <p className="mt-1 max-w-[440px] truncate text-[12.5px] text-fg-muted">{path}</p>
      </div>
    </div>
  );
}

export function EmptyLibrary({ path }: { path: string }) {
  return (
    <div className="fade-in flex h-full items-center justify-center p-8">
      <div className="flex max-w-[420px] flex-col items-center text-center">
        <Folder size={26} strokeWidth={1.6} className="text-fg-subtle" />
        <h2 className="mt-3 text-[15px] font-semibold text-fg">No SVG files in this folder</h2>
        <p className="mt-1 truncate text-[12.5px] text-fg-muted">{path}</p>
        <button
          type="button"
          onClick={() => void useLibrary.getState().pickLibrary()}
          className="focus-ring mt-4 inline-flex h-8 items-center gap-2 rounded-[8px] border border-line px-3 text-[12.5px] font-medium text-fg transition-colors hover:bg-surface-2"
        >
          <FolderOpen size={14} /> Select another folder…
        </button>
      </div>
    </div>
  );
}
