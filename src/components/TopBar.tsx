import { Check, ChevronDown, Folder, FolderOpen, MoreHorizontal } from "lucide-react";
import { useState } from "react";
import type { GallerySize, ViewerBackground } from "../api/types";
import { formatRelativeTime } from "../lib/format";
import { useLibrary } from "../stores/libraryStore";
import { Menu, type MenuEntry } from "./Menu";
import { SearchBar } from "./SearchBar";

export function TopBar() {
  return (
    <header className="flex h-[52px] shrink-0 items-center gap-3 border-b border-line bg-surface px-3">
      <LibrarySwitcher />
      <div className="flex min-w-0 flex-1 justify-center">
        <div className="flex w-full max-w-[760px]">
          <SearchBar />
        </div>
      </div>
      <SizeToggle />
      <SettingsMenu />
    </header>
  );
}

function LibrarySwitcher() {
  const library = useLibrary((s) => s.library);
  const recent = useLibrary((s) => s.recent);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);

  const items: MenuEntry[] = [];
  if (recent.length) {
    items.push({ type: "heading", id: "h", label: "Recent folders" });
    for (const r of recent.slice(0, 8)) {
      const active = library?.path === r.path;
      items.push({
        id: `r-${r.id}`,
        label: r.displayName,
        secondary: `${r.path}  ·  ${formatRelativeTime(r.lastOpened)}`,
        icon: active ? <Check size={14} className="text-accent" /> : <Folder size={14} />,
        onSelect: () => {
          if (!active) void useLibrary.getState().openLibrary(r.path);
        },
      });
    }
    items.push({ type: "separator", id: "s" });
  }
  items.push({
    id: "pick",
    label: "Select Folder…",
    icon: <FolderOpen size={14} />,
    shortcut: "Ctrl+O",
    onSelect: () => void useLibrary.getState().pickLibrary(),
  });

  return (
    <>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        onClick={(e) => setAnchor(anchor ? null : e.currentTarget.getBoundingClientRect())}
        className={[
          "focus-ring flex h-[34px] min-w-0 max-w-[260px] shrink-0 items-center gap-2 rounded-[8px] px-2.5 text-left transition-colors duration-150",
          anchor ? "bg-surface-2" : "hover:bg-surface-2",
        ].join(" ")}
        title={library?.path}
      >
        <span className="flex h-6 w-6 shrink-0 items-center justify-center rounded-[6px] bg-accent-soft text-accent">
          <Folder size={14} strokeWidth={2} />
        </span>
        <span className="truncate text-[13px] font-semibold text-fg">{library?.displayName ?? "No folder"}</span>
        <ChevronDown size={14} className={`shrink-0 text-fg-subtle transition-transform duration-150 ${anchor ? "rotate-180" : ""}`} />
      </button>
      {anchor && <Menu at={anchor} items={items} onClose={() => setAnchor(null)} minWidth={320} label="Library" />}
    </>
  );
}

const SIZES: { id: GallerySize; label: string; title: string }[] = [
  { id: "small", label: "S", title: "Small thumbnails" },
  { id: "medium", label: "M", title: "Medium thumbnails" },
  { id: "large", label: "L", title: "Large thumbnails" },
];

function SizeToggle() {
  const size = useLibrary((s) => s.settings?.gallerySize ?? "medium");
  return (
    <div role="radiogroup" aria-label="Thumbnail size" className="flex h-[30px] shrink-0 items-center rounded-[8px] bg-surface-2 p-[3px]">
      {SIZES.map((s) => {
        const active = s.id === size;
        return (
          <button
            key={s.id}
            type="button"
            role="radio"
            aria-checked={active}
            aria-label={s.title}
            title={s.title}
            onClick={() => void useLibrary.getState().updateSettings({ gallerySize: s.id })}
            className={[
              "focus-ring h-6 w-7 rounded-[6px] text-[12px] font-semibold transition-[background-color,color,box-shadow] duration-150",
              active ? "bg-surface text-fg shadow-[0_0_0_1px_var(--border),0_1px_2px_rgba(0,0,0,0.06)]" : "text-fg-subtle hover:text-fg",
            ].join(" ")}
          >
            {s.label}
          </button>
        );
      })}
    </div>
  );
}

const BACKGROUNDS: { id: ViewerBackground; label: string }[] = [
  { id: "checkerboard", label: "Checkerboard" },
  { id: "white", label: "White" },
  { id: "dark", label: "Dark" },
];

function SettingsMenu() {
  const settings = useLibrary((s) => s.settings);
  const [anchor, setAnchor] = useState<DOMRect | null>(null);
  const update = useLibrary.getState().updateSettings;

  const items: MenuEntry[] = settings
    ? [
        { type: "heading", id: "hb", label: "Viewer background" },
        ...BACKGROUNDS.map(
          (b): MenuEntry => ({
            id: `bg-${b.id}`,
            label: b.label,
            role: "menuitemradio",
            checked: settings.viewerBackground === b.id,
            keepOpen: true,
            onSelect: () => void update({ viewerBackground: b.id }),
          }),
        ),
        { type: "separator", id: "s1" },
        { type: "heading", id: "hg", label: "Gallery" },
        {
          id: "reldir",
          label: "Show folder under filename",
          checked: settings.showRelativeDir,
          keepOpen: true,
          onSelect: () => void update({ showRelativeDir: !settings.showRelativeDir }),
        },
        {
          id: "prefill",
          label: "Render thumbnails in background",
          checked: settings.prefillThumbnails,
          keepOpen: true,
          onSelect: () => void update({ prefillThumbnails: !settings.prefillThumbnails }),
        },
        { type: "separator", id: "s2" },
        { type: "heading", id: "hc", label: "Clipboard" },
        {
          id: "svgtext",
          label: "Include SVG markup as text",
          checked: settings.clipboardIncludeSvgText,
          keepOpen: true,
          onSelect: () => void update({ clipboardIncludeSvgText: !settings.clipboardIncludeSvgText }),
        },
      ]
    : [];

  return (
    <>
      <button
        type="button"
        aria-label="Settings"
        aria-haspopup="menu"
        aria-expanded={!!anchor}
        title="Settings"
        disabled={!settings}
        onClick={(e) => setAnchor(anchor ? null : e.currentTarget.getBoundingClientRect())}
        className={[
          "focus-ring flex h-[30px] w-[30px] shrink-0 items-center justify-center rounded-[8px] text-fg-muted transition-colors duration-150",
          anchor ? "bg-surface-2 text-fg" : "hover:bg-surface-2 hover:text-fg",
        ].join(" ")}
      >
        <MoreHorizontal size={17} />
      </button>
      {anchor && <Menu at={anchor} items={items} onClose={() => setAnchor(null)} align="end" minWidth={268} label="Settings" />}
    </>
  );
}
